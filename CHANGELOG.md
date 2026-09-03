# Changelog

All notable changes to Harbour are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

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
