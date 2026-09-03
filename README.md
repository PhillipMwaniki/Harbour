# Harbour

A cross-platform SSH client that puts the terminal, the session manager and an
SFTP file manager in one window, sharing one connection per host. Local-first:
no account, no cloud, credentials never leave the machine unencrypted.

Harbour is an Xshell + Xftp replacement for teams that live on Windows but ship
to Linux, and it runs the same on macOS and Linux.

> **Status: milestone 1 of 9.** Local shells work end to end - tabs, ConPTY and
> forkpty, batched output with real backpressure, and a themed UI with eleven
> built-in colour schemes. SSH, the session vault, SFTP and transfers are next.
> See [the roadmap](#roadmap).

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

## Themes

Eleven built-in schemes - Harbour Dark, Dark+, Light+, Monokai, Dracula, Nord,
One Dark, Solarized Dark/Light, Gruvbox Dark and Tokyo Night - switchable from
the **Theme** button in the tab bar. A theme covers the whole window, not just
the terminal: chrome colours are published as CSS custom properties, so the tab
bar, menus and dialogs move with it. The choice is remembered between runs.

Importing VS Code, iTerm and Windows Terminal colour schemes lands in
milestone 4, along with per-host theme overrides.

## Keyboard

| Shortcut | Action |
| --- | --- |
| `Ctrl+Shift+T` | New terminal (default shell) |
| `Ctrl+Shift+W` | Close the active terminal |

A user-editable keymap arrives in milestone 4.

## Layout

```
src/              React frontend
  app/            layout shell
  components/     terminal (and later sftp, sessions, settings)
  ipc/            typed wrappers around invoke/listen - the only place
                  that knows command names
  stores/         zustand stores, one per domain
  lib/            themes, formatting, path helpers
src-tauri/src/    Rust core
  commands/       thin IPC handlers
  session/        session manager, pty, output pump, shell detection
docs/             architecture and the IPC contract
```

## Roadmap

1. **Scaffold** - local shell tabs, CI on three platforms. *(done)*
2. SSH core: `russh` with password, key and agent auth; host key prompt; pty channel.
3. Vault: SQLite host store, session tree, keyring, `~/.ssh/config` import,
   **Xshell `.xsh` import** (parser done - see below).
4. Terminal polish: splits, search, keymaps, highlight rules, logging, and
   importing VS Code / iTerm / Windows Terminal colour schemes.
5. SFTP on the shared connection: docked pane, local pane, navigation.
6. Transfer engine: queue, resume, conflicts, drag and drop, open-in-editor.
7. Port forwarding, snippets, follow-cwd.
8. Packaging: installers, portable mode, encrypted vault export/import. **MVP.**
9. Triggers and notifications, fleet runner, SFTP extras, sync adapters,
   serial and telnet, auto-update, E2E tests.

## Migrating from Xshell

Point Harbour at an Xshell export directory and it will walk the `.xsh` files,
mirroring the session-manager folder tree, and import host, port, protocol,
username, description, key name and encoding for each session.

Stored passwords are **not** decoded. Xshell encrypts them against the Windows
account and, if set, a master password, using a scheme that differs across
Xshell 5, 6 and 7; recovering them would be fragile and would mean writing
recovered plaintext into a new store. Imported hosts are flagged as having had a
password so Harbour can prompt once, on first connect, and put it in the OS
keychain.

The parser and its tests are in `src-tauri/src/vault/xshell.rs` today; the
import UI arrives with the vault in milestone 3.

## Documentation

- [`docs/architecture.md`](docs/architecture.md) - how the pieces fit and the rules they follow
- [`docs/ipc.md`](docs/ipc.md) - the command and event contract

## Licence

MIT. See [LICENSE](LICENSE).
