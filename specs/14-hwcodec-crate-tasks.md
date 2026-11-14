# hwcodec crate · Task list

Spec: `specs/14-hwcodec-crate-specification.md` · Mode: team · Only the lead edits this file.

Locations: **HW** = `~/code/hwcodec` (new local git repository, no remote until G1) · **MC** = this Mumcord repository (branch based on `milestone/0.9.0`).

## Decisions made after the specification (user answers, 2026-11-14)

- **AMD test machine:** Linux with the AMD AMF package. Both VA-API and AMF can be tested by hand there; Windows stays untested for AMD.
- **Repository:** `Mumcord/hwcodec` is public from the start (no private stage). Creating it still needs the user's OK at that moment (G1).
- **Packages:** hwcodec is published as five crates.io packages: `hwcodec`, `hwcodec-libvpx-src`, `hwcodec-svt-av1-src`, `hwcodec-dav1d-src`, `hwcodec-libvpl-src`.
- **Windows builds of outside users (deviation from spec section 8, "no network access during a build"):** on Windows only, the build may download prebuilt codec libraries instead of building them from source. Linux and macOS keep building from bundled sources without network access.
- **Escape hatches (analyst decision):** prebuilt libraries through the existing variables `LIBVPX_STATIC_DIR`, `SVTAV1_STATIC_DIR`, `DAV1D_STATIC_DIR`, `HWENC_SDK_DIR`; a second opt-in hatch uses system libraries through pkg-config (used by Mumcord's Linux development builds and FreeBSD).
- **Mumcord dependency:** an uncommitted local path dependency on HW until 0.1.0 is published (G2), then the plain crates.io version. The engine is not published, so no published Mumcord crate is affected.

## Deviations found during the build

- **FR-14.17, fallback:** Mumcord's engine keeps its own loop over encoder candidates, because it mixes AV1 and VP9 candidates and passes per-open options ("Optimize for", camera mode). It calls the crate's encoders directly; the crate's `open_best_encoder` is not used by Mumcord. Detection, self-test and colour conversion do come from the crate.
- **FR-14.2, operating systems:** the spec says AMF and Quick Sync work "on Windows and Linux". In the existing code both are Windows only; on Linux, AMD and Intel cards are used through VA-API. The crate keeps that (no new Linux backends in phase 1) and the README says so.
- **Added to the crate for the engine:** `detect_encoder(codec, encoder) -> EncoderReport` (keeps the 3 s budget check per encoder) and `SelftestResult.average_frame_time`.

## Mobile builds (from A2, 2026-11-14)

- `milestone/0.9.0` now carries the Epic 5 engine preparation (pull request #824): phone builds use the engine with no codec at all (`--no-default-features`), later with `vp9` and `av1-decode`. The base solved this with switchable codecs and stand-in types inside `crates/screenshare`.
- Decision: no stand-in types in hwcodec (a switched-off codec is not part of the public crate). The engine gets a small module that opens the codecs and reports "this codec is not built into this app" when one is off. hwcodec only gains what is right for a public crate: it builds with any feature subset without bindgen, and a CI job proves that, including for the phone targets.
- The Mumcord pull request stays a draft until hwcodec 0.1.0 is on crates.io; hwcodec is published only after its reduced-feature pull request is merged.
- Points to agree with the mobile work: `specs/05-mobile-app-tasks.md` still names `mumcord-screenshare` features (their file); the `mumcord-mobile` repository pins the engine by git commit and will inherit the hwcodec dependency; its cross-build of codec libraries (Epic 5 task 7.1) must use hwcodec's `*_STATIC_DIR` variables.

## Build contract (from B5)

- Order per library: `LIBVPX_STATIC_DIR` / `SVTAV1_STATIC_DIR` / `DAV1D_STATIC_DIR` / `HWENC_SDK_DIR` (prebuilt, wins) → `HWCODEC_USE_SYSTEM_LIBS=1` (pkg-config) → bundled source build. Windows without the variables and cross-compiling stop with a message.
- Tools for the default build: C and C++ compiler, make, perl, nasm (x86 only), cmake, meson, ninja, libclang.
- Source crates in HW `sys/`: `hwcodec-libvpx-src` 2.55 MB, `hwcodec-svt-av1-src` 2.86 MB, `hwcodec-dav1d-src` 1.60 MB, `hwcodec-libvpl-src` 0.36 MB (limit 10 MB), versions `0.1.0+<upstream version>`.

## API contract (outline)

- Kept: `Codec`, `VideoEncoder`, `VideoDecoder`, `EncodedFrame`, `I420Frame`, `HardwareEncoder`, `Error`, `Result`, `open_hardware_encoder`, `Av1Encoder`, `Av1Decoder`.
- Renamed: `Encoder` → `Vp9Encoder`, `Decoder` → `Vp9Decoder`.
- Changed after B1: the existing `EncoderConfig` stays the settings type (no separate `EncoderSettings`); it is `#[non_exhaustive]`, built with `EncoderConfig::new(w, h, fps, kbps)` plus builder methods for temporal layers, threads and maximum size. `Av1Options::new(screen_content)`. `EncoderConfig` and `EncodedFrame` live in `src/codec.rs`. `HWENC_SDK_DIR` is only used for the static Intel dispatcher; headers come from `vendor/`.
- New in phase 1: `EncoderKind::{Hardware(..), Software}`, `detect_encoders(codec) -> Vec<EncoderReport>`, `EncoderReport {encoder, status, device, driver_version}`, `Unavailable {reason: UnavailableReason, detail}`, `selftest::measure_av1(..) -> SelftestResult`, `DEFAULT_PASS_PERCENT`, `default_order(codec)`, `open_best_encoder(codec, config, order)`, `open_software_decoder(codec)`, `convert::{bgra_to_i420(src, src_w, src_h, max_w, max_h) -> Option<I420Frame>, fit, encode_size, pack_rows}` (`bgra_to_i420` is the former engine `bgrx_to_i420`).
- New in phase 2: `Preset::{Live, Quality}`, `HardwareDecoder`, `detect_decoders`, `open_best_decoder`, `DecoderReport`.
- Cargo features, all on by default: `nvenc`, `amf`, `qsv`, `vaapi`, `svt-av1`, `libvpx`, `dav1d`.

## Settings contract (from B2)

- `EncoderConfig` fields `temporal_layers: Option<u8>`, `threads: Option<u32>`, `max_width`, `max_height`; methods `with_temporal_layers(u8)`, `with_threads(u32)`, `with_max_size(w, h)`. `new()` defaults (None, None, 7680x4320) reproduce today's behaviour.
- Limits, all `Error::InvalidConfig` at open and on `set_fps` / `set_resolution`: fps 1–120; picture within the maximum size plus encoder limits (VA-API 4096x2304). Layers: libvpx 1–3, SVT-AV1 1–3, hardware 1–4; automatic choice unchanged. Threads: libvpx 1–64, SVT-AV1 rejects an explicit count, hardware ignores it.
- To re-check in C1: SVT-AV1 with 1 and 2 layers against the bundled pinned version.

## Mumcord build contract (from B10)

- `HWCODEC_USE_SYSTEM_LIBS=1` is set once in MC `.cargo/config.toml` `[env]`; the four prebuilt-library variables are set where they were and still win. Never set the variable to `0` or empty. Keep hwcodec's default features in the engine dependency.
- The E2E container now builds and passes `HWENC_SDK_DIR` itself.
- Header versions in `scripts/build-hwenc-sdk.sh` (used for notices and SBOM) must be bumped together with hwcodec's `vendor/`.
- `THIRD_PARTY_NOTICES` is regenerated once hwcodec is in `Cargo.lock`.

## Detection contract (from B3)

- All names are re-exported at the crate root. `detect_encoders(Codec) -> Vec<EncoderReport>`; `EncoderReport { encoder, status, device: Option<String>, driver_version: Option<String> }` with `.works()`; `EncoderStatus::{Works, Unavailable { reason, detail }}`; `UnavailableReason::{NotAvailableOnThisOs, DriverNotFound, DriverTooOld, CodecNotSupported, OpeningFailed, TestEncodeFailed}`.
- `measure_av1(width, height, fps, Av1Options) -> Result<SelftestResult>` (feature `svt-av1`); `SelftestResult { budget_percent: f32, passed: bool }` with `.passes_at(percent)`; `DEFAULT_PASS_PERCENT: u32 = 50`.
- `EncoderKind::{Hardware(HardwareEncoder), Software}`; `default_order(Codec) -> Vec<EncoderKind>`; `open_best_encoder(Codec, &EncoderConfig, Option<&[EncoderKind]>) -> Result<(EncoderKind, Box<dyn VideoEncoder>)>`; `open_software_decoder(Codec) -> Result<Box<dyn VideoDecoder>>`; `Error::NotBuilt { what }`.
- Operating systems as the backends work today: NVENC on Windows and Linux (AV1 only); VA-API on Linux only (AV1 and VP9); AMF on Windows only (AV1 only); Quick Sync on Windows only (AV1 and VP9); macOS none. **The Linux AMD test machine therefore tests VA-API; AMF cannot be tested there.**
- The engine keeps as wrapper: store cache and cache ids, the 3 s probe budget, test hooks and forced results, GPU vendor lookup, "Optimize for" options, the viewer AV1 filter, recording the active encoder, the width-for-height table and the ms-per-frame conversion for the self-test.

## Windows libraries (user decision 2026-11-14)

- For developers who build hwcodec on Windows (x86_64, MSVC), the build downloads prebuilt codec libraries from a GitHub release `native-libs-<n>` in `Mumcord/hwcodec`, built by the repository's own workflow from the bundled sources, and verifies them against a SHA-256 pinned in the crate. Approved: creating `native-libs-1` once its test build is green.
- This is not a crate version and nothing on crates.io. Mumcord does not use it: Mumcord's installers keep the libraries built by Mumcord's own pipeline.
- Old `native-libs-*` releases must never be changed or deleted, because published crate versions point at them.
- **Changed later the same day (user request):** because the archive is only 6.8 MB, the libraries are also shipped inside a sixth crates.io package, `hwcodec-prebuilt-x86_64-pc-windows-msvc`, pulled in only on Windows x86_64 MSVC. Developers taking hwcodec from crates.io therefore need no download during the build (back in line with spec section 8). The verified download stays only as a fallback for builds straight from the git repository (opt-out `HWCODEC_NO_DOWNLOAD=1`). The binaries are not committed to git; the release pipeline fills the package from the checksum-verified `native-libs-<n>` archive. Publish order: four `-src` packages, the prebuilt package, `hwcodec`.
- Must be documented in the release description, README, RELEASING.md, CHANGELOG and at the pinned checksum in `build.rs`.

## User OK gates

- **G1** Create the public repository `Mumcord/hwcodec` (after C1 is green).
- **G2** Publish 0.1.0 of the six packages on crates.io (after C2 and C3 are green; the crates.io token as CI secret is a user step).
- **G3** Publish 0.2.0 (phase 2).

## Tasks

| ID | Title | Status | Owner | Depends on | Complexity | Covers |
| --- | --- | --- | --- | --- | --- | --- |
| A1 | Analysis & breakdown | done | a1-analyst | – | complex | whole spec |
| B1 | HW skeleton and move of `crates/screenshare` (rename, licences, lints, cargo features instead of `cfg(hwenc)`, bundled vendor headers, empty modules `detect selftest fallback convert`) | done | b1-skeleton | A1 | complex | FR-14.1, 14.2, 14.11 |
| B2 | Encoder settings (fps up to 120, temporal layers, threads, maximum size; reject at open) · HW `src/{codec,encoder,av1_svt}.rs`, `src/hw/{nvenc,amf,qsv}.rs`, `src/hw/vaapi/` | done | b2-settings | B1 | complex | FR-14.3 |
| B3 | Detection with reasons, fallback, self-test · HW `src/{detect,selftest,fallback}.rs`, `src/hw/{mod,dl}.rs`, `Error` in `lib.rs` | done | b3-detect | B1 | complex | FR-14.5–14.9 |
| B4 | Colour conversion · HW `src/convert.rs` | done | b4-convert | B1 | simple | FR-14.4 |
| B5 | Bundled sources and automatic build on Linux and macOS, missing-tool messages, escape hatches · HW `build.rs`, `sys/`, workspace section of `Cargo.toml` | done (source build of libvpx, SVT-AV1, dav1d not yet verified: meson and nasm missing on this machine; C1 runs it) | b5-autobuild | B1 | complex | Spec §8, AC-14.1, 14.2 |
| B6 | Windows build (prebuilt download, checksum-verified) · same files as B5 | done: CI green on Linux, macOS and three Windows jobs; `native-libs-1` release (6.8 MB); sixth package `hwcodec-prebuilt-x86_64-pc-windows-msvc` 6.82 MB; release rehearsal packs six; real GPUs on Windows untested | b6-windows | B11, B16 | complex | Spec §8, AC-14.1 |
| B7 | Examples and tests (detection report, encode/decode, mocked drivers, 120 fps single layer, custom order) · HW `examples/`, `tests/` | done (one test red on purpose until F1) | b7-examples | B2, B3, B4, B15 | complex | FR-14.10 |
| B8 | README, API documentation, CHANGELOG, CI files, licence check · HW `README.md`, `CHANGELOG.md`, `.github/`, `deny.toml` | done | b8-docs | B1 | simple | AC-14.6, Spec §10 |
| B9 | Mumcord engine uses hwcodec; `crates/screenshare` and moved engine code removed · MC `Cargo.toml`, `crates/engine/**` | done (engine tests 1096 passed; E2E not run yet) | b9-engine | B2, B3, B4 | complex | FR-14.17, 14.18 |
| B10 | Mumcord build, notices, CHANGELOG, roadmap row · MC `.github/actions/native-av1`, `ci.yml`, `licenses/native.json`, `scripts/generate-notices.sh`, `.cargo/config`, `CHANGELOG.md`, `specs/roadmap.md` | done (no CI or E2E run yet; C3 verifies) | b10-mc-build | B5 | complex | Spec §10, §14 |
| B15 | Structured reasons from the hardware backends (driver too old, device has no AV1/VP9, missing Quick Sync runtime) and device name / driver version in the detection report · HW `src/hw/{nvenc,amf,qsv}.rs`, `src/hw/vaapi/**`, `src/detect.rs`, `src/hw/mod.rs` | done (NVENC, AMF, Quick Sync mock-tested only; VA-API verified by a real run on the AMD machine) | b15-reasons | B2 | complex | FR-14.6 |
| F1 | Fix: VP9 software encoder ignores the bitrate with a single temporal layer (found by B7; failing test `tests/settings.rs`) · HW `src/encoder.rs` | done | f1-vp9-single-layer | B2 | complex | FR-14.3, AC-14.5 |
| F2 | Mumcord engine logs the detection result per encoder (reason and driver text from the crate's report) at `info`, as Spec 07 section 9 requires, instead of relying on the crate's own log lines · MC `crates/engine/src/screen_share/encoder/{probe,selftest}.rs` | done (hwcodec's own `log` lines are not bridged into Mumcord's log; the engine lines are the only source) | f2-probe-logging | B9 | simple | FR-14.18, Spec 07 §9 |
| B14 | Finalise README, CHANGELOG and `deny.toml` against the final API, env var names, tool list and the `detect` example; run `cargo deny` and `actionlint` | done (`ci.yml` not linted; its Windows job will fail until B6) | b14-docs-final | B5, B7 | simple | AC-14.6 |
| C1 | Verify crate locally: AC-14.2, 14.5, CI halves of 14.3 and 14.4, packaging dry run and size | done: AC-14.2 ✅, AC-14.5 ✅, AC-14.3 ✅ (also on the AMD card via VA-API), AC-14.4 ✅; bundled-source build ⏭ (meson, nasm missing; goes to CI in C2) | c1-qa-crate | B2–B8, B14, B15, F1 | simple | AC-14.2, 14.5, (14.3, 14.4) |
| F3 | Polish from C1: NVENC detail text repeats the library name; README code snippets as complete programs; licence metadata and licence files of the four `sys/` crates; stray `target` directory · HW `src/hw/nvenc.rs` or `src/hw/dl.rs` (text only), `README.md`, `sys/*/Cargo.toml`, `.gitignore` | done | f3-polish | C1 | simple | AC-14.6 |
| F4 | Fix: missed catch-all arm for hwcodec's open enums behind the `test-backend` feature (`crates/engine/src/screen_share/mod.rs`), found by C3; check all feature combinations and platform-specific code | done | f4-test-backend | B9 | simple | FR-14.17 |
| F5 | Cleanup from C3: self-test result logged twice; unused workspace dependencies `bindgen` and `pkg-config` in MC `Cargo.toml`; unrelated regenerated diff in `ui/src/bindings/commands.ts` restored | done | f5-cleanup | C3 | simple | FR-14.17 |
| B18 | E2E spec for the encoder status with all hardware encoders forced to fail and for "Check again" (new spec file, existing specs untouched) | done: `e2e/specs/media/encoder-fallback.e2e.ts`, 4/4 twice; AC-14.9 ✅, AC-14.11 ✅ | b18-e2e-encoder | F5 | complex | AC-14.9, AC-14.11 |
| G1 | User OK: create public repository | done (approved 2026-11-14: create, first commit, push) | user | C1 | – | – |
| B11 | Push with fresh history, CI green on Linux, Windows, macOS | done: https://github.com/Mumcord/hwcodec public, CI green on Linux (17 min) and macOS (5 min), bundled source build works on both; Windows follows with B6 | b11-repo-ci | G1 | complex | Spec §8, §10 |
| B16 | Repository automation for `Mumcord/hwcodec` (user request 2026-11-14): release pipeline on a version tag (checks, Windows prebuilt libraries as release files, crates.io publish of the five packages in order, GitHub release from the CHANGELOG; with a dry-run mode), Dependabot (cargo, GitHub Actions), Claude review workflow as in the Mumcord repository, security policy and templates as in Mumcord, branch protection is its own last step B17 | done: release pipeline (dry run green), Dependabot, Claude review with approval, templates, settings, CI 17 → 5.5 min; approval still comes from `github-actions[bot]` → F6 | b16-automation | B11 | complex | Spec §10 |
| F6 | Claude review approval must come from the Claude bot account, not `github-actions[bot]`; switch "Allow GitHub Actions to create and approve pull requests" off; follow-ups to the merged libloading 0.9 update (remove the Dependabot ignore, correct the stale comment in `Cargo.toml`) · HW `.github/**`, `Cargo.toml` comment | done: `claude[bot]` approves clean pull requests (mechanism of Mumcord `main`), no approval when `.github/` is touched (workflow-enforced), Actions approval setting OFF, libloading follow-ups pushed | f6-review-identity | B16 | complex | Spec §10 |
| B17 | Branch protection on `main` of `Mumcord/hwcodec` (pull request and green CI required, no force-push, one approval required, **approvals dismissed on new pushes**; user request 2026-11-14), switched on after B6. The approval of the Claude review must come from the Claude bot account, not from `github-actions[bot]` | done: rulesets for `main` (PR only, 1 approval, dismissed on push, up to date, 5 required checks, admin bypass \"pull request only\"), `v*` tags admin only, `native-libs-*` update/delete admin only, `release` environment `v*` tags only, dry run moved out of the environment; verified by experiment except the tag rule | b17-protection | B16, B6 | simple | Spec §10 |
| C2 | Verify crate in CI: AC-14.1, 14.6 | done: AC-14.6 ✅; AC-14.1 ⚠ (CI green on 5 jobs, but no job builds a fresh empty project; macOS source build not confirmed from the log; download path shown only indirectly); release path unprotected until B17; docs for the Windows libraries ✅ and checksum verified | c2-qa-repo | B11 | simple | AC-14.1, 14.6 |
| C3 | Verify Mumcord automatically (the only E2E agent): AC-14.7, 14.9, 14.10, 14.11 | done: AC-14.7 ✅, AC-14.10 ✅, logging ✅, E2E 24 spec files 115 passed / 0 failed / 7 skipped by design; AC-14.9 only partly (no spec uses the hook), AC-14.11 ⏭ (no E2E) → B18 | c3-qa-mumcord | B9, B10, F1, F2, B14 | complex | AC-14.7, 14.9–14.11 |
| C4 | Manual check on the AMD machine: AC-14.3, 14.4, 14.8, encode-time comparison | open | user + agent | C3 | simple | AC-14.3, 14.4, 14.8, Spec §9 |
| F7 | From C2, as a pull request on `Mumcord/hwcodec` (first one under branch protection): CI job per OS that builds a fresh empty project depending on hwcodec and runs an encode/decode round trip (AC-14.1), with log lines proving the source build on macOS and the real download on Windows; `release.yml` refuses a `v*` tag whose commit is not on `main`; README nit | done via pull requests #6, #7 and #9 (the first two were merged before their checks finished and left `main` red; #9 repaired it): fresh-project proof on Linux, macOS and both Windows library paths | f7-fresh-project | B17 | complex | AC-14.1, Spec §10 |
| C2b | Re-verify AC-14.1 after F7 is merged | open | | F7 | simple | AC-14.1 |
| B19 | Mumcord pull request before the publish (user OK 2026-11-14): dependency on `Mumcord/hwcodec` by git, pinned to a commit, so Mumcord's CI and E2E run on all platforms before the irreversible publish; notices regenerated; commits, push, pull request against `milestone/0.9.0`, kept unmerged until B13 switches it to the crates.io version | blocked: git dependency, notices and checks done in the worktree (uncommitted), but `milestone/0.9.0` moved by 69 commits (PR #824, mobile engine) and reworked `crates/screenshare` (per-codec features, stand-in types, `default-features = false`); needs A2 and B20 first. Also seen: engine test binary crashes at exit in about 1 of 6 runs (probe thread inside VA-API driver) → C8 | b19-mumcord-pr | C3, B18 | complex | FR-14.17, AC-14.7 |
| A2 | Analysis: how the mobile build features that landed on `milestone/0.9.0` (base commit `713e4a885`: `vp9`, `av1-decode`, `desktop-codecs`, stand-in types) map onto hwcodec; what must change in hwcodec and what in the engine; how to bring the uncommitted switch onto the new base | done | a2-analyst | B19 | complex | FR-14.17, FR-14.18 |
| B20a | Mumcord: back up the uncommitted switch as a patch outside the repository, commit it in the planned focused commits, rebase onto the new `origin/milestone/0.9.0` (27 of 36 overlapping files merge cleanly; the 9 others are files we delete), `cargo check --workspace` | done: 5 commits on top of `origin/milestone/0.9.0`, nothing pushed; backup in `~/.cache/mumcord-build/b20a-backup/`; open for B20b: feature forwards in `crates/engine/Cargo.toml`, lock file and notices to regenerate | b20a-rebase | A2 | complex | FR-14.17 |
| B20h | hwcodec pull request: CI job for reduced feature sets and the phone targets (no features, `libvpx` alone, `dav1d` alone, the desktop set; no features for Android and iOS), `bindgen` and `pkg-config` optional, the five plain items (two layer constants, `Av1Options`, `vp9_threads`, `av1_cpu_supported`) available in every build, README paragraph on cross-compiling | done, waiting for the maintainer's merge: https://github.com/Mumcord/hwcodec/pull/8 at `62ae904`, five checks green; the five plain items are at the crate root in every build; a no-feature build has no build dependencies; reduced sets and the Android and iOS targets are checked in CI (compile only) | b20h-features | A2 | complex | FR-14.1, Spec §8 |
| B20b | Mumcord on the new base: `hwcodec` with `default-features = false`, engine features forwarded (`vp9` → `libvpx`, `av1-decode` → `dav1d`, `desktop-codecs` → `svt-av1` + `nvenc` + `amf` + `qsv` + `vaapi`), new engine module `screen_share/codecs.rs` with the openers that report "not built" when a codec is off, 7 call sites, test gates; checks for no features and each feature alone, Android check, lock file and notices regenerated, desktop tests; then push and the draft pull request | in progress | b20b-engine | B20a, B20h | complex | FR-14.17, FR-14.18 |
| C8 | Find out whether the exit crash of the engine tests (probe thread still inside the VA-API driver at process exit) exists on the base without hwcodec; if yes, report as a separate bug | open | | – | simple | – |
| G2 | User OK: publish 0.1.0 | open | user | C2b, C3 | – | – |
| B12 | Publish and tag 0.1.0 | open | | G2 | simple | Spec §10 |
| B13 | Mumcord depends on the crates.io version (first state that can be committed) | open | | B12 | simple | FR-14.17 |
| P1 | Decoder interface, detection with built-in test stream, fallback, run-time failure report | open | | B12 | complex | FR-14.13, 14.14 |
| P2 | VA-API decoding | open | | P1 | complex | FR-14.12 |
| P3 | AMF decoding | open | | P1 | complex | FR-14.12 |
| P4 | NVIDIA decoding | open | | P1 | complex | FR-14.12 |
| P5 | Quick Sync decoding | open | | P1 | complex | FR-14.12 |
| P6 | Quality preset in all encoders, flush at end of stream | open | | B12 | complex | FR-14.15, 14.16 |
| P7 | Decode example, wrong-picture mock, quality measurement tool | open | | P2–P6 | complex | AC-14.12–14.14 |
| C5 | Verify crate phase 2: AC-14.13, CI halves of 14.12 and 14.14 | open | | P7 | simple | AC-14.12–14.14 |
| G3 | User OK: publish 0.2.0 | open | user | C5 | – | – |
| P8 | Publish and tag 0.2.0 | open | | G3 | simple | Spec §10 |
| P9 | Mumcord engine: setting, cached decoder results, engine decoding for share and camera, fail-over with keyframe request, debug hooks | open | | P8 | complex | FR-14.19, 14.21, 14.22 |
| P10 | Mumcord Settings UI with dark and light screenshots | open | | P9 | complex | FR-14.20 |
| C6 | Verify Mumcord phase 2 (E2E): AC-14.15, 14.17, 14.18, 14.19 | open | | P10 | complex | AC-14.15, 14.17–14.19 |
| C7 | Manual check on the AMD machine: AC-14.12, 14.14, 14.16 and CPU / dropped-picture measurement | open | user + agent | C6 | simple | AC-14.12, 14.14, 14.16, Spec §9 |
| D1 | Implementation report | open | | all | simple | – |
