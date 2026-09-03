# Changelog

All notable changes to Harbour are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Linux beyond Ubuntu. The `.rpm` now names its dependencies, CI installs the
  `.rpm` on Fedora and the `.deb` on Debian stable after every build, and two
  Arch packages - `harbour-bin` from the release's `.deb` and `harbour` from
  source - are built from the checkout in CI and published to the AUR when a
  release is published.
- A release pipeline. Pushing a `vX.Y.Z` tag builds installers for Windows,
  macOS (Apple silicon and Intel) and Linux, attaches them and a `SHA256SUMS`
  file to a draft GitHub release with the changelog entry as its body, and
  leaves publishing to a person. It refuses a tag whose version disagrees with
  `package.json`, `tauri.conf.json` or `Cargo.toml`, or that has no changelog
  section, before building anything.
- **Milestone 6: the transfer engine.** Drag rows between the remote and local
  panes, or onto a directory in the other pane, to copy them - a file, or a
  directory and everything under it - and drag files in from the desktop to
  upload them. A queue at the foot of the file dock shows progress; two
  transfers run at a time per host; pause takes hold at the next 256 KB chunk
  and cancel works at any point. Time stamps travel with the files.
- Conflicts stop to ask. A file already at the destination shows both sides'
  size and date and offers overwrite, skip, keep both (`name (1).ext`) or -
  when the existing copy is smaller than the source - resume from where it
  stops, with one answer able to cover the rest of the transfer. Resuming onto
  anything but a smaller copy is never offered, since it would corrupt the
  file.
- Open in editor: a remote file downloaded to a private temporary directory,
  opened with the OS default for its type, and uploaded back - whole,
  truncating - on every save, whether the editor writes in place or by
  renaming over the file. Closing the entry or the session removes the copy.
- New folder, rename and delete on both panes, from the context menu and the
  keyboard. Delete asks first, and a directory is only removed recursively when
  the user has said so.
- **Milestone 5: SFTP on the shared connection.** File panes (Ctrl+Shift+S)
  docked beside the terminals: the remote machine over SFTP on top, the local
  machine below. The remote side is a second channel on the SSH connection the
  focused terminal already has - no second host key prompt, no second
  password - and follows focus, each session keeping its place. A local shell
  shows the pane empty rather than the last host's files.
- Navigation in both panes: double-click, a typed path, up, home, refresh,
  sort by name, size or date, and a hidden-files toggle that costs no round
  trip. Listings come back with their path canonical and their parent named,
  so `..` and symlinked directories resolve on the machine that owns them and
  the frontend never guesses at path separators. On Windows the local pane
  offers the drives when it reaches a root.
- Looking only: nothing in this milestone writes to either file system.
  Transfers are milestone 6.
- Importing straight from an Xshell `.xts` backup - the file *Tools › Backup*
  writes - as well as from an export directory. The archive is read in place;
  the private keys and credential file it also holds are never opened.
- Host keys from a backup, reviewed alongside the sessions. New ones are
  written to Harbour's own `known_hosts`, saving a trust-on-first-use prompt
  per host; a key that differs from one already trusted is shown and refused,
  since an import must not be a quieter way past the connect-time prompt.
- Xshell colour schemes (`.scs`), as a file, a directory, or straight out of a
  backup, alongside the VS Code, Windows Terminal and iTerm formats.
- Xshell highlight sets (`.hls`) as highlight rules, from a file, a directory
  or a backup. Colour indices are resource ids offset by 280 and the palette is
  `BBGGRR`; both are handled so nobody has to know.
- **Milestone 4: terminal polish.** A tab now holds a tree of split panes -
  Ctrl+Shift+D splits right, Ctrl+Shift+B splits down, the divider drags, and
  a split repeats whatever the focused pane was showing, so splitting a host
  gives a second shell on that host. Closing the last pane closes the tab.
- Find in the scrollback (Ctrl+Shift+F), with case, whole-word and regular
  expression toggles, a match count, and search-as-you-type.
- A user-editable keymap. Every action is listed in Settings with its chords;
  chords can be recorded by pressing them, reset to the built-in binding, or
  removed entirely - an action with no chords is unbound, which is how a key is
  handed back to the terminal. xterm is told which chords Harbour has claimed,
  so Ctrl+Shift+[ moves the focus instead of also sending an escape.
- Highlight rules: regular expressions with a foreground and a background,
  drawn over the output as xterm decorations. A rule whose pattern will not
  compile is reported beside the rule rather than thrown out of a render, and
  the rule listed first wins any text two rules both match.
- Session logging (Ctrl+Shift+L), to `plain` text or to `raw` bytes, with an
  option to start one for every session. The tap sits on the output pump, so a
  log records what the terminal was *sent* and never what was typed; writes go
  to a dedicated thread so a slow disk cannot throttle the session. The pane
  and its tab both show a marker while a log is running.
- Importing colour schemes from VS Code themes, Windows Terminal
  `settings.json` files and iTerm2 `.itermcolors` files - one file or a whole
  directory. Nothing is saved until reviewed, and files that were not schemes
  are listed with the reason. Each scheme only describes a terminal palette, so
  the chrome colours are derived by mixing its own background and foreground.
- Per-host theme overrides, from the host editor: production that does not look
  like staging.
- `settings.json` beside the vault, holding theme, font, scrollback, keymap,
  highlight rules, per-host themes and logging preferences - and no secrets of
  any kind. It is meant to be hand-edited: writes are atomic, and a file that
  will not parse is moved aside to `settings.invalid.json` and replaced with
  defaults rather than stopping the app from starting.
- Font size and scrollback are settings, with Ctrl+= / Ctrl+- / Ctrl+0 to
  change the first without opening a dialog.

- **Milestone 3: the vault.** A SQLite store of saved hosts in a folder tree,
  shown in a session-manager sidebar (Ctrl+Shift+E). Double-click a host, or
  select it and press Enter, to connect.
- Secrets in the OS keychain - Windows Credential Manager, macOS Keychain,
  Secret Service on Linux - addressed by host id and slot. The database holds
  no secret of any kind, only a flag saying whether one is expected. Deleting a
  host or a folder removes the matching keychain entries, and the host editor
  can forget a saved password without deleting the host.
- A "save it in the system keychain" option on the password prompt, offered
  only for a saved host on a machine with a working keychain. There is no
  plaintext fallback: with no keychain, Harbour asks every time.
- `~/.ssh/config` import that follows `Include` (including a wildcard in the
  file name) and applies `ssh`'s own precedence: first value wins, and wildcard
  `Host` blocks contribute defaults rather than becoming hosts.
- Xshell `.xsh` import wired to the store, mirroring the export's folder tree.
  Sessions that are not SSH are listed with the reason rather than dropped.
- Both imports write nothing until reviewed, and skip entries whose source
  named no username unless a fallback is given - importing under a guess would
  produce hosts that fail to connect for an invisible reason.
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

- `session_write` sends input as a raw request body with the session id in a
  header, rather than a JSON number array. A large paste was costing four times
  the bytes and a JSON parse; a JSON body is still accepted for the fallback
  IPC path.
- The theme is no longer kept in `localStorage`: it lives in `settings.json`
  with the rest of the preferences, so there is one source of truth for them.
- A vault that will not open no longer stops the app: it falls back to an
  in-memory store and logs, so local shells and ad-hoc SSH still work.
- `russh` uses the `ring` crypto backend rather than the default `aws-lc-rs`,
  which needs NASM on Windows. Building Harbour still requires nothing but
  Rust, Node and pnpm.
- `cargo audit` ignores RUSTSEC-2023-0071 (the Marvin attack against the `rsa`
  crate, which has no patched version) so that RSA host keys and user keys keep
  working. The reasoning is in `src-tauri/.cargo/audit.toml` and
  `docs/security.md`; it is the only ignored advisory.

### Fixed

- Connecting to a saved host failed outright with "no usable ssh agent" on a
  machine whose agent was absent, had hung up, or held no keys. The agent step
  is tried first for every host, and an error from it ended the attempt
  instead of moving on to the next method as `ssh` does. It is now a failed
  method like any other: the password prompt follows, and if nothing works the
  message still names what the agent said.
- The Xshell import found nothing in a real export. Xshell writes its files as
  UTF-16LE with a byte order mark; they were read as UTF-8, so every section
  header came out as `[\0C\0O\0N…` and every file was skipped with "no
  [CONNECTION] section". The tests used hand-written UTF-8 fixtures, which
  Xshell never produces. Files are now decoded by their byte order mark, and
  the regression test is built from the bytes Xshell writes.
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
