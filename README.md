# Harbour

A cross-platform SSH client that puts the terminal, the session manager and an
SFTP file manager in one window, sharing one connection per host. Local-first:
no account, no cloud, credentials never leave the machine unencrypted.

Harbour is an Xshell + Xftp replacement for teams that live on Windows but ship
to Linux, and it runs the same on macOS and Linux.

> **Status: milestone 6 of 9.** Files move: drag between the remote and local
> panes or in from the desktop, with a queue that pauses, resumes, asks before
> overwriting and can pick up a partial copy where it stopped; open a remote
> file in your editor and every save goes back. Underneath, SFTP on the
> connection the terminal already has, a finished terminal with splits, find,
> keymap, highlight rules and logging, SSH end to end with agent, key and
> password auth, host keys checked against your existing `known_hosts`, saved
> hosts with passwords in the OS keychain, and imports from `~/.ssh/config`
> and Xshell backups. Port forwarding and snippets are next. See
> [the roadmap](#roadmap).

## Requirements

| | Version |
| --- | --- |
| Rust | 1.82+ (stable) |
| Node | 20+ |
| pnpm | 9+ |
| Windows | 10 1809 or later (ConPTY) |
| Linux | `libwebkit2gtk-4.1-dev`, `libxdo-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `build-essential` |
| macOS | Xcode command line tools |

## Getting started

```sh
pnpm install
pnpm tauri:dev      # dev build with hot reload
```

Other useful commands:

```sh
pnpm test                     # frontend unit tests (vitest)
pnpm typecheck                # tsc --noEmit
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri:build              # installers for the current platform
```

Set `HARBOUR_LOG=harbour_lib=debug` for verbose backend logging. Logs are
written to the platform app-log directory as well as stderr.

Installers for each tagged version are on the
[Releases](https://github.com/PhillipMwaniki/Harbour/releases) page, with a
`SHA256SUMS` file beside them. They are not code-signed yet: Windows shows the
"unknown publisher" warning, and macOS needs a right-click → Open the first
time. How a release is cut is in [`docs/releasing.md`](docs/releasing.md).

## Connecting over SSH

**Ctrl+Shift+N**, or *SSH connection…* in the tab-bar menu. Give a host, a
username and a port; Harbour tries the SSH agent first, then a key file if you
named one, then password and keyboard-interactive. Nothing that needs a secret
asks for one up front - the prompt appears only if that method is actually
reached, and the answer goes straight to the connection and nowhere else.

The agent is found where your platform keeps it: `SSH_AUTH_SOCK` on Unix, and
on Windows `SSH_AUTH_SOCK`, then the OpenSSH named pipe, then Pageant.

Host keys are checked against your existing `~/.ssh/known_hosts` - hashed
entries, wildcards, negations and `@revoked` lines included - so a host you
already trust from a terminal connects without a prompt. An unknown host shows
its SHA-256 fingerprint and offers to remember it; a changed key shows both
fingerprints and defaults to rejecting. Harbour never writes to your OpenSSH
files: keys it learns go to its own `known_hosts` in the app config directory.

## Saved sessions

The sidebar (**Ctrl+Shift+E**) is the session manager: a folder tree of saved
hosts. Double-click one, or select it and press Enter, to connect. Each host
records where to connect, as whom, and which authentication methods to try.

Passwords are not typed into the host form. They are asked for by the
connection, at the moment one is needed, with the option to save it - and a
saved password goes to the OS keychain (Windows Credential Manager, macOS
Keychain, Secret Service on Linux), never to the vault file. The vault itself
is a SQLite database beside Harbour's config holding folders, hosts and no
secrets whatsoever. Deleting a host takes its keychain entries with it, and
*Forget it* in the host editor removes them without deleting the host.

If your machine has no usable keychain, Harbour asks every time rather than
falling back to writing the password somewhere.

## Importing an existing estate

Both importers show you everything they found and write nothing until you
press Import.

**OpenSSH** reads `~/.ssh/config`, following `Include` directives, and applies
`ssh`'s own rules - first value wins, and wildcard `Host` blocks contribute
defaults rather than becoming hosts of their own.

**Xshell** reads a `.xts` backup - the file *Tools › Backup* writes, which is
what most people actually have - or a directory of exported `.xsh` files,
mirroring the folder tree either way. Sessions that are not SSH are listed
with the reason they cannot come across rather than being dropped silently. A
backup also offers the host keys Xshell had accepted, so a migrated estate does
not greet you with a trust-on-first-use prompt per host. Entries whose source never named a username are
skipped unless you supply one to fall back on - importing under a guess would
produce hosts that fail to connect for a reason you cannot see.

## Splits, find and logging

A tab holds as many terminals as you want. **Ctrl+Shift+D** splits right and
**Ctrl+Shift+B** splits down, repeating whatever the focused pane was showing -
split a host and you get a second shell on the same host. Drag the divider to
resize; **Ctrl+Shift+W** closes a pane, and the tab goes with its last one.

**Ctrl+Shift+F** searches the scrollback, with case, whole-word and regular
expression toggles and a count of what it found.

**Ctrl+Shift+L** starts writing the session to a file, and the pane and its tab
both show a marker while it does. Logs record *output*, never input, because
the tap sits on the output pump: nothing you type is written, so a password is
in the file only if the remote echoed it. The `plain` format strips escape
sequences and resolves carriage returns, so the file reads the way the screen
did; `raw` keeps every byte. Settings can start a log for every session.

**Highlight rules** colour text in output you do not control - `ERROR` from a
log that never learned about ANSI, a hostname you must not confuse with
another. They are regular expressions with a foreground and a background, and
the rule listed first wins any text two of them match. Xshell highlight sets
(`.hls`, or the ones inside a `.xts` backup) import from the same page.

## Files

**Ctrl+Shift+S** opens the file panes beside the terminals: the remote machine
on top, your own below. The remote pane follows the focused terminal and rides
its connection - SFTP is a second channel on the SSH session you already have,
so there is no second host key prompt and no second password, and switching to
another host's terminal shows that host's files, each keeping its place. A
local shell shows the pane empty rather than the last host's listing.

Both panes navigate the same way: double-click a directory, type a path into
the bar, go up or home, sort by name, size or date, and show hidden files with
one toggle. Right-click for new folder, rename and delete; delete asks first.

**Copying is dragging.** Drag rows from one pane onto the other - or onto a
directory in it - and they are queued: a file, or a directory and everything
under it. Files dragged in from the desktop upload to the remote pane. The
queue at the foot of the dock shows each transfer's progress; pause and resume
land within a fraction of a second, cancel works at any point, and two
transfers run at a time per host so a large file does not starve a small one.

When a file already exists at the destination the transfer **stops to ask**,
showing both sides' size and date: overwrite, skip, keep both, or - when the
existing copy is smaller - resume from where it stops, with one answer able to
cover the rest of the transfer. Time stamps travel with the files.

**Open in editor** downloads a remote file to a private temporary directory,
opens it with whatever your machine opens that kind of file with, and uploads
it back on every save. Close the entry in the queue - or the session - and the
working copy is removed.

## Themes

Eleven built-in schemes - Harbour Dark, Dark+, Light+, Monokai, Dracula, Nord,
One Dark, Solarized Dark/Light, Gruvbox Dark and Tokyo Night - switchable from
the **Theme** button in the tab bar. A theme covers the whole window, not just
the terminal: chrome colours are published as CSS custom properties, so the tab
bar, menus and dialogs move with it.

**Import** a VS Code theme, a Windows Terminal `settings.json`, an iTerm2
`.itermcolors` file, an Xshell `.scs` scheme, a whole directory of them, or
every scheme inside an Xshell `.xts` backup, from *Settings*, under
*Appearance*. Every one of those describes a terminal palette and nothing else,
so Harbour derives the chrome colours by mixing the scheme's own background and
foreground - a warm scheme gets warm borders.

A saved host can **override the theme**, from the Theme field in the host
editor. Production that does not look like staging is the cheapest safety
measure a terminal has.

## Keyboard

Every binding below can be changed under *Keyboard* in Settings, or by editing
`settings.json` by hand. An action with no chords is unbound, which is how you
hand a key back to the terminal.

| Shortcut | Action |
| --- | --- |
| `Ctrl+Shift+T` | New terminal (default shell) |
| `Ctrl+Shift+N` | New SSH connection |
| `Ctrl+Shift+K` | Clear the terminal |
| `Ctrl+Shift+D` / `Ctrl+Shift+B` | Split right / down |
| `Ctrl+Shift+W` | Close the pane |
| `Ctrl+Shift+[` / `Ctrl+Shift+]` | Previous / next pane |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+Shift+E` | Show or hide the session manager |
| `Ctrl+Shift+S` | Show or hide the file panes |
| `Ctrl+Shift+F` | Find in the terminal |
| `Ctrl+Shift+L` | Start or stop logging this session |
| `Ctrl+,` | Settings |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Larger / smaller / default text |

## Settings

`settings.json`, beside the vault in the app config directory. It holds the
theme, the font, the keymap, the highlight rules, the per-host theme overrides
and where logs go - and **no secrets of any kind**, so it is safe to copy
between machines or paste into an issue. It is meant to be hand-edited: a file
that will not parse is moved aside to `settings.invalid.json` and replaced with
defaults rather than stopping the app from starting.

## Layout

```
src/              React frontend
  app/            layout shell
  components/     terminal and panes, file panes, ssh dialogs, settings, session tree
  ipc/            typed wrappers around invoke/listen - the only place
                  that knows command names
  stores/         zustand stores, one per domain
  lib/            panes, keymap, highlight rules, themes, path helpers
src-tauri/src/    Rust core
  commands/       thin IPC handlers
  session/        session manager, pty, output pump, logging, shell detection
  settings/       settings.json, and the colour schemes imported into it
  files/          directory listings, local and remote, in one shape
  transfer/       the queue, and the bytes it moves
  edit.rs         a remote file in a local editor, uploaded on save
  ssh/            connect and auth, channel transport, sftp, known_hosts, agent
  vault/          sqlite host store, os keychain, ssh_config and xshell imports
docs/             architecture and the IPC contract
```

## Roadmap

1. **Scaffold** - local shell tabs, CI on three platforms. *(done)*
2. **SSH core** - `russh` with agent, key, password and keyboard-interactive
   auth; host key verification and prompts; pty channel. *(done)*
3. **Vault** - SQLite host store, session tree, OS keychain, `~/.ssh/config`
   import, Xshell `.xsh` import. *(done)*
4. **Terminal polish** - split panes, find in scrollback, a user-editable
   keymap, highlight rules, session logging, and importing VS Code / iTerm /
   Windows Terminal colour schemes. *(done)*
5. **SFTP on the shared connection** - docked file panes, local pane,
   navigation. *(done)*
6. **Transfer engine** - queue, resume, conflicts, drag and drop,
   open-in-editor. *(done)*
7. Port forwarding, snippets, follow-cwd.
8. Packaging: installers, portable mode, encrypted vault export/import. **MVP.**
9. Triggers and notifications, fleet runner, SFTP extras, sync adapters,
   serial and telnet, auto-update, E2E tests.

## Migrating from Xshell

In Xshell, *Tools › Backup* writes a single `.xts` file. Point Harbour at it -
*Import Xshell* at the foot of the sidebar - and it reads the backup in place:

- **Sessions**, mirroring the session-manager folder tree, with host, port,
  protocol, username and description for each.
- **Host keys** Xshell had accepted, offered for review. New ones go to
  Harbour's own `known_hosts`; a key that differs from one Harbour already
  trusts is shown and refused, and must go through the connect-time prompt.
- **Colour schemes** and **highlight sets**, from Settings.

The backup also contains your private keys and a credential file. Harbour
never opens those - they are not its to copy. A directory of exported `.xsh`
files works too.

Stored passwords are **not** decoded. Xshell encrypts them against the Windows
account and, if set, a master password, using a scheme that differs across
Xshell 5, 6 and 7; recovering them would be fragile and would mean writing
recovered plaintext into a new store. Imported hosts are flagged as having had a
password so Harbour can prompt once, on first connect, and put it in the OS
keychain.

The session parser and its tests are in `src-tauri/src/vault/xshell.rs`; the
backup layout is in `src-tauri/src/xts.rs`.

## Documentation

- [`docs/architecture.md`](docs/architecture.md) - how the pieces fit and the rules they follow
- [`docs/ipc.md`](docs/ipc.md) - the command and event contract
- [`docs/security.md`](docs/security.md) - host keys, secrets, and what is not implemented yet
- [`docs/releasing.md`](docs/releasing.md) - how a tag becomes a set of installers

## Licence

MIT. See [LICENSE](LICENSE).
