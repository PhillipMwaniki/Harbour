<p align="center">
  <img src="assets/app-icon.png" alt="Harbour" width="128" height="128" />
</p>

<h1 align="center">Harbour</h1>

A cross-platform SSH client that puts the terminal, the session manager and an
SFTP file manager in one window, sharing one connection per host. Local-first:
no account, no cloud, credentials never leave the machine unencrypted.

Harbour is an Xshell + Xftp replacement for teams that live on Windows but ship
to Linux, and it runs the same on macOS and Linux.

> **Status: milestone 9 of 9.** Beyond the MVP: output **triggers** with desktop
> notifications, a **fleet runner** that runs one command across many hosts,
> **serial** and **telnet** session kinds alongside SSH, folder-based vault
> **sync**, SFTP permissions and a properties dialog, and an end-to-end suite
> that drives the built app. Milestone 8 made Harbour portable and encrypted - a
> master password, an encrypted vault you can export, import and sync, and a
> portable mode beside the executable. Under all of it: jump hosts of any depth,
> port forwarding, snippets, self-update; SFTP and transfers on the shared
> connection; a terminal with splits, find, keymap, highlight rules and logging;
> SSH with agent, key and password auth, host keys checked against your
> `known_hosts`, and imports from `~/.ssh/config` (with `ProxyJump`) and Xshell
> backups. See [the roadmap](#roadmap).

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
pnpm test:e2e                 # end-to-end, against the built app (see below)
```

The end-to-end tests drive the real, built app through `tauri-driver`. They
need the debug binary built first (`pnpm tauri build --debug --no-bundle`) and
`tauri-driver` on the path (`cargo install tauri-driver`); on Linux they also
need `webkit2gtk-driver` and run under a virtual display (`xvfb-run`). CI runs
them on Linux on every pull request - `tauri-driver` has no macOS support.

Set `HARBOUR_LOG=harbour_lib=debug` for verbose backend logging. Logs are
written to the platform app-log directory as well as stderr.

Installers for each tagged version are on the
[Releases](https://github.com/PhillipMwaniki/Harbour/releases) page, with a
`SHA256SUMS` file beside them. They are not code-signed yet: Windows shows the
"unknown publisher" warning, and macOS needs a right-click → Open the first
time. How a release is cut is in [`docs/releasing.md`](docs/releasing.md).

On Linux, pick the package for your distribution: a `.deb` for Debian and
Ubuntu, an `.rpm` for Fedora and openSUSE, or the `.AppImage` for anything
else - CI installs each on the distribution it is for before a release goes
out. On Arch, `harbour-bin` on the AUR repackages the release and `harbour`
builds it from source.

## Connecting over SSH and telnet

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

The connect dialog has a **SSH / Telnet / Serial** switch. Telnet is a raw TCP
connection with no encryption and no credentials of its own - pick it for a
switch console, a BBS or a legacy daemon, give it a host and a port, and
whatever login the far end wants happens in the terminal. Harbour handles the
telnet option negotiation and reports the window size; there is no SFTP or port
forwarding on a telnet session.

**Serial** opens a console straight onto a local port - a router being flashed,
a microcontroller, anything on a USB-to-serial cable. Pick the port from the
list (Refresh re-scans) and a baud rate, and you have a terminal on the wire.
There is no login and no encryption; it is a direct byte pipe.

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

If your machine has no usable keychain, Harbour keeps saved secrets in an
encrypted file behind a **master password** instead (see below). Until you set
one, it simply asks every time rather than writing a password somewhere you did
not choose.

**Set up key authentication** in one step: editing a saved host, *Set up key
authentication…* generates an Ed25519 keypair and installs its public half in
the host's `authorized_keys`, connecting with your password once to do it. The
private key never leaves your machine; afterwards the host connects with the
key. It is `ssh-keygen` + `ssh-copy-id`, as a button, and the install is
idempotent - running it again never adds a duplicate.

## Importing an existing estate

Both importers show you everything they found and write nothing until you
press Import.

**OpenSSH** reads `~/.ssh/config`, following `Include` directives, and applies
`ssh`'s own rules - first value wins, and wildcard `Host` blocks contribute
defaults rather than becoming hosts of their own. A host's `ProxyJump` is
carried across too: if the bastion it names is imported in the same pass, the
host arrives already wired to tunnel through it, exactly as it would over
`ssh -J`.

**Xshell** reads a `.xts` backup - the file *Tools › Backup* writes, which is
what most people actually have - or a directory of exported `.xsh` files,
mirroring the folder tree either way. Sessions that are not SSH are listed
with the reason they cannot come across rather than being dropped silently. A
backup also offers the host keys Xshell had accepted, so a migrated estate does
not greet you with a trust-on-first-use prompt per host. Entries whose source never named a username are
skipped unless you supply one to fall back on - importing under a guess would
produce hosts that fail to connect for a reason you cannot see.

## Backups, master password and portable mode

**Export vault** (in the session-manager footer) seals your whole tree of
folders and hosts into one encrypted file. Tick *Include saved passwords* and
your credentials go in too, making it a full backup; leave it off for a
shareable list of hosts. The file is sealed with Argon2id and
XChaCha20-Poly1305 under a passphrase you choose - the only thing that opens it,
with no recovery if it is lost. **Import vault** merges such a file back in,
appending everything alongside what you already have rather than overwriting, so
importing into a populated vault is safe and importing the same file twice makes
two copies rather than a conflict.

On a machine with **no system keychain**, a **master password** unlocks an
encrypted file that holds your saved passwords in the same sealed format. You
set it once, enter it when Harbour starts (or skip, and be asked per host), and
can change it from the session-manager footer. The master password is held in
memory only while unlocked and wiped when Harbour closes; the file on disk is
never anything but ciphertext.

**Sync** (Settings › Sync) keeps the vault in step across your machines through
a folder something else already syncs - Dropbox, OneDrive, iCloud. Point it at a
file in that folder, and **Push** writes an encrypted copy of the whole vault
there while **Pull** merges it back on another machine. It is the encrypted
export and import with the path remembered and one click either way; the
passphrase is the only thing that opens the file and is never stored. Pull
merges rather than replaces, so run it once per machine to bring another's hosts
across.

**Portable mode** keeps everything - vault, settings, logs, known hosts, and the
master-password secret file - in a `data` folder beside the executable instead
of in your user profile. Turn it on by placing an empty file named `portable`
next to the Harbour executable. A portable copy leaves nothing on the host
machine and never touches its keychain, so it carries its own secrets behind the
master password: ideal for a USB stick.

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
one toggle. Right-click for new folder, rename, delete and **Properties** -
which shows a file's path, size, timestamp and owner, and on the remote side
lets you change its permission bits with a `chmod` grid. Delete asks first.

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

## Jump hosts and port forwarding

A saved host can sit **behind a bastion**: the Jump host field in the host
editor points at another saved host to tunnel through, and that host can have a
jump of its own, so a chain is as deep as the estate needs. Each hop verifies
its own host key and authenticates with its own credentials, exactly as
`ssh -J` does, and closing the terminal closes the whole chain.

**Ctrl+Shift+P** opens port forwards for the focused SSH terminal: a local
port (or an automatic one), a remote host and a port, carried over the
connection the terminal already has - no second login. The remote host is
resolved on the far side, so `localhost` is the server's own. A forward can be
exposed on the network with an explicit opt-in that is warned about.

## Snippets and the shell

**Ctrl+Shift+I** opens the snippet palette: type to filter your saved commands,
Enter to insert one into the terminal. Snippets are managed in Settings and
stored in `settings.json`.

A **multi-line paste** is confirmed first, showing exactly what will be sent -
a pasted block runs each line as its newline lands, and reading it before it
runs is the difference between a convenience and a foot-gun. Single-line pastes
are not interrupted.

The file panes can **follow the shell**: the Follow toggle makes the pane track
the directory the focused terminal reports over OSC 7, remote or local. The
shell has to emit OSC 7 (most modern bash/zsh setups can be configured to).

## Running on many hosts

**Run on many hosts…** at the foot of the sidebar opens the fleet runner: tick
the saved hosts you want, type a command, and it is run on each at once - as a
one-shot `exec`, not a shell - with the output collected. Results fill in one
host at a time as each finishes, colour-coded by exit status, and clicking a row
shows its output. The run is non-interactive by design: a host whose key you
have not trusted yet, or whose password is not saved, comes back as an error
rather than stopping to ask, so a command across the whole estate is something
you can start and walk away from. Up to eight hosts run at a time, and jump
hosts are followed exactly as they are for an interactive connection.

## Triggers

Under Settings › Triggers, a **trigger** watches a session's output for a
regular expression and acts the moment it appears: a desktop **notification**
(so you can leave a long build and be told when it prints `BUILD SUCCESSFUL`),
a **bell**, or **send** text back to the session - a canned `y⏎` for a prompt
you always answer the same way. Matching runs a line at a time on the streaming
output, with escape sequences stripped first, so a pattern matches what you see
rather than the raw colour codes. Triggers are per-pattern, not per-host, and
apply to every session.

## Updating

Harbour checks GitHub for a newer release on launch and offers it in a bar
under the tabs. Installing is one click; the app then asks to restart, and
never relaunches on its own while sessions are open. Every update is verified
against a signing key before it is applied.

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
| `Ctrl+Shift+P` | Show or hide port forwards |
| `Ctrl+Shift+I` | Insert a snippet |
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
  ssh/forward.rs  local port forwards on a session's connection
  ssh/            connect and auth, channel transport, sftp, known_hosts, agent
  vault/          sqlite host store, os keychain or master-password file,
                  encrypted export/import, ssh_config and xshell imports
  crypto.rs       the Argon2id + XChaCha20-Poly1305 envelope both of those seal with
  portable.rs     portable-mode detection: data beside the executable
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
7. **Port forwarding, jump hosts, snippets, follow-cwd** - local forwards,
   bastions of any depth, a snippet palette, follow-the-shell, and a
   multi-line paste confirmation. Self-update from GitHub releases. *(done)*
8. **Portable, encrypted, movable** - a master password and an encrypted secret
   file for machines without a keychain, encrypted vault export/import to carry
   an estate between machines, and portable mode that keeps everything beside
   the executable. **MVP.** *(done)*
9. **Beyond the MVP** - output triggers with desktop notifications, a fleet
   runner (one command across many hosts), SFTP permissions and a properties
   dialog, folder-based vault sync, serial and telnet session kinds, a private
   key file picker, `ProxyJump` at import, and an end-to-end test suite that
   drives the built app. *(done)*

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
