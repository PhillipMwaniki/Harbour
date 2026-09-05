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
| `session_write` | raw body: the bytes; `session-id` header | `void` |
| `session_resize` | `sessionId: string`, `cols: number`, `rows: number` | `void` |
| `session_ack` | `sessionId: string`, `bytes: number` | `void` |
| `session_set_title` | `sessionId: string`, `title: string` | `void` |
| `session_close` | `sessionId: string` | `void` |
| `session_list` | - | `SessionInfo[]` |
| `session_log_start` | `sessionId: string`, `path: string`, `format: LogFormat`, `append: boolean` | `LogStatus` |
| `session_log_stop` | `sessionId: string` | `LogStatus` |
| `session_log_status` | `sessionId: string` | `LogStatus` |

`session_subscribe` may be called **once** per session; a second call returns
`ALREADY_SUBSCRIBED` rather than silently splitting the stream. Output produced
between `session_open` and `session_subscribe` is buffered, not dropped.

`session_write` sends input as a **raw request body**, with the session id in a
`session-id` header, because the body is the payload. Keystrokes are tiny, but a
paste is not: a 2 MB buffer as `[104,101,...]` is roughly four times the bytes
and a JSON parse at the far end. A JSON array body is still accepted, since the
webview falls back to one when the custom protocol is unavailable.

### Session logging

A log is attached to the session's output pump, one level below the IPC
boundary, so what lands in the file is exactly what the terminal was sent -
never what was typed, and never a re-render of the webview's scrollback. It
starts when it is asked to: what is already on screen is not in the file.

```ts
type LogFormat = "raw" | "plain";

type LogStatus = {
  active: boolean;
  path: string | null;
  format: LogFormat | null;
  bytes: number;
  /** The writer failed. The session carries on regardless. */
  error: string | null;
};
```

`raw` writes every byte, escape sequences included. `plain` removes escape
sequences and resolves carriage returns, so a progress bar leaves one line
rather than three hundred, and the file reads the way the screen did.

Starting a log on a session that already has one replaces it, so "log somewhere
else" is a single action. `session_log_stop` on a session that is not being
logged is success, not an error. Writes happen on their own thread: a log on a
full disk records that it fell behind and the session carries on, because
blocking the pump would throttle the session for a reason nobody asked for.

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
| `telnet_connect` | `host: string`, `port: number`, `cols: number`, `rows: number` | `SessionInfo` |
| `serial_ports` | - | `SerialPortInfo[]` |
| `serial_connect` | `path: string`, `baud: number` | `SessionInfo` |

`telnet_connect` opens a raw TCP telnet session. It has no authentication or
host key of its own - whatever login the far end wants happens in the terminal -
so it is a single call with no round trips. The returned `SessionInfo` has
`kind: "telnet"`, and `session_subscribe` / `session_write` / `session_resize` /
`session_close` work exactly as for the other kinds. `port` `0` means 23. The
telnet negotiation (option offers, window size) is handled in the core and never
reaches the terminal; there is no SFTP or port forwarding on a telnet session.

`serial_ports` lists the serial ports attached now; `serial_connect` opens one
at `baud` and returns a `SessionInfo` with `kind: "serial"`. A serial line is a
plain byte pipe: `session_resize` is accepted but does nothing (there is no
window size), and there is no SFTP or forwarding. `session_close` stops the
reader and releases the port.

```ts
type SerialPortInfo = {
  path: string;            // COM3, /dev/ttyUSB0
  kind: string;            // "USB" | "Bluetooth" | "PCI" | "Unknown"
  product?: string;        // a USB device's product string, when it has one
};
```

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
| `vault_apply_import` | `candidates: ImportCandidate[]`, `username: string \| null`, `hostKeys?: HostKeyCandidate[]` | `ImportResult` |
| `vault_export` | `path: string`, `passphrase: string`, `includeSecrets: boolean` | `void` |
| `vault_import` | `path: string`, `passphrase: string` | `VaultImportSummary` |
| `secret_store_status` | - | `SecretStoreStatus` |
| `secret_store_create` | `master: string` | `void` |
| `secret_store_unlock` | `master: string` | `void` |
| `secret_store_change_master` | `newMaster: string` | `void` |
| `secret_store_lock` | - | `void` |
| `host_connect` | `hostId: string`, `cols: number`, `rows: number` | `SessionInfo` |
| `fleet_run` | `hostIds: string[]`, `command: string` | `FleetResult[]` |
| `key_generate` | `path: string`, `passphrase?: string`, `comment?: string` | `GeneratedKey` |
| `key_deploy` | `hostId: string`, `publicKey: string` | `{ alreadyPresent: boolean }` |

`vault_tree` returns the whole tree in one call: a few hundred hosts at most,
so paging it would cost more than it saves.

A `Host` also carries `guarded`: when set, the frontend confirms a destructive
command (matched against `settings.guardrails`) before it runs on that host -
today at the fleet runner, where a batch mistake is most costly. It is a plain
host field, set through `vault_create_host` / `vault_update_host` like the rest.

**No command returns a secret.** A `Host` says which methods to try and whether
a password is expected (`hasSavedPassword`); the password itself is in the
secret store, keyed by host id, and only ever moves between the store and the
connection that needs it. The one exception is that the store's own commands
*take* a secret inward - a master password the user types, or `vault_import`'s
sealed file - which is the same direction a connect-time password prompt goes.

The secret store is the OS keychain where a machine has one, and an encrypted
file behind a master password where it does not (`SecretStoreStatus.backend` is
`"keychain"` or `"file"`). The file starts locked each session:
`secret_store_create` sets the master password the first time,
`secret_store_unlock` opens it, `secret_store_change_master` re-seals it under a
new one, and `secret_store_lock` forgets it again. The file backend seals with
the same Argon2id + XChaCha20-Poly1305 envelope as the exports. On a keychain
machine the `secret_store_*` mutators return `VAULT_ERROR` - there is no master
password to manage.

```ts
type SecretStoreStatus = {
  backend: "keychain" | "file";
  exists: boolean;   // a keychain always exists; a file may not until set up
  unlocked: boolean; // a keychain is always usable; a file must be unlocked
};
```

`vault_delete_folder` deletes everything underneath, hosts and saved secrets
included. The UI confirms first.

`vault_forget_secrets` removes the host's saved password *and* key passphrase.
Removing something that is not there is success.

`vault_preview_*` write nothing: they read the source and return what they
found for review. `vault_apply_import` is what writes, and it skips any
candidate carrying a `skipReason` or lacking a username with no `username`
fallback to fall back on, rather than importing a guess.

`vault_preview_xshell` takes either an export directory or a `.xts` backup -
the ZIP Xshell's *Tools › Backup* writes, recognised by extension or by the ZIP
signature. Session files are UTF-16LE with a byte order mark and are decoded as
such; a backup is read in place, never extracted to disk, and the private keys
it also contains (`com/SECSH/UserKeys`) are never opened.

A backup also yields `hostKeys`: the host keys Xshell had accepted, each
checked against what Harbour already trusts.

```ts
type HostKeyCandidate = {
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;   // SHA256:..., as the connect-time prompt shows it
  key: string;           // OpenSSH one-line form; a public key is not a secret
  status: "new" | "known" | "changed" | "revoked";
};
```

`new` means nothing is on file for that host and algorithm; `known` means it
is already trusted; `changed` means a *different* key of the same algorithm is
on file. `vault_apply_import` writes **only `new` keys**, whatever it is handed,
and re-checks each against the store first so reviewing the same backup twice
cannot double a line. A changed key is never replaced by an import: the
connect-time prompt, with both fingerprints in front of the user, is the only
way past one. Keys go to Harbour's own `known_hosts`; `~/.ssh` is never
written. `ImportResult.hostKeys` says how many were written.

`vault_export` seals the whole vault - folders, hosts, and, when
`includeSecrets` is set, the keychain's saved passwords and key passphrases -
into one encrypted file at `path`. The sealing is Argon2id then
XChaCha20-Poly1305 (see `src-tauri/src/crypto.rs`); the passphrase is the only
thing not in the file, and an empty one is refused. A secret lives in the
plaintext only for the instant between being read from the keychain and being
sealed, and is never written unencrypted.

`vault_import` opens such a file with `passphrase` and **merges** it in: every
id is reissued as it lands, so nothing already saved is overwritten and
importing the same file twice makes two copies rather than a conflict. Restored
secrets go straight back into the keychain, best-effort - a machine without one
still gets the hosts. A wrong passphrase or an altered file is one
`CRYPTO_ERROR`, and nothing is imported. `VaultImportSummary` reports what was
added:

```ts
type VaultImportSummary = {
  folders: number;
  hosts: number;
  secrets: number; // passwords and passphrases restored to the keychain
};
```

`host_connect` is `ssh_connect` with the vault behind it. The prompts are the
same, except that a saved password is taken from the keychain without asking,
and a `connection:auth_prompt` for a saved host carries `canRemember: true` so
the answer can be saved.

`key_generate` writes an Ed25519 keypair to `path` (and `<path>.pub`),
optionally encrypting the private key with `passphrase`; the private key file is
`0600` on Unix. It returns the public key and its `SHA256:` fingerprint. The
private key never crosses the IPC boundary - only its path and the public half
do. `key_deploy` installs a public key into a saved host's `authorized_keys` by
connecting the way a session does (keychain first, then the password prompt) and
running an idempotent install command over `exec`; `alreadyPresent` says whether
the key was already there. The frontend attaches the private key to the host
(setting `auth.keyPath`) after a successful deploy. `GeneratedKey` is
`{ path, publicPath, publicKey, fingerprint }`.

`fleet_run` runs one command on many saved hosts at once. Each host is a full
connection - its own jump chain, host-key check and keychain credentials - on
which the command is `exec`ed (no pty, no shell) and its stdout, stderr and exit
status collected. It is **non-interactive**: a host whose key is not already
trusted, or whose password is not saved and whose agent cannot get in, comes
back with an `error` rather than raising a prompt - so a run across a whole
estate never blocks on a dialog. At most eight hosts run at once. Results also
stream back as `fleet:result` events, one per host as it finishes, and the whole
set is returned when every host is done.

```ts
type FleetResult = {
  hostId: string;
  name: string;
  exitCode: number | null; // the command's status, or null if it did not run
  stdout: string;
  stderr: string;
  error: string | null;    // set when the host could not be reached or run it
};
```

### Settings

| Command | Arguments | Returns |
| --- | --- | --- |
| `settings_load` | - | `Settings` |
| `settings_save` | `settings: Settings` | `Settings` |
| `settings_reload` | - | `Settings` |
| `settings_paths` | - | `{ settings: string, logs: string }` |
| `theme_import` | `path: string` | `SchemeImport` |
| `highlight_import` | `path: string` | `HighlightImport` |

`settings_save` replaces the **whole document**; there is no partial update,
because a settings dialog that merges field by field ends up with two sources
of truth. What comes back is what was written: the backend clamps and
deduplicates first, so the caller should keep the response rather than what it
sent.

The file is `settings.json` beside the vault, and it is meant to be edited by
hand - `settings_reload` re-reads it. A file that will not parse is moved aside
to `settings.invalid.json` and replaced by defaults rather than deleted, and
never stops Harbour from starting. **It holds no secrets of any kind**, only
preferences: theme, font, keymap, highlight rules, per-host theme overrides and
where logs go.

`theme_import` reads a VS Code theme, a Windows Terminal `settings.json`, an
iTerm2 `.itermcolors` file, an Xshell `.scs` scheme, a directory of any of
those, or the schemes inside an Xshell `.xts` backup, and writes nothing: the
caller reviews the result and saves what it wants with `settings_save`, the same
way the vault importers work. Imported theme ids are prefixed `imported.` so an
imported "Nord" cannot shadow the built-in one. `notes` names the files that
were not colour schemes.

`highlight_import` reads Xshell highlight sets - a `.hls` file, a directory of
them, or the sets inside a `.xts` backup - as `HighlightRule`s with fresh ids,
and writes nothing; the caller adds what it wants to `settings.highlights`.
Two things about the format are handled here so nobody has to know them:
colour indices are resource ids offset by 280, and the palette is `BBGGRR`
(`COLORREF`), the reverse of every other file in the backup. A keyword not
marked `UseRegex` is escaped so it matches itself.

### Files

| Command | Arguments | Returns |
| --- | --- | --- |
| `sftp_home` | `sessionId: string` | `string` |
| `sftp_list` | `sessionId: string`, `path: string` | `DirListing` |
| `sftp_close` | `sessionId: string` | `void` |
| `local_home` | - | `string` |
| `local_roots` | - | `string[]` |
| `local_list` | `path: string` | `DirListing` |

The remote side rides the SSH connection a terminal already has. `sessionId`
is the terminal's session; the first `sftp_*` call for it opens a channel with
the `sftp` subsystem on that connection - **no second host key decision, no
second password** - and later calls reuse it. A local shell's id, or a session
that has closed, gets `SFTP_ERROR`. A server without SFTP is reported with the
server's own refusal.

```ts
type DirListing = {
  path: string;            // made absolute and canonical
  parent: string | null;   // null at a root
  entries: FileEntry[];
};

type FileEntry = {
  name: string;
  kind: "dir" | "file" | "other";  // what a symlink points at, once followed
  symlink: boolean;
  hidden: boolean;                 // dotfile, or Windows hidden attribute
  size: number | null;             // regular files only
  modified: number | null;         // seconds since the epoch
  permissions: number | null;      // Unix mode bits, where they exist
  owner: string | null;
  group: string | null;
};
```

The listing carries its own `parent`, and `path` comes back canonical, so the
frontend never does path arithmetic that depends on knowing whether `..` means
dropping a `/` component or a `\` one. `local_list` on Windows returns plain
`C:\...` paths, never the `\\?\` form `canonicalize` produces. Hidden entries
are included and flagged: showing them is a toggle, not a round trip.

| Command | Arguments | Returns |
| --- | --- | --- |
| `sftp_mkdir` | `sessionId: string`, `path: string` | `void` |
| `sftp_rename` | `sessionId: string`, `from: string`, `to: string` | `void` |
| `sftp_chmod` | `sessionId: string`, `path: string`, `mode: number` | `void` |
| `sftp_remove` | `sessionId: string`, `path: string`, `recursive: boolean` | `void` |
| `local_mkdir` | `path: string` | `void` |
| `local_rename` | `from: string`, `to: string` | `void` |
| `local_remove` | `path: string`, `recursive: boolean` | `void` |

These are the only commands in the files domain that change a file system,
and each is one named operation the user asked for by name. `*_remove` needs
`recursive: true` to delete a directory with anything in it; the UI asks
before sending it. A symlink is removed as a link, never followed.

### Transfers

| Command | Arguments | Returns |
| --- | --- | --- |
| `transfer_enqueue` | `sessionId: string`, `items: TransferRequest[]`, `policy: ConflictPolicy` | `Transfer[]` |
| `transfer_list` | - | `Transfer[]` |
| `transfer_pause` | `id: string` | `void` |
| `transfer_resume` | `id: string` | `void` |
| `transfer_cancel` | `id: string` | `void` |
| `transfer_resolve` | `id: string`, `resolution: Resolution`, `applyToAll: boolean` | `void` |
| `transfer_remove` | `id: string` | `void` |
| `transfer_clear_finished` | - | `number` |

A transfer is one source path to one destination path - a file, or a directory
and everything under it - riding the SFTP channel of `sessionId`. Two run at a
time per session; the rest wait as `queued`. Progress does not come back from
these calls: every change to a transfer is a `transfer:update` event carrying
the **whole transfer**, so the UI is a projection and never reconstructs state
from deltas it might have missed.

```ts
type TransferRequest = {
  direction: "upload" | "download";
  source: string;
  destination: string;   // the full target path, not the directory
};

type ConflictPolicy = "ask" | "overwrite" | "skip" | "resume" | "rename";
type Resolution = "overwrite" | "skip" | "resume" | "rename" | "cancel";

type Transfer = {
  id: string;
  sessionId: string;
  direction: "upload" | "download";
  source: string;
  destination: string;
  state: "queued" | "running" | "paused" | "conflict"
       | "done" | "skipped" | "cancelled" | "failed";
  conflict: ConflictInfo | null;   // set while state is "conflict"
  bytesDone: number;
  bytesTotal: number;              // zero until planned
  filesDone: number;
  filesTotal: number;
  currentFile: string | null;
  error: string | null;
  queuedAt: number;
};

type ConflictInfo = {
  path: string;                    // the destination that already exists
  sourceSize: number;
  sourceModified: number | null;
  destinationSize: number;
  destinationModified: number | null;
  resumable: boolean;              // destination is smaller than source
};
```

A file that already exists at the destination is handled by the transfer's
`policy`. With `ask`, the transfer stops in state `conflict` with the file
described, and waits for `transfer_resolve`; `applyToAll` turns the answer into
the policy for the rest of that transfer. `resume` continues a smaller
destination from where it stops, treats an equal one as already done, and
overwrites a larger one - which cannot be a partial copy of the source.
`rename` writes beside as `name (1).ext`. Time stamps travel with files in both
directions.

`transfer_pause` takes effect at the next chunk - 256 KB - and the state
changes to `paused` once the copy has actually stopped. `transfer_cancel` works
in any state, including waiting on a conflict. A session that closes cancels
everything queued on it. Nothing is persisted: the queue lives as long as the
app does.

### Open in editor

| Command | Arguments | Returns |
| --- | --- | --- |
| `edit_open` | `sessionId: string`, `path: string` | `EditInfo` |
| `edit_list` | - | `EditInfo[]` |
| `edit_close` | `id: string` | `void` |

`edit_open` downloads the remote file to a private directory under the
platform temp directory, opens it with whatever the OS opens that type with,
and watches it: every save is uploaded back, whole, replacing the remote file.
Changes arrive as `edit:update` events. `edit_close` stops watching and removes
the working copy; closing the session does the same for every edit on it.

```ts
type EditInfo = {
  id: string;
  sessionId: string;
  remotePath: string;
  localPath: string;
  uploads: number;
  lastUpload: number | null;
  error: string | null;    // the last upload failed; the local copy is intact
  closed: boolean;
};
```

### Port forwards

| Command | Arguments | Returns |
| --- | --- | --- |
| `forward_open_local` | `sessionId: string`, `spec: ForwardSpec` | `ForwardInfo` |
| `forward_open_dynamic` | `sessionId: string`, `bindAddress: string`, `localPort: number` | `ForwardInfo` |
| `forward_list` | - | `ForwardInfo[]` |
| `forward_close` | `id: string` | `void` |

A local forward is `ssh -L` on the SSH connection `sessionId` already has: it
listens on a local address and, for each connection, opens a `direct-tcpip`
channel to the target and copies bytes both ways. Nothing new authenticates; a
forward reaches only what its session can, and closes with it. The bind happens
inside `forward_open_local`, so a port already in use is an error there, not a
silent failure later. Changes arrive as `forward:update` events carrying the
whole forward.

`forward_open_dynamic` is `ssh -D`: a SOCKS5 proxy on the bound port. Each
connection negotiates SOCKS5 (no auth, CONNECT, IPv4/domain/IPv6) to name its
own target, which is then opened over the same `direct-tcpip` path - so a whole
application reaches whatever the session can. Its `ForwardInfo` has
`kind: "dynamic"` and an empty `host`/`port`, since the target varies per
connection; a local forward has `kind: "local"`.

```ts
type ForwardSpec = {
  bindAddress: string;   // 127.0.0.1 keeps it local; 0.0.0.0 exposes it
  localPort: number;     // 0 asks for a free port, reported back
  host: string;          // resolved on the remote side
  port: number;
};

type ForwardInfo = {
  id: string;
  sessionId: string;
  bindAddress: string;
  localPort: number;     // the port actually bound
  host: string;
  port: number;
  state: "listening" | "closed" | "failed";
  connections: number;   // accepted over the forward's life
  error: string | null;
};
```

## Events

| Event | Payload |
| --- | --- |
| `session:opened` | `SessionInfo` |
| `session:closed` | `{ sessionId, reason: "exit" \| "killed" \| "error", exitCode: number \| null }` |
| `transfer:update` | `Transfer` - the whole transfer, on every change |
| `edit:update` | `EditInfo` - on open, on every upload, on failure, on close |
| `forward:update` | `ForwardInfo` - the whole forward, on every change |

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
| `SETTINGS_ERROR` | The settings file could not be written |
| `SCHEME_IMPORT_FAILED` | That path held no colour scheme Harbour understands |
| `HIGHLIGHT_IMPORT_FAILED` | That path held no Xshell highlight set |
| `SFTP_ERROR` | The session has no remote side, or its SFTP channel could not be opened |
| `FILES_ERROR` | A path could not be listed, made, renamed, removed or copied; the message names it |
| `TRANSFER_ERROR` | A transfer command did not apply: not waiting on a conflict, still running |
| `TRANSFER_NOT_FOUND` | No transfer with that id |
| `EDIT_ERROR` | A file could not be opened for editing, or no such edit |
| `FORWARD_ERROR` | A forward could not bind, or no such forward |
| `LOG_FAILED` | A session log could not be opened |
| `PROMPT_NOT_FOUND` | `connection_respond` for a prompt no longer waiting |
| `PROMPT_TIMED_OUT` | Nobody answered a prompt within five minutes |
| `INTERNAL` | Unclassified; always a bug worth a log line |

`SSH_AUTH_FAILED` carries a message naming what was tried, what the server
never offered, and what it still accepts. "Authentication failed" on its own is
useless when the real problem is `PasswordAuthentication no`.

## Not yet implemented

The spec defines one further domain - `fleet_*`. It is listed here so the naming stays consistent when it lands,
but no handler exists yet.
