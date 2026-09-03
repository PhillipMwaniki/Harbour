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

### Connections

| Command | Arguments | Returns |
| --- | --- | --- |
| `ssh_connect` | `target: SshTarget`, `methods: AuthChoice[]`, `cols: number`, `rows: number` | `SessionInfo` |
| `connection_respond` | `promptId: string`, `answer: object` | `void` |

`ssh_connect` resolves only once the session is live. The host key and
credential round-trips happen *inside* the call, as events, so the frontend has
one promise to track and one place to render a failure. The returned
`SessionInfo` has `kind: "ssh"`; everything after that - `session_subscribe`,
`session_write`, `session_resize`, `session_close` - is the same as for a local
shell.

```ts
type SshTarget = { host: string; port: number; user: string };

type AuthChoice =
  | { kind: "agent" }
  | { kind: "key"; path: string }
  | { kind: "password" }
  | { kind: "keyboardInteractive" };
```

`methods` is tried in order, skipping any the server does not offer. An empty
array means the backend's default: agent, password, keyboard-interactive.

`connection_respond` answers whichever prompt raised `promptId`; the payload
shape depends on the prompt and is validated where it is awaited. Answering an
id that is no longer waiting - it timed out, or its connection died - returns
`PROMPT_NOT_FOUND`.

### Vault

| Command | Arguments | Returns |
| --- | --- | --- |
| `vault_tree` | - | `VaultTree` |
| `vault_create_folder` | `parentId: string \| null`, `name: string` | `Folder` |
| `vault_rename_folder` | `folderId: string`, `name: string` | `void` |
| `vault_move_folder` | `folderId: string`, `parentId: string \| null` | `void` |
| `vault_delete_folder` | `folderId: string` | `void` |
| `vault_create_host` | `host: HostInput` | `Host` |
| `vault_update_host` | `hostId: string`, `host: HostInput` | `Host` |
| `vault_delete_host` | `hostId: string` | `void` |
| `vault_move_host` | `hostId: string`, `folderId: string \| null` | `void` |
| `vault_forget_secrets` | `hostId: string` | `void` |
| `vault_keychain_available` | - | `boolean` |
| `vault_preview_ssh_config` | `path?: string` | `ImportPreview` |
| `vault_preview_xshell` | `path: string` | `ImportPreview` |
| `vault_apply_import` | `candidates: ImportCandidate[]`, `username: string \| null` | `ImportResult` |
| `host_connect` | `hostId: string`, `cols: number`, `rows: number` | `SessionInfo` |

`vault_tree` returns the whole tree in one call: a few hundred hosts at most,
so paging it would cost more than it saves.

**No command takes or returns a secret.** A `Host` says which methods to try
and whether a password is expected (`hasSavedPassword`); the password itself is
in the OS keychain, keyed by host id, and only ever moves between the keychain
and the connection that needs it.

`vault_delete_folder` deletes everything underneath, hosts and saved secrets
included. The UI confirms first.

`vault_forget_secrets` removes the host's saved password *and* key passphrase.
Removing something that is not there is success.

`vault_preview_*` write nothing: they read the source and return what they
found for review. `vault_apply_import` is what writes, and it skips any
candidate carrying a `skipReason` or lacking a username with no `username`
fallback to fall back on, rather than importing a guess.

`host_connect` is `ssh_connect` with the vault behind it. The prompts are the
same, except that a saved password is taken from the keychain without asking,
and a `connection:auth_prompt` for a saved host carries `canRemember: true` so
the answer can be saved.

## Events

| Event | Payload |
| --- | --- |
| `session:opened` | `SessionInfo` |
| `session:closed` | `{ sessionId, reason: "exit" \| "killed" \| "error", exitCode: number \| null }` |

`session:closed` fires when the child process is reaped, whether it exited on
its own or was killed by `session_close`. For an SSH session it fires when the
remote shell exits (`"exit"`, with its status), when Harbour closed it
(`"killed"`), or when the connection dropped underneath it (`"error"`). A local
session reports `"exit"` with the status the OS gave, since a pty cannot tell
the three apart. The session is already gone from the backend's map by the time
the event arrives.

### Connection prompts

Both carry a `promptId` to pass back to `connection_respond`, and both block the
connection attempt until they are answered or the five-minute timeout expires.

| Event | Payload |
| --- | --- |
| `connection:hostkey_prompt` | `{ promptId, host, port, status, algorithm, fingerprint, stored }` |
| `connection:auth_prompt` | `{ promptId, host, user, kind, label, instruction, echo, canRemember }` |

`status` is `"unknown"` (nothing on file: trust on first use) or `"changed"` (a
key of the same type is on file and does not match). `stored` lists what is
already recorded - the conflicting key, or keys of other types for the same
host - each with `algorithm`, `fingerprint`, `source` and `line`. Answer with
`{ accept: boolean, remember: boolean }`; `accept: false` aborts the connection
with `SSH_HOSTKEY_REJECTED`.

`kind` is `"password"`, `"passphrase"` or `"challenge"` (a question the server
worded, under keyboard-interactive). `echo` is true only when the server says
the answer is not secret. `canRemember` says whether there is anywhere to save
the answer - a saved host, on a machine with a working keychain - and is false
for an ad-hoc connection.

Answer with `{ secret: string | null, remember: boolean }`. `null` means the
user dismissed the prompt: the attempt stops without spending another
authentication try. `remember` is honoured only when `canRemember` was true,
and never for a `challenge`, since a one-time code saved and replayed is worse
than useless.

## Error codes

| Code | Meaning |
| --- | --- |
| `SESSION_NOT_FOUND` | No live session with that id (it may have just exited) |
| `ALREADY_SUBSCRIBED` | `session_subscribe` called twice for one session |
| `SHELL_NOT_FOUND` | Requested shell id is not installed |
| `PTY_OPEN_FAILED` | The OS refused to allocate a pty |
| `SPAWN_FAILED` | The shell binary could not be started |
| `IO_ERROR` | Read/write against the pty failed |
| `SSH_CONNECT_FAILED` | The host could not be reached (DNS, routing, refused) |
| `SSH_AUTH_FAILED` | Every method was exhausted, or the user cancelled |
| `SSH_HOSTKEY_REJECTED` | The host key was refused, revoked, or a certificate |
| `SSH_HOSTKEY_CHANGED` | Reserved for a non-interactive refusal on a changed key |
| `SSH_KEY_LOAD_FAILED` | A key file could not be read or decrypted |
| `SSH_AGENT_UNAVAILABLE` | No agent answered, or it holds no identities |
| `SSH_CHANNEL_FAILED` | The remote refused a channel, pty or shell request |
| `SSH_PROTOCOL_ERROR` | Anything russh reports below that level |
| `HOST_NOT_FOUND` | No saved host with that id |
| `FOLDER_NOT_FOUND` | No folder with that id |
| `VAULT_ERROR` | The store refused: bad input, or SQLite said no |
| `KEYRING_UNAVAILABLE` | The OS keychain could not be reached |
| `PROMPT_NOT_FOUND` | `connection_respond` for a prompt no longer waiting |
| `PROMPT_TIMED_OUT` | Nobody answered a prompt within five minutes |
| `INTERNAL` | Unclassified; always a bug worth a log line |

`SSH_AUTH_FAILED` carries a message naming what was tried, what the server
never offered, and what it still accepts. "Authentication failed" on its own is
useless when the real problem is `PasswordAuthentication no`.

## Not yet implemented

The spec defines further domains - `sftp_*`, `transfer_*`, `forward_*` and
`fleet_*`. They are listed here so the naming stays consistent when they land,
but no handler exists yet.
