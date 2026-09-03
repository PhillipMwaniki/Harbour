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

One advisory is ignored, in `src-tauri/.cargo/audit.toml`, and it must stay the
only one: **RUSTSEC-2023-0071**, the Marvin attack against the `rsa` crate. RSA
operations there are not constant-time, and no patched version exists. It is
accepted rather than fixed because the alternative is dropping RSA, which means
refusing hosts that present an `ssh-rsa` host key or expect an RSA user key -
still a large share of the machines this tool exists to reach. In Harbour an
RSA private key signs one authentication challenge per connection, to a server
the user chose; Ed25519 and ECDSA are unaffected and are what key generation
will default to in milestone 3. The ignore carries this reasoning inline, and
any future entry must do the same.

## Current state

Milestone 2 implements the host key rules and the credential handling that goes
with connecting; the vault rules still describe milestone 3.

Implemented:

- Host keys, in full. `~/.ssh/known_hosts` and `known_hosts2` are read -
  including hashed entries, wildcards, negations and `@revoked` - and Harbour
  appends only to its own file under the app config directory. An unknown key
  prompts with the SHA-256 fingerprint and offers to remember it; a changed key
  leads with the warning, shows both fingerprints and their source line, and
  focuses Reject. There is no "always accept".
- Credentials. A password or passphrase is typed into a dialog, sent once to
  the attempt that asked for it, and dropped. It is never stored, never logged,
  and never sent back to the webview. Cancelling is distinct from an empty
  answer: it stops the attempt rather than spending a try.
- `@cert-authority` lines are parsed and ignored rather than mistaken for host
  keys, and a server offering a host *certificate* is refused outright rather
  than trusted unverified.

Not yet:

- The vault, the keyring, the master password and encrypted export - all
  milestone 3. Nothing is persisted between connections today except a host key
  the user chose to remember.
- Agent forwarding is not implemented at all, which is the safe default.
- Session logging does not exist, so the no-echo masking rule has nothing to
  apply to yet.
