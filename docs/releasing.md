# Releasing

A release is a git tag. Pushing `vX.Y.Z` runs `.github/workflows/release.yml`,
which builds installers for Windows, macOS (Apple silicon and Intel) and Linux,
attaches them to a **draft** GitHub release with the changelog entry as its
body, and adds a `SHA256SUMS` file. A person then reads the draft and clicks
Publish. Nothing in the pipeline can put an installer in front of users on its
own.

## Cutting a release

1. **Pick the version.** Semantic versioning; `0.x` until the milestone 8 MVP.
   A pre-release is `vX.Y.Z-beta.1` and is marked as such on GitHub.

2. **Bump it in all three places.** They must agree with the tag exactly, or
   the workflow stops before building anything.

   - `package.json` → `"version"`
   - `src-tauri/tauri.conf.json` → `"version"` (this is what the installers
     carry)
   - `src-tauri/Cargo.toml` → `version`

   Then `cargo generate-lockfile` is *not* needed, but `cargo build` once so
   `Cargo.lock` picks up the new version of the `harbour` package.

3. **Cut the changelog.** In `CHANGELOG.md`, rename `## [Unreleased]` to
   `## [X.Y.Z] - YYYY-MM-DD` and open a fresh, empty `## [Unreleased]` above
   it. The section becomes the release body verbatim; the workflow refuses a
   version with no section, or an empty one.

4. **Check locally.** Both scripts the workflow runs can be run here:

   ```bash
   node scripts/check-version.mjs vX.Y.Z
   ```

   ```bash
   node scripts/release-notes.mjs X.Y.Z
   ```

5. **Commit, tag, push.**

   ```bash
   git commit -am "Release X.Y.Z"
   ```

   ```bash
   git tag vX.Y.Z && git push origin master vX.Y.Z
   ```

6. **Wait, then publish.** The Release workflow takes ten to twenty minutes.
   When it is green, the draft is at *Releases* with every installer and
   `SHA256SUMS` attached. Read the notes, try one installer, click Publish.

## What the workflow does

| Job | Purpose |
| --- | --- |
| `verify` | Refuses a tag that is not `vX.Y.Z`, that disagrees with the three version fields, or that has no changelog section. Produces the version and the release body for the rest. |
| `test` | The same gate CI applies to every pull request, on all three platforms. A tag that does not pass it does not ship. |
| `build` | `tauri-apps/tauri-action` on a four-way matrix: Windows x64, macOS arm64, macOS x64, Linux x64. The first to finish creates the draft; each attaches its bundles. One platform failing does not cancel the others. |
| `checksums` | Downloads everything on the draft and attaches `SHA256SUMS`. Runs even if a platform failed, over whatever did upload. |

Installers produced: `.msi` and NSIS `.exe` on Windows; `.dmg` and `.app` on
macOS, one per architecture; `.deb`, `.rpm` and `.AppImage` on Linux, plus the
two Arch packages below once the release is published.

## Linux

Ubuntu builds the Linux installers, but Ubuntu is not where most of them are
installed, so CI installs what it built where it will be used:

| Job | What it proves |
| --- | --- |
| Install on Fedora (rpm) | `dnf install` of the `.rpm` resolves its dependencies on current Fedora |
| Install on Debian (deb) | `apt-get install` of the `.deb` resolves on Debian stable, not only Ubuntu |
| Arch package (from source) | `packaging/aur/harbour/PKGBUILD` builds and installs from this checkout with `makepkg` |

The `.deb` works out its own dependencies from what the binary links. The
`.rpm` cannot, so they are named in `tauri.conf.json` under `bundle.linux.rpm`
- `webkit2gtk4.1`, `gtk3`, `librsvg2` - and the Fedora job is what catches a
name going stale.

### The AUR

Two packages, both in `packaging/aur`: **`harbour-bin`** repackages the
release's `.deb`, and **`harbour`** builds from the tagged source. They are
published by `.github/workflows/aur.yml` when a release is *published* - not
when the draft is created, because the AUR must never point at installers
nobody has looked at. The workflow stamps the version and the `.deb` asset
name into the PKGBUILDs, computes the checksums, test-builds `harbour-bin`,
and pushes both to `aur.archlinux.org`.

It needs three repository secrets, for an AUR account that owns the packages
(the first push creates them):

| Secret | Value |
| --- | --- |
| `AUR_SSH_PRIVATE_KEY` | A private key whose public half is registered on the AUR account |
| `AUR_USERNAME` | The account's name, used as the commit author |
| `AUR_EMAIL` | The account's email |

Without them the workflow prints a notice and does nothing. To publish a
version that is already on the Releases page - or to retry - run *AUR* from
the Actions tab with the version.

## Re-running

If a platform fails for a reason that was not the code - a runner outage, a
flaky download - re-run the failed jobs from the Actions tab. `tauri-action`
finds the existing draft by tag and adds to it; `checksums` replaces the sums
file. If the fix needs a commit, delete the draft and the tag, and tag again:
a tag that moves is worse than a tag that is skipped.

## Signing

Nothing is code-signed yet. What that means for people installing:

- **Windows** shows the SmartScreen "unknown publisher" warning. Fixing it
  takes an Authenticode certificate: `bundle.windows.certificateThumbprint`
  in `tauri.conf.json` and the certificate installed on the runner from a
  secret. That is separate from the updater key below. Not configured.
- **macOS** refuses to open an unsigned, un-notarised app by default. Until
  there is an Apple Developer ID: right-click → Open the first time, or
  `xattr -cr /Applications/Harbour.app`. Signing and notarisation are
  `APPLE_CERTIFICATE`, `APPLE_ID` and friends in the `build` environment;
  `tauri-action` handles the rest once they exist. Not configured.
- **Linux** has no equivalent expectation.

## The updater

Harbour updates itself from these releases. The app carries the **public** half
of a signing key (in `tauri.conf.json`, `plugins.updater.pubkey`); CI signs each
release's `latest.json` with the **private** half, and the app verifies every
update against the public half before applying it.

For that to happen, two repository secrets must be set, for the keypair that
matches the public key in the config:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | The contents of the private key file generated with the public key in the config |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The key's password (empty if it was generated without one) |

Until they are set, the release still builds and the draft is still created -
just unsigned, with no `latest.json`, so the in-app updater finds nothing and
is inert. Add the secrets and re-run the Release workflow for a signed release
the updater will pick up. A new keypair can be generated with `pnpm tauri signer generate`; doing
so means replacing `plugins.updater.pubkey` with the new public key in the same
commit, or existing installs will reject every future update.

## Where things live

- `.github/workflows/release.yml` - the pipeline.
- `.github/workflows/ci.yml` - what every pull request runs; `test` in the
  release workflow mirrors it.
- `scripts/check-version.mjs`, `scripts/release-notes.mjs` - the two checks,
  runnable locally with plain Node, no dependencies.
- `CHANGELOG.md` - the source of every release body.
