# Raspberry Pi development and testing

Use a 64-bit Raspberry Pi OS desktop installation (`uname -m` must say `aarch64`).
Mumcord uses the existing Linux/ARM64 client; it does not need a separate protocol, UI or
server configuration.

## Keep the Pi responsive

On an 8 GB Pi 4, start with **one build or test worker**, not one per CPU. Run builds and tests
sequentially. Reserve at least 2 GB of available RAM for the desktop, and monitor
`MemAvailable` in `/proc/meminfo`, not just the `free` column of `free`.

Do not rely on `systemd-run -p MemoryMax=… -p MemorySwapMax=0` without first checking
`/sys/fs/cgroup/cgroup.controllers`. Raspberry Pi OS can boot with `cgroup_disable=memory`:
the properties are accepted, but **cannot enforce a memory limit** without the `memory`
controller. Use an external watchdog to stop the whole build process group if available RAM
falls below the reserve. This is a safety check, not a hard allocation limit.

For a no-swap run, check `/proc/swaps` before starting and while testing. An unused zram swap
device is still swap. If temporarily disabling it, stop its systemd swap unit too; otherwise
systemd can reactivate it after `swapoff`. Record and restore the original state when finished.
Do not permanently change the Pi's boot configuration or swap policy just to build Mumcord.

The SD card needs room for the checkout, dependencies and Rust artifacts. Keep temporary
native builds on disk (`TMPDIR`), not the RAM-backed `/tmp`. Use a dedicated target directory
and remove it after the session:

```sh
export CARGO_BUILD_JOBS=1
export CMAKE_BUILD_PARALLEL_LEVEL=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TARGET_DIR="$HOME/.cache/mumcord-build/rpi-session"
export TMPDIR="$HOME/.cache/mumcord-rpi-tmp"
mkdir -p "$CARGO_TARGET_DIR" "$TMPDIR"
```

`CARGO_BUILD_JOBS` also limits the libvpx and dav1d build scripts;
`CMAKE_BUILD_PARALLEL_LEVEL` limits the SVT-AV1 and hardware encoder SDK scripts. Without
these overrides the scripts retain their existing parallelism on all platforms.

## Dependencies and build

Use the Rust version required by `Cargo.toml` or newer and Node.js 24, as in CI. Debian 13's
packaged Node.js 20 is too old for the current frontend toolchain.

Install the Linux development dependencies listed in `.github/workflows/build-and-smoke.yml`
(`Install Linux system libraries`). Raspberry Pi OS/Debian 13 calls its SVT-AV1 development
package `libsvtav1enc-dev`, but that package does not provide the SVT-AV1 4 API Mumcord needs.
The Intel oneVPL development package is not available for ARM64 there either. Do not substitute
an old SVT-AV1 or drop codecs from the desktop app to make the build pass. Build the pinned
native libraries:

```sh
native="$HOME/.cache/mumcord-rpi-native"
scripts/build-libvpx.sh "$native/libvpx"
scripts/build-svt-av1.sh "$native/svt-av1"
scripts/build-dav1d.sh "$native/dav1d"
scripts/build-hwenc-sdk.sh "$native/hwenc"
export LIBVPX_STATIC_DIR="$native/libvpx"
export SVTAV1_STATIC_DIR="$native/svt-av1"
export DAV1D_STATIC_DIR="$native/dav1d"
export HWENC_SDK_DIR="$native/hwenc"

npm ci
npm run build
cargo build -p mumcord --features custom-protocol --locked
```

`custom-protocol` embeds the built frontend. Without it, a plain Cargo build waits for the
Vite development server instead of acting as a standalone client. This is a debug build for
functional checks, **not** a release-performance benchmark.

For repeated iterations, build the frontend on a faster machine and copy `dist/` to the Pi
before compiling the app. Rust cross-compilation needs an ARM64 C/C++ toolchain, the Pi's
headers and libraries as a sysroot, target-aware pkg-config/bindgen settings, and ARM64 builds
of the pinned native codec libraries. Merely setting `--target aarch64-unknown-linux-gnu`
is not enough. Run the resulting ARM64 executables on the Pi; a successful cross-build alone
does not verify its drivers, desktop integration or audio.

## Checks

```sh
bash scripts/ci/test-native-build-jobs.sh
npm run typecheck
npm run lint
npm test -- --maxWorkers=1
cargo test --workspace --locked -- --test-threads=1
```

Some Rust integration tests require the local test environment; follow their documented setup
in `testenv/`. Never connect to a public Mumble server for testing. Run the app in the Pi's
actual desktop session, not just over a headless SSH session. Check startup, the first-run
wizard, reconnect, voice, tray behavior and window resizing against a local test server.
Record which checks actually ran and distinguish failures from checks that were not performed.

For performance comparisons use a release build, identical settings and the same workload.
Record the Pi model, OS and WebKitGTK versions, CPU/RAM usage, and `vcgencmd get_throttled`.
Do not globally disable GPU acceleration or change other operating systems' defaults to work
around a Pi-specific problem without reproducing it first.
