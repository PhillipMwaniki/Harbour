# Architecture

Harbour is a Tauri 2 app: a React frontend in the platform webview, and a Rust
core that owns every connection, process and file handle. The webview is a
rendering surface, not a place where privileged work happens.

```
React frontend (webview)
  TabBar - TerminalView(s) - [SftpPane, TransferQueue: milestone 5+]
  Zustand stores: sessions, ui
        |  invoke() / listen() / Channel<bytes>
Rust core
  commands/  thin handlers: validate, dispatch, no logic
  session/   SessionManager -> SessionHandle per open session
             local.rs  (portable-pty: ConPTY / forkpty)
             reader.rs (batching + ack backpressure)
             shell.rs  (what can we launch here?)
  vault/     host storage and imports
             xshell.rs (.xsh session import; SQLite store lands in ms 3)
  telemetry.rs  tracing -> rotating file, never secrets
```

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
7. **Secrets never reach the webview or the logs.** Nothing in milestone 1
   handles credentials yet; the rule is stated here so it predates the code
   that will.

## Threading model

| Component | Where it runs |
| --- | --- |
| Pty read loop | `harbour-pty-read` OS thread, one per session |
| Child reaper | `harbour-pty-wait` OS thread, one per session |
| Session teardown | `harbour-pty-close` OS thread, one per close |
| Output pump (batching, backpressure) | tokio task on Tauri's runtime |
| Command handlers | tokio tasks; `shell_list` uses `spawn_blocking` |
| Writes and resizes | caller's task, holding a short-lived `parking_lot` lock |

`SessionHandle` guards its writer, master pty and killer behind separate
mutexes so a slow write cannot block a resize or a kill.

## What is not here yet

The theme system is frontend-only: `src/lib/themes.ts` holds the catalogue and
`src/stores/settings.ts` publishes the chrome colours as `--hb-*` CSS custom
properties. Nothing about a theme crosses the IPC boundary.

Milestone 1 covers local shells only. SSH (`russh`), the SQLite vault, SFTP and
transfers, port forwarding, and the fleet runner each add a sibling module under
`src-tauri/src/`, and the `SessionKind` enum grows a variant per transport. The
IPC shapes for those are reserved in `docs/ipc.md`.
