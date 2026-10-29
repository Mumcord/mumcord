# Releasing Mumcord

Releases are built by GitHub Actions (`.github/workflows/release.yml`). Pushing a version tag such as `v0.1.0` that points to a commit on `main` builds the Windows installer (`.exe`), the macOS disk image (`.dmg`, universal for Apple Silicon and Intel) and the Linux AppImage, smoke-tests each one against a local Mumble server, and creates a **draft** GitHub Release with the files and a `SHA256SUMS` checksum file. Nothing becomes public until you publish the draft.

Builds are unsigned until code signing is set up (Windows via Azure Artifact Signing, macOS via an Apple Developer ID: see [docs/release-signing.md](docs/release-signing.md) and the [code signing policy](CODE_SIGNING.md)). The release notes say per platform whether the build is signed and explain the SmartScreen and Gatekeeper warnings for unsigned ones.

## Versions

The version is set at build time from git; nobody bumps it in the manifests by hand.

| Build                                | Version shown in the app (About, diagnostics, log, server) | Package version of the bundles   |
| ------------------------------------ | ---------------------------------------------------------- | -------------------------------- |
| Release tag `vX.Y.Z` (`release.yml`) | `X.Y.Z`                                                    | `X.Y.Z`                          |
| CI build of `main` or a pull request | `dev~<short commit>`, e.g. `dev~a1b2c3d`                   | test builds: the latest `v*` tag |
| Local build                          | `dev~YYYYMMDD`, plus `-dirty` with uncommitted changes     | the version in `Cargo.toml`      |

- **Display version.** `src-tauri/build.rs` reads `MUMCORD_VERSION` (set by CI: the tag's version in releases, `dev~<short commit>` in `ci.yml`, `e2e.yml` and test builds). Without it, it uses the build date and asks git whether the tree has uncommitted changes; without git (for example a source tarball) it leaves out `-dirty` instead of failing. The rules are in `src-tauri/build/version.rs`. Dev builds never look for updates, since they are newer than the release they are based on. Theme share codes keep the numeric package version, because other clients reject a version they cannot parse.
- **Package version.** Installer formats need a plain number (RPM rejects `-`, MSI/NSIS want `X.Y.Z`, macOS bundle versions are numeric), so dev bundles carry the version of the latest `v*` tag (`scripts/latest-release-version.sh`) and only the app's display version says `dev~…`.
- **Setting it.** `scripts/set-version.sh X.Y.Z` writes the version into `package.json`, `package-lock.json`, `[workspace.package]` in `Cargo.toml` (every crate inherits it), the requirements of the internal crates crates.io needs (`version = "X.Y"`), `Cargo.lock` and `scripts/smoke/seed/Cargo.lock`. CI runs it on a fresh checkout and never commits the result; `scripts/ci/test-set-version.sh` checks it on every CI run. `src-tauri/tauri.conf.json` has no version on purpose: Tauri takes the app version from `Cargo.toml`. Do not add one.

Between a release and the merge of its sync pull request (step 4), the manifests in the repository keep the version of an older release; that is expected. Dev builds are not affected: their display version comes from git and their package version from the latest `v*` tag, never from the manifests.

## 1. Check the changelog

No release pull request is needed. Make sure `CHANGELOG.md` on `main` has a section for the version, headed `## [0.4.3] - Unreleased` (a plain `## [Unreleased]` section works too: it is renamed after the release). Its text becomes the "What's new" part of the release notes, so write it for users. Do not put a date in the heading; the sync pull request (step 4) adds the tag's date.

Work through the manual checks of the release, for 0.4.0 [docs/manual-checklist-0.4.0.md](docs/manual-checklist-0.4.0.md).

## 2. Tag the release on `main`

```sh
git switch main
git pull
git tag -a v0.4.3 -m "Mumcord 0.4.3"
git push origin v0.4.3
```

The tag must be `v` plus a version (`X.Y.Z`, optionally with a pre-release suffix such as `-rc.1`) and must point to a commit on `main`. The tag alone decides the version.

## 3. Wait for the workflow

Open the **Actions** tab and follow the **Release** run for the tag. It takes roughly as long as the Test builds workflow (up to about an hour). It runs:

1. **Check tag**: the tag is a version, the commit is on `main`, and a dry run of `scripts/set-version.sh` works.
2. **Generate third-party notices** from the tagged commit (see [Third-party notices](#third-party-notices)); the builds bundle them. It fails when a dependency has a licence that is not allowed.
3. **Build** on Linux, Windows and macOS. Each build job first sets the tag's version in all manifests and lock files, so the bundles, the app's About page and the diagnostics all say `X.Y.Z`. With signing set up, the Windows app and installer and the macOS app are signed automatically; nothing needs your approval.
4. **Smoke** test: each build is installed on a fresh machine and connects to a local test server.
5. **Generate SBOMs**: one software bill of materials per package type (see [SBOM](#sbom)).
6. **Create draft release**: attaches the files, the SBOMs and `SHA256SUMS`, attests their build provenance and writes the release notes (the download section, then the version's `CHANGELOG.md` section as "What's new", then the change list GitHub generated).
7. **Publish crates**: publishes the library crates with the tag's version (see below).

## 4. Review and publish

1. Open **Releases** on GitHub; the new release is marked **Draft**.
2. Check the notes: the fixed download and signing section, the "What's new" text from the changelog, followed by the change list GitHub generated from merged pull requests. Edit the text if needed.
3. Check that the `.exe`, `.dmg`, `.AppImage`, `.cdx.json` and `SHA256SUMS` files are attached. Optionally download one and try it.
4. Click **Publish release**.
5. Publishing starts `.github/workflows/post-release.yml`, which opens the pull request "chore: sync version X.Y.Z and third-party notices" (branch `chore/sync-version-X.Y.Z`) with the released version in the manifests and lock files, the regenerated notices and the finalised `CHANGELOG.md` (the version's heading gets the tag's date, a link is added at the bottom, and an empty `## [Unreleased]` section is added on top; a section that is already dated is left alone, and running it again changes nothing), and starts CI and E2E on it. Merge it once the checks are green. It runs on publishing, not when the build finishes, so a draft you discard changes nothing; pre-releases (`-rc.1` and the like) are not synced. If it did not run, start it by hand: **Actions → Post-release sync → Run workflow** with the version. Opening the pull request needs **Settings → Actions → General → Allow GitHub Actions to create and approve pull requests**; without it the run pushes the branch and shows a warning with a link to open the pull request by hand. While the sync pull request is open, `notices.yml` does nothing (the sync contains fresh notices) and closes nothing; the sync closes an open notices pull request.

## Release integrity

A published release must never change. Its fingerprint is `releases/<version>.json`: the tag and the commit it points to, every release asset with name, size and SHA-256 (the digest GitHub stores on upload; this covers `SHA256SUMS` and the SBOMs, and `sha256sums` repeats the hash of `SHA256SUMS`), the text and SHA-256 of the version's `CHANGELOG.md` section, and for each library crate the checksum and yanked flag crates.io reports for that version (`null` = not published for that version, as for the older releases).

- **Recording.** The post-release sync (step 4) builds the manifest with `scripts/release-manifest.sh vX.Y.Z` and commits it in the same pull request; an existing manifest is never rewritten, and the published release itself is not touched (no manifest asset is uploaded). By hand: `scripts/release-manifest.sh vX.Y.Z > releases/X.Y.Z.json` (needs `gh`, `curl`, `jq`).
- **Checking.** `.github/workflows/release-integrity.yml` runs daily and on demand (**Actions → Release integrity → Run workflow**). For every manifest it runs `scripts/release-manifest.sh --verify releases/X.Y.Z.json`, which compares the tag's commit, the assets and the crates.io data with the manifest. On a difference the run fails and opens the issue "Release integrity: vX.Y.Z changed" with the diff (an open issue gets a comment instead of a duplicate). It uses only the workflow token (`contents: read`, `issues: write`).
- **Changelog.** A released version's `CHANGELOG.md` section (from its dated heading to the next heading) is part of the release. The daily check compares the section on `main` with the manifest, and the CI check **Released changelog sections** (`scripts/ci/check-changelog-released.sh`, on every pull request) fails when a pull request changes a section whose heading already has a date. Only `## [Unreleased]` and `## [X.Y.Z] - Unreleased` may change; dating a section for the first time, as the post-release sync does, is allowed because the base still says "Unreleased". A mistake in a released entry is corrected by a note in the next version's section, not by editing the old one.
- **If it fires.** Never fix a published release in place (no re-uploaded asset, no moved tag): find out what changed and who did it, then publish a patch version. A crate can only be yanked, never replaced; yanking is also reported.
- **Prevention.** The ruleset "Protect release tags" blocks creating, updating and deleting `v*` tags; only the repository role Admin can bypass it (always), so an admin can still move a tag. Assets are not protected by it. GitHub's **immutable releases** lock a release's assets and tag once published and are available for this repository (the setting exists and is currently off): switch it on under **Settings → General → Releases → Enable release immutability** (or the organisation's settings for all repositories). Because it applies to releases published afterwards, and a draft stays editable until you publish it, the normal flow (draft, review, publish) keeps working; it makes the daily check a second line of defence for already published releases.

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

`THIRD_PARTY_NOTICES`, `ui/public/third-party-notices.txt` and `ui/public/third-party-notices.json` (the same list as structured data, shown in the app under About → Third-party licences) are generated by `scripts/generate-notices.sh`, which CI runs; nobody has to run it by hand:

- **Release**: the Release workflow regenerates both files from the tagged commit before the builds, and every bundle contains that version, so the shipped list is always correct even if the committed files are stale.
- **Main**: `.github/workflows/notices.yml` runs on every push to `main` that changes `Cargo.lock`, `package-lock.json`, `about.toml`, `licenses/` (with `licenses/native.json`, the components that are not Rust or npm packages), `scripts/notices.js` or the script (and by hand via **Run workflow**). When the files change, it opens or updates the pull request "chore: regenerate third-party notices" from the branch `chore/regenerate-notices` and starts CI and E2E on it, because a pull request opened by a workflow starts no checks by itself. Review and merge it like any other pull request. Opening the pull request needs **Settings → Actions → General → Allow GitHub Actions to create and approve pull requests**; without it the run pushes the branch and shows a warning with a link to open the pull request by hand. When `main` is up to date again, the workflow closes an open notices pull request.

**Licence policy**: the script fails when a dependency uses a licence that is not allowed: Rust crates must match `accepted` in `about.toml`, npm production packages `npm_allowed` in the script. The error names the package and its licence. Replace the dependency, or add the licence to the list only after checking that it is compatible with the AGPL-3.0.

The tool versions are pinned in `scripts/license-tools.env`; CI installs them from there and caches them. To run the script locally, install the same cargo-about (`cargo install cargo-about --version <version> --locked --features cli`) and run `npm ci` first. The "native libraries" section (libvpx, PipeWire, GStreamer, WebKitGTK, source offer) is written in the script; update it when bundled native libraries change.

## SBOM

Every release has one CycloneDX 1.5 JSON SBOM per package type whose contents differ, named `mumcord-<version>-<platform>.cdx.json`: `appimage-x86_64`, `appimage-aarch64`, `deb-rpm-x86_64`, `deb-rpm-aarch64` (the `.deb` and `.rpm` have the same contents), `windows-x86_64` and `macos-universal`. They are listed in `SHA256SUMS` and covered by the build provenance attestation (`gh attestation verify <file> --repo Mumcord/mumcord`).

Each SBOM lists the Rust crates compiled for that platform (`cargo-cyclonedx`, without build and dev dependencies; on Linux also the in-game overlay layer), the npm production packages of the UI (`@cyclonedx/cyclonedx-npm` from `package-lock.json`), libopus as vendored by the `opusic-sys` crate, libvpx (the version in `scripts/build-libvpx.sh`, on Windows the runner's vcpkg version) and, for the AppImage, WebKitGTK and GStreamer with the versions of the build host and every plugin kept by `scripts/filter-gst-plugins.sh`. The build jobs record the native libraries with `scripts/sbom-native.sh`; the **Generate SBOMs** job runs `scripts/generate-sbom.sh`, which calls the tools and merges their output with `scripts/sbom-merge.js`. System components the packages only load from the host (WebKitGTK for `.deb`/`.rpm`, WebView2 on Windows, WebKit on macOS) and the other libraries linuxdeploy copies into the AppImage are not listed.

To try it locally (cargo-cyclonedx from `scripts/license-tools.env`): write a native list with `scripts/sbom-native.sh > native/sbom-native-linux.txt` (Linux needs `GSTREAMER_PLUGINS_DIR=$(scripts/filter-gst-plugins.sh)`), then run `PLATFORMS="appimage-x86_64 deb-rpm-x86_64" scripts/generate-sbom.sh 0.0.0 native out`. `cyclonedx validate --input-file <file> --input-version v1_5` (cyclonedx-cli) checks the result.

## Publishing the library crates (crates.io)

The library crates `mumcord-protocol` (`crates/protocol`), `mumcord-codec` (`crates/codec`), `mumcord-store` (`crates/store`), `mumcord-audio` (`crates/audio`) and `mumcord-p2p` (`crates/p2p`) are published on crates.io with the app version. Protocol, codec and store are independent; audio depends on codec and p2p on protocol, so those two wait for them. The **Release** workflow does it automatically in its last job, after the draft release exists; tags with a pre-release suffix (such as `-rc.1`) are skipped. Each job of `publish-crates.yml` first runs `scripts/set-version.sh` with that version, which also sets the requirements of the internal crates (`mumcord-codec`, `mumcord-protocol`, `mumcord-store`) to the minor series, e.g. `0.4`, so `mumcord-audio` and `mumcord-p2p` find their dependencies on crates.io. Each crate is checked on its own: a version that is already on crates.io is skipped with a notice, since published versions can never be replaced.

To publish a release's crates without a new release, run the **Publish crates** workflow from the Actions tab and pick the release tag under "Use workflow from"; it publishes that tag's version. Left empty on `main`, the version input falls back to the latest release tag, and the crates are built from `main`'s code. It needs the repository secret `CARGO_REGISTRY_TOKEN` (a crates.io API token). Check a package locally with `cargo publish -p <crate> --dry-run --locked` (for `mumcord-audio` this only works once the matching `mumcord-codec` is on crates.io, and for `mumcord-p2p` the matching `mumcord-protocol`). Each crate must stay self-contained: only crates.io dependencies and no files outside its own directory (the `.proto` files live in `crates/protocol/proto`).

## AUR package (Arch Linux)

The `mumcord-bin` package on the Arch User Repository repackages the release `.deb` (the Arch packages `webkit2gtk-4.1` and friends are used instead of the bundled copies an AppImage would carry). `.github/workflows/aur.yml` updates it automatically when you click **Publish release** (step 4); pre-releases are skipped. Without the secret below the job skips itself, so nothing breaks.

One-time setup:

1. Create an account on <https://aur.archlinux.org>.
2. Create a key pair just for this: `ssh-keygen -t ed25519 -f aur-key -C mumcord-ci -N ""`. Add `aur-key.pub` under **My Account > SSH Public Key** on the AUR.
3. Create the package once by hand: `git clone ssh://aur@aur.archlinux.org/mumcord-bin.git`, copy `packaging/aur/PKGBUILD` (with real version and checksums) and a `.SRCINFO` into it, commit and push.
4. In the GitHub repository add the secret `AUR_SSH_KEY` (Settings > Secrets and variables > Actions) with the content of the private key file `aur-key`. Then delete the local key files.

## If the workflow fails

Nothing has been published, so it is safe to start over:

1. Read the failing step in the Actions run. A failed **Check tag** step says whether the tag is not a version, the commit is not on `main`, or the version could not be set.
2. If a draft release was already created, delete it on the Releases page.
3. Delete the tag, locally and on GitHub:

   ```sh
   git tag -d v0.4.3
   git push origin :refs/tags/v0.4.3
   ```

4. Fix the problem with a normal pull request into `main`.
5. Tag the fixed `main` again (step 2).

Once a release is published, do not move or reuse its tag. Fix problems with a new version instead (for example `v0.4.4`).
