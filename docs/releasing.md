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
macOS, one per architecture; `.deb`, `.rpm` and `.AppImage` on Linux.

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

The **updater** signing key is different, and is the one the workflow already
knows about: `TAURI_SIGNING_PRIVATE_KEY` and its password, as repository
secrets. They are for the milestone 9 auto-update, which verifies every
downloaded update against the public half. Until they exist the variables are
empty, the build is unaffected, and no `.sig` files are produced.

## Where things live

- `.github/workflows/release.yml` - the pipeline.
- `.github/workflows/ci.yml` - what every pull request runs; `test` in the
  release workflow mirrors it.
- `scripts/check-version.mjs`, `scripts/release-notes.mjs` - the two checks,
  runnable locally with plain Node, no dependencies.
- `CHANGELOG.md` - the source of every release body.
