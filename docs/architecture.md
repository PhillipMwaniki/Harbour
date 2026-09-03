# Architecture

Harbour is a Tauri 2 app: a React frontend in the platform webview, and a Rust
core that owns every connection, process and file handle. The webview is a
rendering surface, not a place where privileged work happens.

```
React frontend (webview)
  TabBar - TerminalView(s) - ConnectDialog, HostKeyDialog, SecretDialog
           [SftpPane, TransferQueue: milestone 5+]
  Zustand stores: sessions, prompts, ui
        |  invoke() / listen() / Channel<bytes>
Rust core
  commands/  thin handlers: validate, dispatch, no logic
  session/   SessionManager -> SessionHandle per open session
             local.rs  (portable-pty: ConPTY / forkpty)
             reader.rs (batching + ack backpressure)
             shell.rs  (what can we launch here?)
  ssh/       client.rs      (connect, authenticate, request a pty)
             transport.rs   (the running channel)
             known_hosts.rs (trust, and nothing else)
             agent.rs       (SSH_AUTH_SOCK / OpenSSH pipe / Pageant)
  prompt.rs  round-trip questions to the user
  vault/     host storage and imports
             xshell.rs (.xsh session import; SQLite store lands in ms 3)
  telemetry.rs  tracing -> rotating file, never secrets
```

## One session, two transports

A pty and an SSH channel have almost nothing in common, but the session layer
needs only three things from either: write, resize, kill. That is the
`Transport` trait, and it is the whole of the difference.

Everything above it is shared. Both transports hand the manager a
`Receiver<Vec<u8>>`; the same pump batches it, the same ack budget throttles it,
the same terminal renders it. `session_write`, `session_resize`,
`session_subscribe` and `session_close` do not know which kind of session they
are talking to, and neither does the frontend once `ssh_connect` has returned.

The methods must all return promptly, which is why the SSH transport queues
commands for a writer task rather than awaiting the remote inline: tearing down
a session cannot be allowed to block on a host that has stopped answering.

## Rules the code follows

1. **The core owns lifetime.** The frontend asks to open and close things; it
   never holds a handle. A session that dies announces itself with
   `session:closed`, and the UI reacts.
2. **Bytes, not strings, on hot paths.** See `docs/ipc.md`.
3. **Backpressure is mandatory, not advisory.** The output pump stops at 1 MB
   unacknowledged. This is what keeps `cat` of a large file from locking up the
   webview.
4. **One thread per blocking pty, one task per pump.** `portable-pty` is
   blocking, so reads and `waitpid` live on dedicated OS threads that hand work
   to tokio through channels.
5. **A panicking session must not take the app down.** The release profile
   deliberately does not set `panic = "abort"`, so a panic in a session thread
   or pump task stays contained.
6. **Releasing a pty happens off the caller's thread.** On Windows
   `ClosePseudoConsole` blocks until the console output buffer is drained, so
   the reader thread keeps draining to EOF after its channel closes, and
   teardown runs on a dedicated thread. Neither a command handler nor the UI
   thread may ever wait on a console winding down.
7. **Secrets never reach the webview or the logs.** The user types a password
   into a dialog and it goes straight to the authentication attempt that asked
   for it: no store, no log line, no retention past the attempt. Nothing sends
   a secret *to* the webview, ever.
8. **A question to the user is a round trip, not an argument.** The core cannot
   know in advance whether a host key is unknown or what a server will ask for,
   so `prompt.rs` parks the connection on a `oneshot` and emits an event. A
   prompt nobody answers times out; a dropped attempt deregisters its own
   prompt on the way out.

## Threading model

| Component | Where it runs |
| --- | --- |
| Pty read loop | `harbour-pty-read` OS thread, one per session |
| Child reaper | `harbour-pty-wait` OS thread, one per session |
| Session teardown | `harbour-pty-close` OS thread, one per close |
| Output pump (batching, backpressure) | tokio task on Tauri's runtime |
| Command handlers | tokio tasks; `shell_list` uses `spawn_blocking` |
| Local writes and resizes | caller's task, holding a short-lived `parking_lot` lock |
| SSH channel reader | tokio task, one per session |
| SSH channel writer, holding the session handle | tokio task, one per session |
| russh session event loop | tokio task, spawned by russh |

`LocalTransport` guards its writer, master pty and killer behind separate
mutexes so a slow write cannot block a resize or a kill. `SshTransport` has no
locks at all: every operation is a message on one queue, which also keeps a
resize from overtaking input typed before it.

Backpressure works the same way on both, one level down. The pty reader thread
blocks on a full queue, which stops it reading the pty; the SSH reader task
stops awaiting the channel, which stops russh adjusting the window, which stops
the remote sending.

## What is not here yet

The theme system is frontend-only: `src/lib/themes.ts` holds the catalogue and
`src/stores/settings.ts` publishes the chrome colours as `--hb-*` CSS custom
properties. Nothing about a theme crosses the IPC boundary.

Milestone 2 covers SSH shells. Hosts are described by hand in the connect
dialog and nothing about them is saved: the SQLite vault, the session tree and
the keyring land in milestone 3, and `AuthChoice` lists will come from there
rather than from a form. Host certificates, agent forwarding and port
forwarding are later still.

SFTP, transfers and the fleet runner each add a sibling module under
`src-tauri/src/`, and the `SessionKind` enum grows a variant per transport. The
IPC shapes for those are reserved in `docs/ipc.md`.
