# Code signing policy

This policy applies to the Windows release builds of Mumcord published on the [GitHub Releases page](https://github.com/Mumcord/mumcord/releases). They are signed with a [Certum](https://www.certum.eu) code signing certificate through Certum's SimplySign cloud signing service. Certum issues the certificate after verifying the maintainer's identity, and the verified name appears as the publisher. Until the signing setup is active, Windows releases are unsigned and their release notes say so.

## What is signed

Only binaries built from this repository's source code by the release workflow (`.github/workflows/release.yml`) on GitHub-hosted runners are signed:

- the Mumcord application executable (`mumcord.exe`), also shipped as `Mumcord.exe` in the portable zip (`Mumcord_<version>_x64-portable.zip`),
- its uninstaller, and
- the Windows installer (`Mumcord_<version>_x64-setup.exe`) that contains them.

Every signed file carries the product name "Mumcord" and the release's version number in its file metadata, and a time stamp from Certum. Third-party binaries are never signed under this policy. Test builds and builds from pull requests are never signed.

The macOS builds are signed and notarized with an Apple Developer ID; the Linux builds are not signed. Neither is covered by this policy.

## Team roles

| Role                     | Members                                                 |
| ------------------------ | ------------------------------------------------------- |
| Committers and reviewers | [Tobias Vorwachs (ToBee94)](https://github.com/ToBee94) |
| Release managers         | [Tobias Vorwachs (ToBee94)](https://github.com/ToBee94) |

- **Committers** may change the source code in this repository without further review.
- **Reviewers** review every contribution from people who are not committers (pull requests) before it is merged.
- **Release managers** start releases by pushing a version tag and publish the draft release by hand.

## Release process

1. A release is started by pushing a version tag on `main` (see [RELEASING.md](RELEASING.md)). The workflow stops if the tag does not match the app version or the commit is not on `main`.
2. The release workflow builds the application on a GitHub-hosted runner. Only this job, running in the protected `release` environment for a version tag, may sign: only this job receives the SimplySign login (account name and one-time-code secret) that unlocks the certificate.
3. The application executable is signed first, then the installer is built around it and signed as well.
4. The signed installer is smoke-tested and attached to a draft GitHub release, which a release manager reviews and publishes by hand.

## Security requirements

- All team members use multi-factor authentication (MFA) for their GitHub account and their Certum SimplySign account (which always asks for a one-time code from the SimplySign mobile app).
- The signing key is held by Certum in a hardware security module and never leaves it. No signing key, certificate, password or token is stored in this repository or on a team member's computer.
- The SimplySign login is stored only as secrets of this repository's protected `release` environment, which accepts only version tags, and never appears in build logs. Deleting these secrets and having Certum reset the SimplySign token stops all automated signing immediately.

## Privacy

Mumcord sends no telemetry. The network connections it makes and the data it stores are described in the [privacy policy](PRIVACY.md).

## Download

Installers for Windows (`.exe`), macOS (`.dmg`, Apple Silicon and Intel) and Linux (`.AppImage`) are on the [Releases page](https://github.com/Mumcord/mumcord/releases), each with a `SHA256SUMS` file to check the download. Each release's notes say whether its Windows and macOS builds are signed; unsigned builds show a security warning on first start, and the notes explain how to open the app anyway.
