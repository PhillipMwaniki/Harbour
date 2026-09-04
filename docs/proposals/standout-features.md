# Standout features — design spec

Three features that lean on what Harbour already has (a Rust core, one SSH
connection multiplexing shell + SFTP + forwards + exec, the vault, the secret
store, and the output tap) and that most SSH clients handle poorly or not at
all. In recommended build order:

1. [Key generation + deploy](#1-key-generation--deploy) — the everyday win.
2. [Broadcast input](#2-broadcast-input) — small, high delight.
3. [Command guardrails on production](#3-command-guardrails-on-production) — a safety differentiator.

Each section: what it is, the UX, the data-model and IPC changes, the backend
approach, security, edge cases, testing, and a rough effort estimate.

---

## 1. Key generation + deploy

**What it is.** Turn "I connect to this host with a password" into "I connect
with a key" in one action: generate an Ed25519 keypair (or reuse an existing
one) and install its public half into the host's `~/.ssh/authorized_keys`, then
point the saved host at the key. It is `ssh-keygen` + `ssh-copy-id`, as a button,
over a connection Harbour already has.

Why it stands out: almost no GUI client does the *deploy* half well, and it is
the highest-friction moment in an SSH user's life.

### UX

Two entry points, same underlying flow:

- **Host editor** → a "Set up key authentication…" button next to the private
  key field. Opens a small dialog:
  - Choose **Generate a new key** (default) or **Use an existing key** (Browse to
    a public key, or pick one Harbour generated before).
  - New key: name (default `harbour_ed25519`), optional passphrase, and where to
    save it (default `~/.ssh/`).
  - A note showing exactly what will be appended to the remote
    `authorized_keys`, and to which host.
  - **Deploy** connects (asking for the password once, the normal prompt flow),
    installs the key, and — on success — sets the host's `auth.keyPath` and
    saves. A closing summary: "Key installed on web-prod. It will use the key
    from now on."
- **A live session** → context action "Copy SSH key here…", which skips the
  connect step (it reuses the open session) and just deploys, then offers to
  attach the key to the matching saved host if there is one.

### Data model

No schema change required. The host already has `auth.keyPath`; deploy sets it.
Optionally record generated keys in settings so the "use an existing key" picker
can list Harbour's own:

```rust
// settings/mod.rs
pub struct GeneratedKey {
    pub path: String,        // the private key path
    pub public_path: String, // the .pub
    pub fingerprint: String, // SHA256:...
    pub created_at: i64,
}
// Settings { ... pub generated_keys: Vec<GeneratedKey> }
```

A generated key's passphrase, if any, goes in the secret store keyed by the key
path — never in settings.

### IPC

```ts
// Generate a keypair, write both files, return the public key for review.
key_generate({ path: string; passphrase?: string })
  -> { path: string; publicKey: string; fingerprint: string }

// Install a public key into a host's authorized_keys. Either over a live
// session (sessionId) or by connecting a saved host (hostId).
key_deploy({ target: { sessionId } | { hostId }; publicKey: string })
  -> { alreadyPresent: boolean }
```

`host_setup_key` can orchestrate generate → deploy → update the vault host, but
keeping the two commands separate keeps them testable and reusable.

### Backend

**Generation** — `russh::keys` already ships the `ssh-key` crate.

```rust
use russh::keys::ssh_key::{PrivateKey, Algorithm, LineEnding};
let key = PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519)?;
let key = if let Some(pw) = passphrase { key.encrypt(&mut OsRng, pw)? } else { key };
std::fs::write(&path, key.to_openssh(LineEnding::LF)?)?;      // 0600
std::fs::write(&public_path, key.public_key().to_openssh()?)?; // 0644
```

Set `0600` on the private key on Unix (`std::os::unix::fs::PermissionsExt`); on
Windows, tighten the ACL or at least document it. The public key's comment
should be something identifying, e.g. `harbour@<hostname>` or the user's login.

**Deploy** — reuse the `run_command` exec path already built for the fleet
runner. One idempotent command handles the create-dirs / dedupe / perms dance:

```sh
install -d -m 700 ~/.ssh && \
touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && \
grep -qxF '<PUBLIC_KEY>' ~/.ssh/authorized_keys || echo '<PUBLIC_KEY>' >> ~/.ssh/authorized_keys
```

The public key is base64 + a comment — no single quotes — so single-quoting it
is safe; still, validate it parses as a public key before sending, and reject
anything with a newline or a single quote defensively. `grep -qxF` makes it
idempotent (`alreadyPresent` = the grep matched). For a live session, run the
same command over a fresh exec channel via the `ChannelOpener` the session
already holds, rather than typing into the user's shell.

### Security

- The private key never leaves the machine; only the public key is sent.
- Passphrase goes to the secret store, never to settings or logs.
- File permissions set explicitly; a world-readable private key is a bug.
- Deploy is an outward action (it writes to a remote), so the dialog shows the
  exact line and the target host before the user confirms — matching the
  "confirm side-effectful actions" rule.
- Idempotent: re-running never appends a duplicate.

### Edge cases

- `~/.ssh` on a remote with a non-POSIX shell (fish, a restricted shell): the
  command above is POSIX `sh`; wrap as `sh -c '<...>'` to be safe.
- A host that only allowed password auth and disables it later — out of scope;
  we only add the key, we do not touch `sshd_config`.
- Deploy to a host reached through a jump chain: works unchanged, since
  `run_command` already follows the chain.

### Testing

- Unit: key generation produces a valid OpenSSH keypair; the fingerprint
  matches; permissions are `0600`.
- Integration (against the test SSH server): `key_deploy` runs the exec, and a
  second deploy reports `alreadyPresent: true`. The server's `exec_request`
  handler (already added for the fleet tests) can record the command and fake an
  `authorized_keys`.

**Effort: medium.** Generation is small; deploy reuses `run_command`; the dialog
is one component.

---

## 2. Broadcast input

**What it is.** Type once, send the keystrokes to every session in a group —
run the same interactive command on ten servers at the same time and watch each
respond. The interactive twin of the fleet runner, and a feature people switch
terminals for (iTerm/tmux `synchronize-panes`), made better by pairing it with
the vault.

### UX

- A **broadcast toggle** in the tab/toolbar: "Broadcast to all panes in this
  tab" is the common case; an "Advanced…" option lets the user pick an arbitrary
  set of open sessions as a group.
- While broadcasting, every pane in the group gets an unmistakable marker (a
  coloured border + a small "BROADCAST" badge), because sending `rm` to ten
  boxes by accident is exactly the disaster to design against.
- Input typed in *any* group pane is mirrored to all of them. Output is not
  merged — each terminal shows its own, so you see divergence immediately.
- A one-key escape (Esc, or clicking the toggle) ends broadcast.

### Data model / IPC

None new. This is a frontend concern built on `sessionWrite`, which already
takes `(sessionId, bytes)`.

```ts
// stores/broadcast.ts
interface BroadcastState {
  active: boolean;
  members: Set<string>;        // session ids
  toggle(): void;
  setMembers(ids: string[]): void;
}
```

In `TerminalView`, the `term.onData` handler already calls
`sessionWrite(info.sessionId, bytes)`. When broadcast is active and this pane is
a member, fan out instead:

```ts
const bc = useBroadcast.getState();
const targets = bc.active && bc.members.has(info.sessionId)
  ? [...bc.members]
  : [info.sessionId];
for (const id of targets) void sessionWrite(id, bytes).catch(() => {});
```

### Edge cases

- **Only send to live sessions** — drop ids whose session has closed (the store
  prunes on `session:closed`).
- **No echo loop** — we fan out raw input, not output, so there is no loop.
- **Resize stays per-pane** — each terminal keeps its own size; broadcasting is
  input only.
- **Local shells** can be in a group too (handy), but default the "all panes"
  action to remote sessions to avoid surprises.
- **Paste + broadcast** — a multi-line paste to a group should go through the
  existing paste-confirmation dialog once, then fan out.

### Testing

- Unit (store): membership, prune-on-close, toggle.
- Component: with broadcast active and two panes as members, a keystroke in one
  calls `sessionWrite` for both; with it off, only the focused one.

**Effort: small-to-medium**, entirely frontend. High delight per line of code.

---

## 3. Command guardrails on production

**What it is.** Before a destructive command runs on a host flagged
*production*, Harbour stops and asks — showing the command and the host — so
`rm -rf /`, `mkfs`, `dd`, `shutdown`, `DROP TABLE`, `git push --force` don't go
through on the wrong box. It fits Harbour's existing "make production
unmistakable" stance (host themes) and reuses the highlight/trigger matcher.

### Where it can be reliable — and where it can't

The honest constraint: for **typed** input, the shell owns line editing, so
Harbour cannot always know the exact command about to run without shell
integration. So scope guardrails to the places where Harbour *does* hold the
full command text, which are also where batch mistakes actually happen:

1. **Fleet runner** — Harbour has the exact command; block or confirm per host
   risk. Fully reliable. *(Highest value, smallest work.)*
2. **Snippet insertion and paste** into a guarded session — Harbour has the
   text; confirm before it is sent. Reliable, and reuses the paste-confirm
   pattern already in place.
3. **Typed input** — best-effort: reconstruct the current line from keystrokes
   and check it on Enter. Works for the common case (no fancy editing); be
   explicit in the UI that it is a safety net, not a guarantee. Optional, and
   can ship later behind the reliable two.

A later, fully-reliable typed-input version can use **shell integration**: ship
a bash/zsh `preexec` snippet that emits the command over a private OSC sequence
Harbour reads (the same mechanism iTerm/Warp use). That is its own feature; the
spec above delivers guardrails without it.

### Data model

```rust
// A host carries an explicit risk flag (independent of its theme).
// vault schema v3: ALTER TABLE hosts ADD COLUMN guarded INTEGER NOT NULL DEFAULT 0
Host { ..., pub guarded: bool }

// Editable rules, defaulted to a sensible built-in set (settings/mod.rs).
pub struct Guardrail {
    pub id: String,
    pub label: String,      // "Recursive delete"
    pub pattern: String,    // JS regex, e.g. \brm\s+(-\w*\s+)*-?[rf]
    pub enabled: bool,
}
// Settings { ... pub guardrails: Vec<Guardrail> }
```

Ship defaults for `rm -rf`, `dd`, `mkfs`, `shutdown`/`reboot`/`halt`,
`> /dev/sd*`, `chmod -R … /`, `truncate`, `DROP TABLE`/`DROP DATABASE`,
`git push --force`. The matcher is the compiled-regex engine already written for
highlights and triggers — reuse `compile*` and a per-line test.

### UX

- Host editor gains a **"Guard this host"** checkbox (with a hint: "confirm
  destructive commands before they run"). Guarded hosts get a small shield in
  the session tree and on the tab.
- When a guarded action is about to run a matching command, a **confirm dialog**:
  the command (with the matched fragment highlighted), the host name, the rule
  that fired, and **Run** / **Cancel** (Cancel focused by default).
- A Settings → **Guardrails** editor, mirroring the Triggers/Highlights editors.

### IPC

Minimal. Guardrail evaluation is frontend (same as highlights/triggers). The
only backend change is the `guarded` host field flowing through the existing
`vault_*` commands and the schema v3 migration. The fleet runner can also apply
guardrails server-side as defence in depth, but the primary check is at the UI
boundary where the command originates.

### Edge cases

- A guarded rule matching a *substring* of a safe command (`rm -rf` inside a
  quoted string): accept some false positives — a confirm dialog is cheap, and
  erring toward asking is the point.
- Fleet run over mixed hosts: evaluate per host, so the confirm names which
  guarded hosts a command would touch.
- Turning guardrails off entirely: a global toggle, off = today's behaviour.

### Testing

- Unit: the matcher flags the default-dangerous set and clears benign commands.
- Component: a guarded fleet target with a matching command shows the confirm;
  Cancel aborts that host; an unguarded host runs without a prompt.
- Store/migration: schema v3 adds `guarded`, defaulting existing hosts to false.

**Effort: medium** for the reliable parts (fleet + paste/snippet + the flag +
editor); the typed-input safety net and shell integration are separable follow-
ups.

---

## Suggested order and rationale

| Feature | Effort | Leans on | Why first |
| --- | --- | --- | --- |
| Key generation + deploy | Medium | `run_command`, secret store, vault | Highest everyday payoff; self-contained |
| Broadcast input | Small–medium | `sessionWrite`, session store | Cheap, delightful, pure frontend |
| Command guardrails | Medium | trigger matcher, host flag, fleet | Safety story; do the reliable parts first |

All three are independent and shippable on their own branch, each with the same
gates the rest of Harbour holds to (Rust `fmt` + `clippy -D warnings` + tests;
frontend typecheck + tests + build), and each documented in `docs/ipc.md`,
`docs/security.md` and the README as it lands.
