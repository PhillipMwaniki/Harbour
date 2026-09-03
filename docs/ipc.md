# IPC contract

The frontend talks to the Rust core exclusively through Tauri commands and
events. This file is the source of truth for that boundary; changing a command
signature means changing this file and `src/ipc/types.ts` in the same commit.

## Conventions

- Commands are `snake_case`. Events are `domain:event`.
- Arguments are camelCase on the JS side and snake_case in Rust; Tauri maps
  between them automatically.
- Every command returns `Result<T, AppError>`. `AppError` serialises as
  `{ code, message }`, where `code` is stable and safe to branch on.
- **Terminal payloads are bytes, not strings.** Output crosses the boundary as
  a raw `ArrayBuffer` on a `Channel`, never as JSON-encoded text: pty output is
  not guaranteed to be valid UTF-8, and stringifying it at 50 MB/s is not
  viable.

## Backpressure

Output flow is ack-based, and the frontend is required to participate:

1. The backend coalesces pty reads into batches of up to 32 KB (or 8 ms,
   whichever comes first) and sends them on the session channel.
2. Each batch is charged against a 1 MB in-flight budget.
3. The frontend calls `term.write(bytes, callback)` and, in the callback,
   accumulates the byte count; `OutputAcker` flushes it via `session_ack` on the
   next animation frame, or immediately past 256 KB.
4. Once the budget is exhausted the backend stops pulling from the pty, which
   propagates down to the process producing the output.

A frontend that never acks will see output stop after 1 MB. This is by design.

## Commands

### Sessions

| Command | Arguments | Returns |
| --- | --- | --- |
| `session_open` | `shellId?: string`, `cols: number`, `rows: number`, `cwd?: string` | `SessionInfo` |
| `session_subscribe` | `sessionId: string`, `onData: Channel<ArrayBuffer>` | `void` |
| `session_write` | `sessionId: string`, `data: number[]` | `void` |
| `session_resize` | `sessionId: string`, `cols: number`, `rows: number` | `void` |
| `session_ack` | `sessionId: string`, `bytes: number` | `void` |
| `session_set_title` | `sessionId: string`, `title: string` | `void` |
| `session_close` | `sessionId: string` | `void` |
| `session_list` | - | `SessionInfo[]` |

`session_subscribe` may be called **once** per session; a second call returns
`ALREADY_SUBSCRIBED` rather than silently splitting the stream. Output produced
between `session_open` and `session_subscribe` is buffered, not dropped.

`session_write` currently passes input as a JSON number array. Keystrokes are
tiny so this is not on the hot path, but a large paste is measurably slower than
it needs to be; moving it to a raw request body is tracked for milestone 4.

### Shells

| Command | Arguments | Returns |
| --- | --- | --- |
| `shell_list` | - | `ShellSpec[]` |

Best default first, with `default: true` on that entry. On Windows this
enumerates PowerShell 7, Windows PowerShell, cmd, Git Bash and every installed
WSL distribution; elsewhere `$SHELL` followed by the usual suspects.

## Events

| Event | Payload |
| --- | --- |
| `session:opened` | `SessionInfo` |
| `session:closed` | `{ sessionId, reason: "exit" \| "killed" \| "error", exitCode: number \| null }` |

`session:closed` fires when the child process is reaped, whether it exited on
its own or was killed by `session_close`. The session is already gone from the
backend's map by the time the event arrives.

## Error codes

| Code | Meaning |
| --- | --- |
| `SESSION_NOT_FOUND` | No live session with that id (it may have just exited) |
| `ALREADY_SUBSCRIBED` | `session_subscribe` called twice for one session |
| `SHELL_NOT_FOUND` | Requested shell id is not installed |
| `PTY_OPEN_FAILED` | The OS refused to allocate a pty |
| `SPAWN_FAILED` | The shell binary could not be started |
| `IO_ERROR` | Read/write against the pty failed |
| `INTERNAL` | Unclassified; always a bug worth a log line |

## Not yet implemented

The spec defines further domains - `host_*`, `sftp_*`, `transfer_*`,
`forward_*`, `fleet_*`, and the interactive `connection:hostkey_prompt` /
`connection:auth_prompt` round-trips. They are listed here so the naming stays
consistent when they land, but no handler exists yet.
