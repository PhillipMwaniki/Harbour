# Changelog

All notable changes to Harbour are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- **Milestone 2: SSH core.** Remote shells over `russh`, opened from a connect
  dialog (Ctrl+Shift+N) and driven through the same tabs, output pump and
  backpressure budget as local sessions.
- Authentication by SSH agent, private key file, password and
  keyboard-interactive, tried in the order given and skipping anything the
  server does not offer. A failure names what was tried, what the server never
  offered and what it still accepts, rather than just "authentication failed".
- Agent discovery per platform: `SSH_AUTH_SOCK` on Unix; `SSH_AUTH_SOCK`, the
  OpenSSH named pipe, then Pageant on Windows.
- Host key verification against the user's `~/.ssh/known_hosts` and
  `known_hosts2`, covering hashed entries, wildcard and negated patterns,
  non-default ports and `@revoked`; `@cert-authority` lines are parsed and
  ignored rather than mistaken for host keys. Keys the user chooses to remember
  are appended to Harbour's own file, never to the user's.
- Host key prompts showing the full SHA-256 fingerprint. An unknown host offers
  to remember it; a changed key shows both fingerprints with their source line,
  defaults to reject, and has no "always accept".
- Credential prompts raised by the core mid-connection, answered over a
  `connection_respond` round trip. Secrets go straight to the attempt that
  asked for them: never stored, never logged, never sent to the webview.
  Cancelling stops the attempt instead of spending an authentication try.
- A `Transport` abstraction over the session layer, so a pty and an SSH channel
  differ only in write, resize and kill; everything above them is shared.
- SSH keepalives every 30 seconds, so a dropped link is noticed instead of
  waiting for the user to type into a dead session. A session that dies
  unexpectedly now keeps its tab, carrying the reason, rather than vanishing.
- End-to-end tests that run a real `russh` server in-process and connect to it,
  covering the handshake, host key verification, authentication, the pty and
  shell requests, resizes, and output in both directions.
- **Milestone 1: scaffold.** Tauri 2 shell with a React + TypeScript frontend,
  Tailwind, Zustand and xterm.js, building on Windows, macOS and Linux.
- Local terminal sessions over `portable-pty` (ConPTY on Windows, forkpty
  elsewhere), with tabs, resize, title from OSC sequences, and clean teardown of
  child processes on window close.
- Shell detection: PowerShell 7, Windows PowerShell, cmd, Git Bash and installed
  WSL distributions on Windows; `$SHELL` plus the usual candidates elsewhere.
- Output pipeline with 32 KB / 8 ms batching and an ack-based 1 MB backpressure
  budget, so heavy output cannot lock up the webview.
- Theme system covering the terminal palette and the app chrome, with eleven
  built-in schemes (Harbour Dark, Dark+, Light+, Monokai, Dracula, Nord, One
  Dark, Solarized Dark/Light, Gruvbox Dark, Tokyo Night), a picker in the tab
  bar, and the choice remembered between runs.
- Xshell `.xsh` import parser: walks an export directory into hosts and folders,
  reading host, port, protocol, username, description, key name and encoding.
  Stored passwords are recorded as present but never decoded.
- Stable IPC error codes and the contract documented in `docs/ipc.md`.
- CI building and testing on `windows-latest`, `macos-latest` and
  `ubuntu-22.04`.

### Changed

- `russh` uses the `ring` crypto backend rather than the default `aws-lc-rs`,
  which needs NASM on Windows. Building Harbour still requires nothing but
  Rust, Node and pnpm.
- `cargo audit` ignores RUSTSEC-2023-0071 (the Marvin attack against the `rsa`
  crate, which has no patched version) so that RSA host keys and user keys keep
  working. The reasoning is in `src-tauri/.cargo/audit.toml` and
  `docs/security.md`; it is the only ignored advisory.

### Fixed

- The first prompt no longer renders blank. The pty was opened at a guessed
  80x24 and resized once xterm had measured itself; ConPTY repaints on resize
  and PSReadLine only redraws on input, so the shell's opening prompt was lost
  until the first keystroke. The terminal now measures itself first and opens
  the session at its real size.
- Closing a session no longer stalls the caller. Releasing a pty master calls
  `ClosePseudoConsole` on Windows, which blocks until the console's output
  buffer has been drained; the reader thread stopped reading as soon as its
  channel closed, so teardown could block for minutes. The reader now drains to
  EOF, and teardown runs on its own thread.

[Unreleased]: https://github.com/harbour-app/harbour/commits/main
