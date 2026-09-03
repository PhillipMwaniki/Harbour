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

- Secrets never go in SQLite or the settings store. The host record holds only
  a flag saying whether an entry is expected; the secret itself lives in the OS
  keychain (Windows Credential Manager, macOS Keychain, Secret Service), keyed
  by the host's id and the slot (`password` or `passphrase`).
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
  session to a logging macro.
- **Session logs record output, never input.** A log is attached to the output
  pump, so what reaches the file is what the terminal was sent. Nothing typed
  is written by Harbour: a password reaches the file only if the remote echoed
  it back, which is exactly what a no-echo prompt exists to prevent. This is
  stronger than masking input, and it is a property of where the tap sits
  rather than a rule someone has to remember.
- A session log is nonetheless a plaintext copy of a session, in a directory
  the user chose. It is off by default, started per session, and the pane and
  its tab both show a marker while one is running - a log nobody knows about
  would be worse than no log at all.

## Host keys

- Trust on first use, with an explicit prompt showing the SHA-256 fingerprint.
- A **changed** host key is a hard warning that defaults to reject, showing both
  the stored and the offered fingerprint. There is no "always accept" option.
- Known hosts are read from the user's `~/.ssh/known_hosts` and written to an
  app-managed file in OpenSSH format.
- Host keys imported from an Xshell backup are reviewed first, and only keys
  with nothing on file are written. A key that differs from one already
  trusted is shown and refused: the connect-time prompt, with both
  fingerprints in front of the user, is the only way past a changed key, and
  an import must not be a quieter one.

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
will default to when key generation lands. The ignore carries this reasoning
inline, and any future entry must do the same.

## Current state

Milestone 4 adds session logging and a settings file. The master-password and
export rules above still describe milestone 8.

Implemented:

- The vault. SQLite holds folders and hosts and nothing else; every secret is
  in the OS keychain, addressed by host id. Deleting a host or a folder takes
  the matching keychain entries with it, so nothing is orphaned. There is no
  plaintext fallback: with no keychain the user is asked every time, which is
  an inconvenience, where writing a password to a file they did not ask for
  would be a betrayal.
- Host key trust, in full. `~/.ssh/known_hosts` and `known_hosts2` are read -
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
- Imports that write nothing until reviewed. `~/.ssh/config` and Xshell exports
  are read into a list the user confirms; Xshell passwords are still not
  decoded, only noted as having existed. Colour scheme imports work the same
  way, and read colours out of the file and nothing else.
- Reading an Xshell `.xts` backup in place. It is a ZIP of the whole profile,
  and alongside sessions and host keys it holds the user's private keys and a
  credential file. Harbour reads four directories out of it by name and has no
  code path that opens the others; a test asserts that nothing the reader
  returns contains what is under `UserKeys`. Entry names are checked against
  path traversal before use, as any untrusted archive's should be.
- SFTP that rides the terminal's connection. The file pane opens a second
  channel on a session that has already verified its host key and
  authenticated; it never holds a credential, never makes a trust decision,
  and cannot reach a host the user has not already connected to. In this
  milestone it is read-only: nothing it can be asked to do changes either
  file system.
- A settings file that holds no secrets of any kind - theme, font, keymap,
  highlight rules, per-host themes, where logs go - and that is treated as
  untrusted input on the way in: a malformed one is moved aside and replaced
  with defaults rather than crashing the app, and a highlight rule whose
  regular expression will not compile is reported beside the rule rather than
  thrown out of a render.

Not yet:

- The master password and encrypted export, both milestone 8. Today the vault
  file is plain SQLite - it holds no secrets, but it does describe the estate,
  so it is worth the same care as `~/.ssh/config`.
- Agent forwarding is not implemented at all, which is the safe default.
- The clipboard rules above are not implemented: a multi-line paste is sent
  without confirmation. That is milestone 7, with snippets.
