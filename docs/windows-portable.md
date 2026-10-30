# Portable Mumcord for Windows

Every release ships `Mumcord_<version>_x64-portable.zip` next to the installer (`Mumcord_<version>_x64-setup.exe`). It runs without installing: from a USB stick, on a PC without administrator rights, or for a quick try (issue #181).

## What the zip contains

| File                                       | Purpose                                                              |
| ------------------------------------------ | -------------------------------------------------------------------- |
| `Mumcord.exe`                              | The same program the installer installs (signed when the release is) |
| `portable`                                 | Empty marker file that switches portable mode on                     |
| `README-portable.txt`                      | Short user guide (`packaging/windows/README-portable.txt`)           |
| `LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES` | Mumcord's licence and the third-party components with their licences |

There is no single-file `-portable.exe` download: a lone exe cannot carry the marker file, so it would quietly use the normal profile folders, and it would ship without the licence texts.

## Portable mode

When a file named `portable` sits next to `Mumcord.exe`, Mumcord keeps everything in `MumcordData\` beside the exe instead of `%APPDATA%` and `%LOCALAPPDATA%`:

| Folder              | Contents                                                                   |
| ------------------- | -------------------------------------------------------------------------- |
| `MumcordData\data`  | The database (servers, settings, chat history), window sizes, game plugins |
| `MumcordData\local` | The WebView2 data (`EBWebView`) and the caches                             |
| `MumcordData\logs`  | The log files                                                              |

The decision is made once at start (`src-tauri/src/portable.rs`). Without the marker the exe behaves exactly like an installed Mumcord.

Portable mode changes nothing on the system unless the user asks for it:

- No `mumble://` link handler: the first-start wizard leaves "Open mumble:// links with Mumcord" unticked (Settings still offers it).
- No start at login: off by default, as in every build, and only set when the user turns it on.
- No Start menu or desktop shortcuts: only the installer creates them.

About shows the version as "… · Portable", the diagnostic file says "(portable)" after the version, and the update hint asks for the new portable zip. To update, unpack the new zip into the same folder and replace `Mumcord.exe`; `MumcordData` stays.

## Identity and passwords

The identity certificate and saved server passwords stay in Windows Credential Manager, as in the installed app, and not in `MumcordData`. A private key in a plain file on a USB stick would be easy to copy, and protecting it with an extra password would ask for that password at every start. The trade-off: the identity belongs to the Windows user, even in portable mode. On another PC or for another Windows user, export the identity under Settings → Identity (a password-protected file) and import it there.

## WebView2

Mumcord draws its window with the Microsoft Edge WebView2 Runtime. Windows 11 and most Windows 10 PCs have it; the installer downloads it when it is missing. The portable zip does not bundle it (the fixed-version runtime would add well over 100 MB). If it is missing, Mumcord shows a message box with Microsoft's download link and offers to open it, instead of failing without a window (`src-tauri/src/webview2.rs`).

## Build and tests

The Windows job of `.github/workflows/build-and-smoke.yml` packages the zip after the NSIS bundle, from the same `mumcord.exe` (which Tauri signs in place when Azure signing is set up). The Windows smoke test unpacks the zip, connects from that folder to the local test server and checks that the data went to `MumcordData` and nothing to the profile folders. The release lists the zip in `SHA256SUMS`, attests it and attaches it; the `windows-x86_64` SBOM covers it, and the release manifest (`scripts/release-manifest.sh`) records it with every other asset.
