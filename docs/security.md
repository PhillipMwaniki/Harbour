# Security model

Harbour handles credentials for machines people care about. These rules are
binding on every change, and most of them predate the code that will need them.

## Boundaries

- The webview is untrusted rendering surface. Privileged work - processes,
  sockets, files, keychain - happens only in the Rust core.
- Tauri capabilities are an allowlist (`src-tauri/capabilities/default.json`).
  Adding a permission needs a justification in the PR that adds it.
- The CSP forbids remote content: no remote scripts, no remote styles, no
  frames, no form actions. Links in terminal output are handed to the OS
  browser via the opener plugin, never navigated to in-app.

## Secrets

- Secrets never go in SQLite or the settings store. The host record holds a
  `credential_ref`; the secret itself lives in the OS keychain (Windows
  Credential Manager, macOS Keychain, Secret Service).
- If the keyring is unavailable, prompt the user. Never fall back to plaintext
  storage.
- Optional master password wraps a random vault key held in the keyring; with
  it enabled, secrets are additionally encrypted at rest with
  XChaCha20-Poly1305 rather than trusting the OS keychain alone.
- Vault export uses Argon2id (m=64 MB, t=3) to derive a key, then
  XChaCha20-Poly1305. The format carries a version byte.

## Logging

- `tracing` output goes to stderr and a rotating file in the app log directory.
- Never pass a password, passphrase, private key, or the contents of a terminal
  session to a logging macro. When a session log is enabled, input is masked
  while the pty is in no-echo mode.

## Host keys

- Trust on first use, with an explicit prompt showing the SHA-256 fingerprint.
- A **changed** host key is a hard warning that defaults to reject, showing both
  the stored and the offered fingerprint. There is no "always accept" option.
- Known hosts are read from the user's `~/.ssh/known_hosts` and written to an
  app-managed file in OpenSSH format.

## Agent forwarding

Off by default. Per-host opt-in, with a warning that explains what a
compromised remote host can do with a forwarded agent.

## Clipboard

Multi-line pastes show a confirmation with the exact text that will be sent.
Bracketed paste is used where the remote supports it.

## Supply chain

`cargo audit` and `pnpm audit` run in CI. Dependencies are pinned in lockfiles,
which are committed.

## Current state

Milestone 1 has no credential handling at all - local shells only. Nothing on
this page is implemented yet beyond the capability allowlist, the CSP and the
logging discipline. The rules exist first so the code that lands in milestones
2 and 3 has something to conform to.
