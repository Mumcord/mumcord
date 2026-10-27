# Releasing Mumcord

Releases are built by GitHub Actions (`.github/workflows/release.yml`). Pushing a version tag such as `v0.1.0` that points to a commit on `main` builds the Windows installer (`.exe`), the macOS disk image (`.dmg`, universal for Apple Silicon and Intel) and the Linux AppImage, smoke-tests each one against a local Mumble server, and creates a **draft** GitHub Release with the files and a `SHA256SUMS` checksum file. Nothing becomes public until you publish the draft.

Builds are unsigned until code signing is set up (Windows via Azure Artifact Signing, macOS via an Apple Developer ID: see [docs/release-signing.md](docs/release-signing.md) and the [code signing policy](CODE_SIGNING.md)). The release notes say per platform whether the build is signed and explain the SmartScreen and Gatekeeper warnings for unsigned ones.

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

Before that commit, write the release's section in `CHANGELOG.md` (replace "Unreleased" with the release date) and, for a feature release, a "What's new" block for the version in the **Write checksums and release notes** step of `.github/workflows/release.yml` (see the 0.3.0 and 0.4.0 blocks). Work through the manual checks of the release, for 0.4.0 [docs/manual-checklist-0.4.0.md](docs/manual-checklist-0.4.0.md).

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
2. **Build** on Linux, Windows and macOS. With signing set up, the Windows app and installer and the macOS app are signed automatically; nothing needs your approval.
3. **Smoke** test: each build is installed on a fresh machine and connects to a local test server.
4. **Create draft release**: attaches the files and `SHA256SUMS` and writes the release notes.

## 4. Review and publish

1. Open **Releases** on GitHub; the new release is marked **Draft**.
2. Check the notes: the fixed download and signing section, followed by the change list GitHub generated from merged pull requests. Edit the text if needed.
3. Check that the `.exe`, `.dmg`, `.AppImage` and `SHA256SUMS` files are attached. Optionally download one and try it.
4. Click **Publish release**.

## AppImage: no H.264 / GPL-only codecs

`bundleMediaFramework` (in `src-tauri/tauri.linux.conf.json`) makes Tauri's linuxdeploy GStreamer plugin copy every plugin in `GSTREAMER_PLUGINS_DIR` into the AppImage, together with their libraries. To keep H.264/H.265 (patents) and GPL-only codecs out, `.github/workflows/build-and-smoke.yml` sets `GSTREAMER_PLUGINS_DIR` to a filtered copy made by `scripts/filter-gst-plugins.sh` (it omits libgstlibav, x264, x265, OpenH264, a52dec, dtsdec, mpeg2dec, libdvdread, faac, faad). The smoke test fails if `libgstlibav`, `libavcodec` or `libx264` still end up in the AppImage. VP9 (`libgstvpx`) stays, so YouTube plays. The build host also installs gst-plugins-bad, but the script keeps only its Opus parser (`libgstopusparse`): without it WebKit reports Opus in WebM as unsupported and YouTube refuses to play; the smoke test checks it is bundled. The `.deb`, `.rpm` and AUR packages still depend on the distribution's `gst-libav`. To change the list, edit `scripts/filter-gst-plugins.sh`. The plugin scanner (`gst-plugin-scanner`) must be bundled too, or the web process warns "External plugin loader failed" for every plugin; linuxdeploy only finds it on Debian-style hosts, so the workflow sets `GSTREAMER_HELPERS_DIR` from `scripts/gst-helpers-dir.sh` and the smoke test checks it is in the AppImage. A local AppImage build needs both variables: `GSTREAMER_PLUGINS_DIR=$(scripts/filter-gst-plugins.sh) GSTREAMER_HELPERS_DIR=$(scripts/gst-helpers-dir.sh) npm run tauri build -- --bundles appimage`. At start the AppImage also loads the host's GStreamer plugins after the bundled ones (`src-tauri/src/gst_fallback.rs`), so a user who installs gst-libav gets H.264 and AAC; it uses its own plugin registry in `~/.cache/app.mumcord.client/gstreamer-1.0/`. Known risk: a host plugin built for a different GStreamer version than the bundled one may fail to load; GStreamer then skips it, playback stays without H.264 and the player shows the install hint.

## Static libvpx

Screen sharing encodes VP9 with libvpx. Every release bundle links a static copy into the app. Windows and macOS have no system libvpx. On Linux a package built on Ubuntu 24.04 against the shared library would need `libvpx.so.9`, which older distributions such as Debian 12 (`libvpx.so.7`) do not have, so the `.deb`, `.rpm` and AppImage would not start there. Local development builds and the Linux CI test builds (`ci.yml`) still link the distribution's shared libvpx (`libvpx-dev`, found with pkg-config).

1. `scripts/build-libvpx.sh <dir>` builds it. Windows: vcpkg (preinstalled on GitHub's runners) installs `libvpx:x64-windows-static-md` (static library, dynamic C runtime like Rust's default) and the script copies `include/vpx/` and `lib/vpx.lib`. macOS: the script downloads the libvpx release pinned in `LIBVPX_VERSION`, builds it for arm64 and x86_64 (x86_64 assembly needs `nasm` from Homebrew) and joins both into one universal `lib/libvpx.a` with `lipo`, so the universal DMG gets both halves. Linux: the script builds the same pinned release for the host architecture (x86_64 runner and ARM64 runner) with `--enable-pic` into `lib/libvpx.a`; x86_64 assembly needs `nasm` or `yasm` (apt).
2. `build-and-smoke.yml` (all platforms) and `ci.yml` (Windows and macOS) cache the result in the runner's temp folder, per system and architecture. The cache key changes when the script changes and, on Windows, when the runner's vcpkg ships a different libvpx version; a cache miss rebuilds it (a few minutes).
3. The workflows then set `LIBVPX_STATIC_DIR` to that folder, and `crates/screenshare/build.rs` links `static=vpx` from `<dir>/lib` and generates the bindings from `<dir>/include`. They also set `LIBCLANG_PATH` for bindgen: LLVM on the Windows runner (`C:\Program Files\LLVM\bin`), Xcode's toolchain on macOS; on Linux bindgen finds `libclang-dev` on its own. No Linux package lists libvpx as a runtime dependency.

To update libvpx on macOS and Linux, change `LIBVPX_VERSION` in the script; the next CI run rebuilds and caches it. On Windows the version follows the runner's vcpkg. The libvpx BSD licence and patent grant are in `THIRD_PARTY_NOTICES` and in `ui/public/third-party-notices.txt`, which is built into every bundle.

To build locally on a Mac (`brew install nasm`) or on Linux (`nasm` from the distribution): `scripts/build-libvpx.sh ~/libvpx-static`, then `export LIBVPX_STATIC_DIR=~/libvpx-static` before `npm run tauri build`. Check a Linux build with `ldd target/release/mumcord | grep vpx`, which must print nothing.

## Third-party notices

Run `scripts/generate-notices.sh` after dependencies change. The "native libraries" section (libvpx, PipeWire, GStreamer, WebKitGTK, source offer) is written in that script; update it when bundled native libraries change.

## Publishing the library crates (crates.io)

The library crates `mumcord-protocol` (`crates/protocol`), `mumcord-codec` (`crates/codec`), `mumcord-store` (`crates/store`), `mumcord-audio` (`crates/audio`) and `mumcord-p2p` (`crates/p2p`) are published on crates.io with the app version. Protocol, codec and store are independent; audio depends on codec and p2p on protocol, so those two wait for them. The **Release** workflow does it automatically in its last job, after the draft release exists; tags with a pre-release suffix (such as `-rc.1`) are skipped. On a minor or major version bump also change the `version` of the `mumcord-codec` and `mumcord-protocol` entries (and `mumcord-store`, kept in step) in the workspace `Cargo.toml` (it is the minor series, e.g. `0.4`), or publishing `mumcord-audio` and `mumcord-p2p` fails. Each crate is checked on its own: a version that is already on crates.io is skipped with a notice, since published versions can never be replaced.

To publish the current version of `main` without a new release, run the **Publish crates** workflow from the Actions tab on `main`. It needs the repository secret `CARGO_REGISTRY_TOKEN` (a crates.io API token). Check a package locally with `cargo publish -p <crate> --dry-run --locked` (for `mumcord-audio` this only works once the matching `mumcord-codec` is on crates.io, and for `mumcord-p2p` the matching `mumcord-protocol`). Each crate must stay self-contained: only crates.io dependencies and no files outside its own directory (the `.proto` files live in `crates/protocol/proto`).

## AUR package (Arch Linux)

The `mumcord-bin` package on the Arch User Repository repackages the release `.deb` (the Arch packages `webkit2gtk-4.1` and friends are used instead of the bundled copies an AppImage would carry). `.github/workflows/aur.yml` updates it automatically when you click **Publish release** (step 4); pre-releases are skipped. Without the secret below the job skips itself, so nothing breaks.

One-time setup:

1. Create an account on <https://aur.archlinux.org>.
2. Create a key pair just for this: `ssh-keygen -t ed25519 -f aur-key -C mumcord-ci -N ""`. Add `aur-key.pub` under **My Account > SSH Public Key** on the AUR.
3. Create the package once by hand: `git clone ssh://aur@aur.archlinux.org/mumcord-bin.git`, copy `packaging/aur/PKGBUILD` (with real version and checksums) and a `.SRCINFO` into it, commit and push.
4. In the GitHub repository add the secret `AUR_SSH_KEY` (Settings > Secrets and variables > Actions) with the content of the private key file `aur-key`. Then delete the local key files.

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
