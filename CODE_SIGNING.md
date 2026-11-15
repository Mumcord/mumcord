# Code signing policy

This policy applies to the Windows release builds of Mumcord published on the [GitHub Releases page](https://github.com/Mumcord/mumcord/releases). They are signed with a [Certum](https://www.certum.eu) code signing certificate through Certum's SimplySign cloud signing service. Certum issues the certificate after verifying the maintainer's identity, and the verified name appears as the publisher. Until the signing setup is active, Windows releases are unsigned and their release notes say so.

## What is signed

Only binaries built from this repository's source code by the release workflow (`.github/workflows/release.yml`) on GitHub-hosted runners are signed:

- the Mumcord application executable (`mumcord.exe`), also shipped as `Mumcord.exe` in the portable zip (`Mumcord_<version>_x64-portable.zip`),
- its uninstaller, and
- the Windows installer (`Mumcord_<version>_x64-setup.exe`) that contains them.

Every signed file carries the product name "Mumcord" and the release's version number in its file metadata, and a time stamp from Certum. Third-party binaries are never signed under this policy. Test builds and builds from pull requests are never signed.

The macOS builds are signed and notarized with an Apple Developer ID. The release files for Linux and FreeBSD are not signed themselves; the package repository that carries them is (see [below](#linux-and-freebsd-package-repository)). Neither is covered by the Certum part of this policy.

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

## Linux and FreeBSD package repository

The package repository at <https://packages.mumcord.app> ([install steps](docs/package-repository.md)) is signed with two keys of its own, made only for this purpose (not a personal key):

| Key                                                                | Signs                                                                                                             | Fingerprint                                                                                  | Expires                         |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------- |
| OpenPGP RSA 4096, "Mumcord Package Signing <packages@mumcord.app>" | APT `InRelease`/`Release.gpg`, every `.rpm` (`rpm --addsign`) and `repomd.xml`, every pacman package and database | `A4096249A499587C9AEF13E7F022C81D47D2BBEB`                                                   | 2031-11-15                      |
| RSA 4096 (FreeBSD `pkg`)                                           | The FreeBSD catalogue, which lists each package's checksum                                                        | SHA-256 of `mumcord.pub`: `9c03cc83f882d84192c8489d070c519ac2ec190c7eb61a53161913d330aeafa6` | none (`pkg` keys do not expire) |

- **Public keys** are committed in `packaging/repo/` and published at the repository root. The workflow refuses to sign when a secret does not belong to the committed public key.
- **Private keys** exist only as the repository secrets `PACKAGE_REPO_GPG_KEY` and `FREEBSD_REPO_KEY`, used only by `.github/workflows/package-repo.yml`, and an offline backup (with the OpenPGP revocation certificate) kept by the release manager outside this repository. They never appear in logs.
- **Expiry and rotation:** about six months before 2031-11-15, the release manager extends the OpenPGP key's expiry (same fingerprint), commits and publishes the updated public key, and announces in the release notes that users refresh it (APT: download `mumcord.asc` again; DNF/zypper: `rpm --import` again; pacman: `pacman-key --refresh-keys` or add it again). A key that is suspected to be leaked is revoked at once: the revocation certificate is published as the new `mumcord.asc`, a new key is created, the secrets are replaced, and the release notes and [docs/package-repository.md](docs/package-repository.md) give the new fingerprint. The FreeBSD key is replaced the same way (new `mumcord.pub`, users download it again).

## Privacy

Mumcord sends no telemetry. The network connections it makes and the data it stores are described in the [privacy policy](PRIVACY.md).

## Download

Installers for Windows (`.exe`), macOS (`.dmg`, Apple Silicon and Intel) and Linux (`.AppImage`) are on the [Releases page](https://github.com/Mumcord/mumcord/releases), each with a `SHA256SUMS` file to check the download. Each release's notes say whether its Windows and macOS builds are signed; unsigned builds show a security warning on first start, and the notes explain how to open the app anyway.
