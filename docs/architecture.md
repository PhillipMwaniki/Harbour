# Architecture

Harbour is a Tauri 2 app: a React frontend in the platform webview, and a Rust
core that owns every connection, process and file handle. The webview is a
rendering surface, not a place where privileged work happens.

```
React frontend (webview)
  TabBar - PaneTree -> TerminalView(s) - SearchBar, highlight layer
           ConnectDialog, HostKeyDialog, SecretDialog, SettingsDialog
           FileDock -> FilePane (remote over SFTP, and local)
           TransferPanel, ConflictDialog
  SessionTree - HostDialog, ImportDialog
  Zustand stores: sessions, prompts, vault, settings, files
  lib/       panes (split tree), keymap, highlight, themes
        |  invoke() / listen() / Channel<bytes>
Rust core
  commands/  thin handlers: validate, dispatch, no logic
  session/   SessionManager -> SessionHandle per open session
             local.rs   (portable-pty: ConPTY / forkpty)
             reader.rs  (batching + ack backpressure)
             logging.rs (session output to a file, on its own thread)
             shell.rs   (what can we launch here?)
  ssh/       client.rs      (connect, authenticate, request a pty)
             transport.rs   (the running channel; opens further channels)
             sftp.rs        (SFTP on the terminal's connection; the registry)
             known_hosts.rs (trust, and nothing else)
             agent.rs       (SSH_AUTH_SOCK / OpenSSH pipe / Pageant)
  prompt.rs  round-trip questions to the user
  files/     directory listings in one shape
             local.rs  (std::fs, made to look like SFTP)
  transfer/  the queue
             engine.rs (slots, pause, cancel, conflicts, snapshots)
             copy.rs   (planning a tree, then moving one file at a time)
  edit.rs    a remote file in the local editor, uploaded on save
  settings/  preferences, and the colour schemes imported into them
             store.rs  (settings.json: atomic write, never fatal)
             scheme.rs (VS Code / Windows Terminal / iTerm importers)
             color.rs  (deriving chrome tokens from a terminal palette)
  vault/     saved hosts, and the imports that fill them
             store.rs      (SQLite: folders and hosts, no secrets)
             secrets.rs    (OS keychain: passwords and passphrases)
             ssh_config.rs (~/.ssh/config)
             xshell.rs     (.xsh export directories)
             import.rs     (both sources -> reviewable candidates)
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

## A tab is a tree of panes

A tab holds a binary tree of splits with a terminal at every leaf. The tree
(`src/lib/panes.ts`) holds pane ids only; the panes themselves live in a flat
map beside it on the tab. That separation is what keeps a title arriving, or a
session attaching, from rebuilding the layout - and it is what lets the tree
operations be pure functions with tests that never touch a terminal.

A pane's terminal is created once and kept for the life of the pane. Hiding a
tab sets `display: none` rather than unmounting, because unmounting an xterm
instance loses its scrollback, and closing a pane is the only thing that ends
the session behind it.

## SFTP shares the terminal's connection

An SSH connection carries any number of channels, and SFTP is just one more: a
channel with the `sftp` subsystem on it. The file pane therefore never
connects or authenticates. It asks the terminal's connection for a second
channel and rides the trust and credentials that connection already
established - one host key prompt, one password, however many panes.

russh's connection handle is single-owner and lives in the transport's writer
task, so the channel is opened *through* that task: `ChannelOpener` is a
handle on the writer's command queue, and `Command::OpenChannel` asks the task
to open a session channel and hand it back. Nothing else ever holds the
connection, which is what keeps "close the terminal" meaning "close
everything".

`ssh::sftp::Connections` maps live session ids to their openers, and to the
SFTP channel once one has been opened. It sits beside the session manager
rather than inside it: the manager knows about transports and nothing about
SSH, and this is the one place that needs to. A local shell has no entry,
which is how `sftp_*` learns a session has no remote side.

## Transfers stop to ask

A transfer is a tokio task. It waits for one of its session's two slots, plans
the whole tree first - so the totals are known and every directory exists
before its files - and then copies one file at a time, checking a `Control`
between 256 KB chunks. That check is where a pause takes hold and where a
cancel arrives, so neither waits for a file to finish.

Every change to a transfer goes out through one emitter as the whole
snapshot: progress at most a few times a second, state changes always. The
frontend keeps the latest copy of each and renders it; it never derives state
from a sequence of deltas, and a missed event costs nothing.

A destination that already exists is a decision, not an error. Under the
default policy the task parks on a `oneshot` and the transfer shows as
`conflict` with both sides described; the answer can cover the rest of the
transfer. Resume is offered only when the destination is smaller than the
source, because resuming onto anything else would produce a corrupt file. A
directory in the way of a file is the one case that fails outright: no answer
to "overwrite?" makes it right.

Open-in-editor is the same channel used differently. The file is downloaded to
a private directory, handed to the OS, and its *directory* is watched -
editors save in place or by writing and renaming, and watching the directory
catches both - with a short settle so one save is one upload. The upload
truncates: a save shorter than the file it replaces must not leave the old
tail behind.

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
8. **The database and the keychain are separate on purpose.** SQLite holds the
   tree; the OS keychain holds the secrets, addressed by host id. Neither
   contains the other's data, so a copied vault file is not a credential leak
   and a keychain entry on its own names nothing.
9. **A question to the user is a round trip, not an argument.** The core cannot
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
| Vault and keychain calls | `spawn_blocking`; SQLite is synchronous and macOS can prompt |
| Session log writer | `harbour-session-log` OS thread, one per open log |
| SSH channel reader | tokio task, one per session |
| SSH channel writer, holding the session handle | tokio task, one per session |
| SFTP channel open | inside the writer task, on request; the channel itself is driven by `russh-sftp` |
| Local directory listing | `spawn_blocking`; `std::fs` and drive probing touch the disk |
| Transfer copy | one tokio task per transfer, two running at a time per session |
| Save watcher | `notify`'s own thread, bridged to one tokio task per edit |
| russh session event loop | tokio task, spawned by russh |

`LocalTransport` guards its writer, master pty and killer behind separate
mutexes so a slow write cannot block a resize or a kill. `SshTransport` has no
locks at all: every operation is a message on one queue, which also keeps a
resize from overtaking input typed before it.

Backpressure works the same way on both, one level down. The pty reader thread
blocks on a full queue, which stops it reading the pty; the SSH reader task
stops awaiting the channel, which stops russh adjusting the window, which stops
the remote sending.

## Settings, and what they are for

`settings.json` sits beside the vault and holds preferences: theme, font,
scrollback, keymap overrides, highlight rules, per-host theme overrides and
where session logs go. It is a separate file from the vault for the same reason
the vault and the keychain are separate - it is a file people copy between
machines, paste into an issue and hand-edit - and it therefore holds nothing
that matters if it leaks.

Two rules follow from that. A settings file that will not parse never stops
Harbour from starting: it is moved aside to `settings.invalid.json` and
replaced with defaults, because a terminal that refuses to open over a stray
comma is useless exactly when it is needed. And every write goes through a
temporary file and a rename, so an interrupted save leaves the old settings
rather than half a document.

The built-in themes stay in `src/lib/themes.ts` and never cross the IPC
boundary; only *imported* ones are stored, because only they have to survive a
restart. An imported scheme describes a terminal palette and nothing else, so
`settings/scheme.rs` derives the chrome tokens by mixing the scheme's own
background and foreground - a warm scheme gets warm borders.

The keymap is data (`src/lib/keymap.ts`) rather than a switch statement in a
keydown handler, because two other things need to read it: the settings file,
which overrides it, and the settings dialog, which lists it. xterm is told
which chords the keymap has claimed, so `Ctrl+Shift+[` moves the focus instead
of also sending an escape to the shell.

## What is not here yet

Milestone 3 covers saved hosts. What the vault does not do yet: reordering
within a folder (positions exist, nothing sets them), a master password, and
encrypted export - the last two are milestone 8. Host certificates, agent
forwarding and port forwarding are later still.

The transfer queue is not persisted: it lives as long as the app does, and a
transfer whose session closes is cancelled with its last state saying so.
Port forwarding and the fleet runner each add a sibling module under
`src-tauri/src/`, and the `SessionKind` enum grows a variant per transport. The
IPC shapes for those are reserved in `docs/ipc.md`.
