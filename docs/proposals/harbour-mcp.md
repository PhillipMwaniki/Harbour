# Harbour MCP server — design

Expose Harbour's saved infrastructure to AI agents through a Model Context
Protocol (MCP) server, so an agent can list saved hosts, run commands on them,
move files over SFTP, and open port forwards — using the same vault, keychain
secrets, and `known_hosts` the desktop app uses, and the same non-interactive
safety rules as the fleet runner.

Decisions taken with the user:

- **Architecture:** a standalone server binary that reuses Harbour's core crate.
  It runs headless — no GUI, works whether or not the desktop app is open.
- **Capabilities (v1 target):** full — inventory, command execution (single and
  fan-out), SFTP read/write/list, and port forwards.

## Why a core crate first

`harbour_lib` (the `src-tauri` crate) is almost Tauri-free already. Outside the
`commands/` layer the *only* Tauri dependency is `tauri::async_runtime` — a thin
re-export of tokio — used in six files (`edit.rs`, `ssh/client.rs`,
`ssh/forward.rs`, `ssh/transport.rs`, `transfer/copy.rs`, `transfer/engine.rs`).
Everything Tauri-specific (`#[tauri::command]`, `State`, `AppHandle`, the app
entry point) is confined to `commands/`, `lib.rs`, and `main.rs`.

So we do **not** move modules into a new crate. Instead:

1. Replace the six `tauri::async_runtime::{spawn, spawn_blocking, JoinHandle}`
   uses with the identical `tokio::task::…` (tokio is already a dependency).
2. Make `tauri` and the `tauri-plugin-*` crates **optional**, behind a new
   default feature `app`.
3. Feature-gate the Tauri surface — `commands/`, the app entry in `lib.rs`,
   `main.rs`, and `prompt::EventAsker` — with `#[cfg(feature = "app")]`.

The desktop app keeps building exactly as today (the `app` feature is on by
default). The MCP crate depends on `harbour_lib` with `default-features = false`,
so it links only the Tauri-free core: `vault`, `ssh`, `session` traits,
`settings`, `crypto`, `files`, `error`.

### Workspace

`src-tauri` is currently a standalone package. Introduce a workspace at the repo
root with members `["src-tauri", "mcp"]`. Tauri supports living in a workspace;
CI's `cd src-tauri && cargo …` commands keep working. The lock file moves to the
root. A new CI matrix entry builds and tests the `mcp` crate.

## Finding the shared state

The MCP must read the *same* files the app does. The app uses Tauri's
`app_config_dir()`, which for identifier `com.harbour.app` resolves to
`dirs::config_dir()/com.harbour.app` on every platform we target. The MCP
computes the same path with the `dirs` crate:

- vault: `<config>/vault.sqlite3` → `Vault::open`
- known_hosts: `<config>/known_hosts` → `KnownHosts::new`
- secrets: `<config>/secrets.vault` → `SecretStore::detect` (keychain-preferring)

Portable mode (a `harbour.portable` marker beside a shared install) is honoured
by reusing the existing `portable::base_dir` logic, so a portable install and
its MCP agree on where everything lives.

## The server

- **Transport:** stdio, newline-delimited JSON-RPC 2.0 — what MCP clients
  (Claude Desktop/Code and others) launch and speak. No network listener.
- **Protocol:** hand-rolled minimal MCP (`initialize`, `tools/list`,
  `tools/call`, and the `notifications/initialized` no-op). Hand-rolled rather
  than pulling a large SDK keeps the dependency set as tight as the rest of the
  project (the same reason the app uses `ring` over `aws-lc`). The dispatch and
  every tool's JSON schema are unit-tested.
- **Async:** tokio (multi-thread), reusing `client::run_command`, the SFTP
  module, and the forward engine directly.

## Tools

Inventory (read-only):

- `harbour_list_hosts` → `[{ id, name, hostname, port, username, folder,
  jumpHost, guarded, hasSavedPassword }]`
- `harbour_list_folders` → the folder tree

Execution:

- `harbour_run_command` `{ host, command }` → `{ exitCode, stdout, stderr }`.
  `host` accepts an id or a unique name. Non-interactive: keychain secrets only,
  unknown host keys refused — exactly `client::run_command` behind the
  `FleetAsker`.
- `harbour_run_fleet` `{ hosts[], command }` → one result per host, bounded
  concurrency, mirroring the fleet runner.

SFTP:

- `harbour_sftp_list` `{ host, path }` → directory entries
- `harbour_sftp_read` `{ host, path, maxBytes? }` → file contents (text, or
  base64 with a flag when not valid UTF-8; capped)
- `harbour_sftp_write` `{ host, path, content, base64? }` → bytes written

Port forwards (stateful — the server holds the connection for the forward's
life):

- `harbour_forward_open` `{ host, kind: local|remote|dynamic, … }` → the bound
  forward
- `harbour_forward_list` → active forwards
- `harbour_forward_close` `{ id }`

## Security

- The server acts with the user's saved credentials, so it is exactly as
  privileged as the person who ran it. It performs **no** interactive trust
  decisions: an unknown host key fails the call (a person must trust a new key
  once, in the app), and a password that is not in the keychain fails rather
  than prompting.
- Write/execute tools (`run_*`, `sftp_write`, `forward_*`) are gated by a
  `--allow-write` flag (and `HARBOUR_MCP_ALLOW_WRITE=1`); without it the server
  advertises only the read-only inventory and SFTP-read tools. This makes
  "let an agent look at my estate" safe by default and "let it act" a
  deliberate opt-in.
- Secrets never appear in any tool result or log. Command output is returned
  verbatim (it is the point), but the server never echoes a password, key, or
  passphrase.
- Guardrails: a host marked `guarded` in the vault has its `run_command`
  checked against the configured guardrail patterns; a match is refused with a
  message rather than run, so the same rails that protect the terminal protect
  the agent.

## Phasing (one PR each, each green)

- **P0 — enable.** Workspace, feature-gating, `async_runtime`→`tokio` swap, the
  `mcp` crate skeleton speaking MCP over stdio with a single `harbour_list_hosts`
  tool reading the real vault. Proves the whole pipeline end to end.
- **P1 — execute.** `harbour_run_command` + `harbour_run_fleet`, the
  non-interactive asker, name→id resolution, guardrail checks, the
  `--allow-write` gate. Integration-tested against the in-repo test SSH server.
- **P2 — files.** The three `harbour_sftp_*` tools over a one-shot connection.
- **P3 — forwards.** The stateful forward tools, with the server owning the
  connections for their lifetime.

Docs: a `docs/mcp.md` contract (tools, schemas, config snippet for pointing an
MCP client at the binary) lands with P1 and grows per phase.

## Effort

Medium-large. P0 is the riskiest (structural) but small in code; P1 is the
substance; P2/P3 are additive. No change to the app's behaviour at any point —
the `app` feature keeps it identical.
