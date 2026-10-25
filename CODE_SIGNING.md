# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

This policy applies to the Windows release builds of Mumcord published on the [GitHub Releases page](https://github.com/ToBee94/mumcord/releases). Until the SignPath Foundation application is approved and the signing setup is active, Windows releases are unsigned and their release notes say so.

## What is signed

Only binaries built from this repository's source code by the release workflow (`.github/workflows/release.yml`) on GitHub-hosted runners are signed:

- the Mumcord application executable (`mumcord.exe`), and
- the Windows installer (`Mumcord_<version>_x64-setup.exe`) that contains it.

Every signed file carries the product name "Mumcord" and the release's version number in its file metadata. Third-party binaries are never signed under this policy. Test builds and builds from pull requests are never signed.

The macOS and Linux builds are not covered by this policy.

## Team roles

| Role                     | Members                                                 |
| ------------------------ | ------------------------------------------------------- |
| Committers and reviewers | [Tobias Vorwachs (ToBee94)](https://github.com/ToBee94) |
| Approvers                | [Tobias Vorwachs (ToBee94)](https://github.com/ToBee94) |

- **Committers** may change the source code in this repository without further review.
- **Reviewers** review every contribution from people who are not committers (pull requests) before it is merged.
- **Approvers** approve each signing request by hand in SignPath.

## Release process

1. A release is started by pushing a version tag on `main` (see [RELEASING.md](RELEASING.md)).
2. The release workflow builds the application on GitHub-hosted runners and submits the executable to SignPath for signing.
3. An approver checks the signing request (version, source commit, workflow run) and approves it manually. The installer is then built around the signed executable and submitted as a second signing request, which is approved the same way.
4. The signed installer is smoke-tested and attached to a draft GitHub release, which the maintainer reviews and publishes by hand.

No signing request is approved automatically.

## Security requirements

- All team members use multi-factor authentication (MFA) for their GitHub account and their SignPath account.
- Signing keys are held by SignPath; no signing key or certificate is stored in this repository or on a team member's computer. The API token that submits signing requests is stored only as an encrypted GitHub Actions secret.

## Privacy

Mumcord sends no telemetry. The network connections it makes and the data it stores are described in the [privacy policy](PRIVACY.md).
