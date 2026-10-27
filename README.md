<p align="center">
  <img src="assets/brand/mumcord-logo.png" alt="Mumcord logo" width="160" />
</p>

# Mumcord

Mumcord is a modern desktop voice chat client for gaming groups and online communities. It is built with Rust, Tauri 2 and React/TypeScript.

Website: **[mumcord.app](https://mumcord.app)** · Wiki: [github.com/ToBee94/mumcord/wiki](https://github.com/ToBee94/mumcord/wiki) · Website source: [mumcord-www](https://github.com/ToBee94/mumcord-www)

Mumcord is **compatible with Mumble**. It is **not affiliated with or endorsed by the Mumble project**. "Mumble" is the name of that project; Mumcord does not use the Mumble logo.

![Mumcord main window: channel tree, chat and member list (dark theme, demo data)](docs/images/mumcord-main.png)

## What it is

- A pure client: it speaks the Mumble protocol and works with unmodified, standard Mumble servers. Nothing needs to be installed or changed on the server.
- Mumble servers 1.4 and newer are fully supported. Older servers are supported on a best-effort basis; features they lack are hidden.

## Features

- **Voice chat for Mumble servers**: push-to-talk or voice activation, noise suppression, music quality, per-person volume, positional audio with the official game plugins (Beta).
- **Several servers at once** (0.4.0): up to 5 connected servers (configurable up to 15), voice on one, chat, private messages, notifications and badges on all of them; auto-connect at start.
- **Chat**: images, link previews and player cards, replies and quotes, history sync between Mumcord users, full-size images on demand (Beta), channel passwords, a rich-text comment editor.
- **Themes** (0.4.0): presets and your own colour themes, each with a dark and a light version, shareable as a code and optionally set per server.
- **Watch party** (0.4.0, Beta): watch YouTube together with the Mumcord users in your voice channel, with a shared playlist.
- **Screen sharing** (0.4.0, Beta): share a window or screen with up to 8 Mumcord users in your channel at up to 720p30, over a direct connection (see [Screen sharing](#screen-sharing-beta) for what each OS supports).
- **Friends, now playing and now gaming**, connection info (ping, loss, bandwidth) and a weak-connection icon.
- **In-game overlay** that shows who is talking: a Vulkan layer on Linux (with an OpenGL launch option), a click-through window over borderless games on Windows (0.4.0). No overlay on macOS.
- **Linux packages** (0.4.0): `.deb`, `.rpm`, AUR `mumcord-bin` and AppImage for x86_64 and ARM64, a one-line installer, app-menu entry, `mumble://` links and start at login.
- All 24 official EU languages, keyboard operation, screen-reader labels and system theme.

## Status

Early development. Release 0.4.0 added several servers at once, themes, watch party and screen sharing; expect rough edges, especially in the Beta features. See [CHANGELOG.md](CHANGELOG.md) for what each release added.

From 0.4.1, the macOS builds are signed and notarized. Windows builds are still unsigned until code signing is set up, so Windows shows a security warning when you start them.

**macOS Local Network permission:** macOS 15 and later asks whether Mumcord may find devices on your local network. Allow it, or connections to servers in your home network fail with "No route to host". You can change it later in System Settings → Privacy & Security → Local Network.

**Log files** (useful for bug reports; they contain no chat, names or audio): macOS `~/Library/Logs/app.mumcord.client/`, Linux `~/.local/share/app.mumcord.client/logs/`.

## Download

Installers for Windows (`.exe`), macOS (`.dmg`, Apple Silicon and Intel) and Linux (`.deb`, `.rpm` and `.AppImage`, each for x86_64 and ARM64) are on the [Releases page](https://github.com/ToBee94/mumcord/releases), each with a `SHA256SUMS` file to check the download. Each release's notes say whether its Windows and macOS builds are signed; unsigned builds show a security warning on first start, and the notes explain how to open the app anyway.

On Linux, one line installs Mumcord with your package manager (or the AppImage on other distributions); add `-s -- --uninstall` after `sh` to remove it:

```sh
curl -fsSL https://github.com/ToBee94/mumcord/releases/latest/download/install.sh | sh
```

On Arch Linux and derivatives, Mumcord is also in the AUR as `mumcord-bin`.

See the [changelog](CHANGELOG.md) for what changed in each release.

### Linux: GStreamer plugins for watch parties

The `.deb`, `.rpm` and AUR packages install the GStreamer plugins that WebKitGTK needs to play YouTube videos in watch parties. The AppImage bundles the GStreamer plugins it needs and plays YouTube via VP9. It deliberately does not bundle H.264 (gst-libav), so videos that YouTube only serves as H.264 may not play there. The AppImage also loads the GStreamer plugins installed on the system, after its own, so installing gst-libav adds H.264 there too; the player shows a hint when a codec is missing. For a distribution without the packages above, or if you want H.264 with the AppImage, install the plugins yourself:

- Debian, Ubuntu: `sudo apt install gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav`
- Fedora: `sudo dnf install gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-libav` (`gstreamer1-libav` may need [RPM Fusion](https://rpmfusion.org/))
- Arch, CachyOS, Manjaro: `sudo pacman -S gst-plugins-good gst-plugins-bad gst-libav`
- openSUSE: `sudo zypper install gstreamer-plugins-good gstreamer-plugins-bad gstreamer-plugins-libav`

### Linux: flickering or slow rendering

Mumcord sets `WEBKIT_DISABLE_COMPOSITING_MODE=1` at startup because WebKitGTK 2.54 flickers with the watch-party video otherwise. To keep WebKit's default compositing (for example if your WebKitGTK version renders better that way), start Mumcord with `MUMCORD_WEBKIT_COMPOSITING=1`. A `WEBKIT_DISABLE_COMPOSITING_MODE` you set yourself is never overwritten.

### Screen sharing (Beta)

Screen sharing needs a direct connection between the person sharing and each viewer (allowed once in a dialog, changeable in _Settings → Privacy_). Mumcord uses no outside server to set it up, so it works when one side has IPv6 or a home router that allows port mapping (UPnP, NAT-PMP or PCP), or when both are on the same network. Behind carrier-grade NAT or two routers without port mapping, that pair cannot watch; Mumcord says so after a short wait.

|                    | Picture                                                                                                                                                                 | Sound                                                                                      |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| **Windows**        | Windows 10 version 1903 or newer; the system picker offers windows and screens                                                                                          | Only the shared app's sound, Windows 10 version 2004 or newer; no sound for a whole screen |
| **macOS**          | macOS 14 or newer with the system picker (window, app or screen); macOS 12.3 and 13 share the whole main screen; needs the "Screen & System Audio Recording" permission | Only the shared content's sound, never Mumcord's own; macOS 13 or newer                    |
| **Linux, Wayland** | The desktop's picker (xdg-desktop-portal with PipeWire) chooses a window or screen                                                                                      | No sound                                                                                   |
| **Linux, X11**     | Whole screen only                                                                                                                                                       | No sound                                                                                   |

Up to 8 people can watch one share; the quality drops automatically to 480p at 15 frames per second when the upload is too slow.

## Code signing policy

Windows releases are signed with Azure Artifact Signing once the setup is active. What is signed, and who builds, reviews and publishes signed releases, is described in the [code signing policy](CODE_SIGNING.md).

## Privacy

Mumcord sends no telemetry. Every network connection it makes is listed in the [privacy policy](PRIVACY.md).

Maintainers: how to cut a release is described in [RELEASING.md](RELEASING.md).

## Building from source

### Prerequisites

- Rust, stable toolchain, 1.90 or newer (`rustup`), with the `rustfmt` and `clippy` components.
- Node.js 24 (the version CI uses) and npm.
- No `protoc` is needed: the protobuf compiler is vendored (`protoc-bin-vendored`).
- Linux, Debian/Ubuntu (24.04 in CI):

      sudo apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
        libxdo-dev libasound2-dev libvpx-dev libpipewire-0.3-dev libclang-dev \
        cmake patchelf xdg-utils build-essential pkg-config libssl-dev

- Linux, Arch/CachyOS:

      sudo pacman -S --needed base-devel webkit2gtk-4.1 libayatana-appindicator librsvg \
        xdotool alsa-lib libvpx libpipewire clang cmake openssl pkgconf patchelf xdg-utils

- Windows: Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC), and the WebView2 runtime (preinstalled on Windows 11).
- macOS: Xcode command line tools (`xcode-select --install`).
- Windows and macOS link a static libvpx for screen sharing: build it with `scripts/build-libvpx.sh <dir>` (Windows: vcpkg and LLVM for bindgen; macOS: `brew install nasm`) and set `LIBVPX_STATIC_DIR=<dir>` before building. Linux release packages do the same so they run on older distributions; Linux development builds use the system libvpx. Details: [RELEASING.md](RELEASING.md#static-libvpx).

### Get the code and run

    git clone <repository-url> mumcord && cd mumcord
    npm ci
    npm run tauri dev

### Build release packages

    npm run tauri build

Bundles land in `target/release/bundle/`: `appimage/*.AppImage` (Linux), `nsis/*-setup.exe` (Windows), `dmg/*.dmg` and `macos/*.app` (macOS). Each OS builds only its own formats. To build a single format, pass `-- --bundles appimage` (or `nsis`, `dmg`). The packages are unsigned; Windows and macOS show a security warning on first start.

### Checks

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
    npm run lint
    npm run typecheck
    npm test

`npm run build` type-checks and builds the frontend only.

### Optional

- Manual test checklists: [release 0.4.0](docs/manual-checklist-0.4.0.md), [screen sharing on Windows](docs/screen-share-windows.md), [screen sharing on macOS](docs/screen-share-macos.md); measurements in [docs/screen-share-performance.md](docs/screen-share-performance.md); known problems in [docs/troubleshooting.md](docs/troubleshooting.md).
- Local Mumble servers for manual and automated tests: [`testenv/README.md`](testenv/README.md). Do not test against public servers.
- UI end-to-end tests (`npm run e2e`, needs Docker): [`e2e/README.md`](e2e/README.md).
- After dependency changes, regenerate the third-party notices with `scripts/generate-notices.sh`.

## Licenses

Mumcord is licensed under the [GNU AGPL v3 only](LICENSE) from version 0.2.0. Contributions require agreeing to the CLA described in [CONTRIBUTING.md](CONTRIBUTING.md). Versions up to and including 0.1.1 were released under the MIT License and stay MIT for those who have them.

Third-party licenses, including Mumble's BSD license for the protocol definitions in `proto/`, are listed in [`THIRD_PARTY_NOTICES`](THIRD_PARTY_NOTICES). Regenerate it with `scripts/generate-notices.sh` when dependencies change.
