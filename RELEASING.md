# Releasing Mumcord

Releases are built by GitHub Actions (`.github/workflows/release.yml`). Pushing a version tag such as `v0.1.0` that points to a commit on `main` builds the Windows installer (`.exe`), the macOS disk image (`.dmg`, universal for Apple Silicon and Intel) and the Linux AppImage, smoke-tests each one against a local Mumble server, and creates a **draft** GitHub Release with the files and a `SHA256SUMS` checksum file. Nothing becomes public until you publish the draft.

Builds are unsigned until code signing arrives (Epic 5). The release notes say so and explain the Windows SmartScreen and macOS Gatekeeper warnings.

## 1. Bump the version

The version lives in three places, which must all match the tag (the workflow checks this and stops otherwise):

| File                                   | What to change                                                    |
| -------------------------------------- | ----------------------------------------------------------------- |
| `package.json` and `package-lock.json` | `"version"`; both are updated by the `npm version` command below  |
| `Cargo.toml` (`[workspace.package]`)   | `version = "…"`; every crate, including the app, inherits it      |
| `Cargo.lock`                           | updated automatically by `cargo check` after editing `Cargo.toml` |
| `scripts/smoke/seed/Cargo.lock`        | the smoke-test seeder is its own Cargo project; refresh it too    |

`src-tauri/tauri.conf.json` has no version on purpose: Tauri takes the app version from `Cargo.toml`. Do not add one.

On a branch, for example for version `0.2.0`:

```sh
npm version 0.2.0 --no-git-tag-version   # package.json + package-lock.json
# edit Cargo.toml: [workspace.package] version = "0.2.0"
cargo check --workspace                  # refreshes Cargo.lock
cargo check --manifest-path scripts/smoke/seed/Cargo.toml   # refreshes the seeder's Cargo.lock
git add package.json package-lock.json Cargo.toml Cargo.lock scripts/smoke/seed/Cargo.lock
git commit -m "chore: release 0.2.0"
```

Open a pull request and merge it into `main` once CI is green.

## 2. Tag the release on `main`

```sh
git switch main
git pull
git tag -a v0.2.0 -m "Mumcord 0.2.0"
git push origin v0.2.0
```

The tag must be `v` plus exactly the version from step 1, and it must point to a commit on `main`.

## 3. Wait for the workflow

Open the **Actions** tab and follow the **Release** run for the tag. It takes roughly as long as the Test builds workflow (up to about an hour). It runs:

1. **Check tag**: the tag matches the version in all files above and the commit is on `main`.
2. **Build** on Linux, Windows and macOS.
3. **Smoke** test: each build is installed on a fresh machine and connects to a local test server.
4. **Create draft release**: attaches the files and `SHA256SUMS` and writes the release notes.

## 4. Review and publish

1. Open **Releases** on GitHub; the new release is marked **Draft**.
2. Check the notes: the fixed download and "unsigned" section, followed by the change list GitHub generated from merged pull requests. Edit the text if needed.
3. Check that the `.exe`, `.dmg`, `.AppImage` and `SHA256SUMS` files are attached. Optionally download one and try it.
4. Click **Publish release**.

## If the workflow fails

Nothing has been published, so it is safe to start over:

1. Read the failing step in the Actions run. A failed **Check tag** step says exactly which file has the wrong version or that the commit is not on `main`.
2. If a draft release was already created, delete it on the Releases page.
3. Delete the tag, locally and on GitHub:

   ```sh
   git tag -d v0.2.0
   git push origin :refs/tags/v0.2.0
   ```

4. Fix the problem with a normal pull request into `main`.
5. Tag the fixed `main` again (step 2).

Once a release is published, do not move or reuse its tag. Fix problems with a new version instead (for example `v0.2.1`).
