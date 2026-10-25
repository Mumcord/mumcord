# Code signing policy

This policy applies to the Windows release builds of Mumcord published on the [GitHub Releases page](https://github.com/ToBee94/mumcord/releases). They are signed with [Azure Artifact Signing](https://learn.microsoft.com/azure/artifact-signing/) by Microsoft. The certificate is issued to the maintainer's company, whose legal name, verified by Microsoft, appears as the publisher. Until the signing setup is active, Windows releases are unsigned and their release notes say so.

## What is signed

Only binaries built from this repository's source code by the release workflow (`.github/workflows/release.yml`) on GitHub-hosted runners are signed:

- the Mumcord application executable (`mumcord.exe`),
- its uninstaller, and
- the Windows installer (`Mumcord_<version>_x64-setup.exe`) that contains them.

Every signed file carries the product name "Mumcord" and the release's version number in its file metadata, and a time stamp from Microsoft. Third-party binaries are never signed under this policy. Test builds and builds from pull requests are never signed.

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
2. The release workflow builds the application on a GitHub-hosted runner. Only this job, running in the protected `release` environment for a version tag, may sign: it proves its identity to Azure through GitHub's OpenID Connect, and Azure accepts no other caller.
3. The application executable is signed first, then the installer is built around it and signed as well.
4. The signed installer is smoke-tested and attached to a draft GitHub release, which a release manager reviews and publishes by hand.

## Security requirements

- All team members use multi-factor authentication (MFA) for their GitHub account and their Microsoft (Azure) account.
- Signing keys are held by Microsoft in hardware security modules and never leave them. No signing key, certificate, password or token is stored in this repository, in GitHub secrets or on a team member's computer.
- Signing access is granted only to the release workflow through a federated credential that names this repository and its `release` environment. Revoking that credential stops all signing immediately.

## Privacy

Mumcord sends no telemetry. The network connections it makes and the data it stores are described in the [privacy policy](PRIVACY.md).

## Download

Installers for Windows (`.exe`), macOS (`.dmg`, Apple Silicon and Intel) and Linux (`.AppImage`) are on the [Releases page](https://github.com/ToBee94/mumcord/releases), each with a `SHA256SUMS` file to check the download. Each release's notes say whether its Windows and macOS builds are signed; unsigned builds show a security warning on first start, and the notes explain how to open the app anyway.
