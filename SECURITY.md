# Security policy

This policy applies to all software published by the [Mumcord organisation](https://github.com/Mumcord) on GitHub. It explains which versions get security fixes, how to report a vulnerability and what you can expect from us afterwards.

## Scope

Covered is every repository of the Mumcord organisation and everything built and published from it, in particular:

| Software                                                | What it is                                                              |
| ------------------------------------------------------- | ----------------------------------------------------------------------- |
| [mumcord](https://github.com/Mumcord/mumcord)           | The voice chat client, its installers and packages, and its Rust crates |
| [mumpeer](https://github.com/Mumcord/mumpeer)           | The relay server for Mumcord's peer-to-peer features                    |
| [mumbot](https://github.com/Mumcord/mumbot)             | The bots for Mumble servers                                             |
| [plugins](https://github.com/Mumcord/plugins)           | The plugin registry and the official plugins                            |
| [homebrew-tap](https://github.com/Mumcord/homebrew-tap) | The Homebrew tap                                                        |
| [mumcord-www](https://github.com/Mumcord/mumcord-www)   | The website at mumcord.app                                              |

The release, build and signing pipelines of these repositories are covered as well.

Not covered, because we do not maintain it:

- **The Mumble server and the original Mumble client.** Report these to the [Mumble project](https://github.com/mumble-voip/mumble/security).
- **Mumble servers run by other people.** Report problems to the server's operator.
- **Plugins from other authors.** Report these to the plugin's author. A plugin that breaks out of Mumcord's plugin sandbox or gets around its permissions is a vulnerability in Mumcord and is covered.
- **Third-party libraries.** Report these to the library's maintainers. If the problem can be exploited through Mumcord software, please tell us too, so that we can update or work around it.

## Supported versions

Mumcord software is in an early stage (version 0.x). Security fixes are made for the **latest release** of each product and ship as a new patch release. Older releases do not get fixes; please update to the latest release.

A release candidate that is currently published is covered too. Development builds and builds from pull requests are not supported, but reports about a vulnerability on `main` or a milestone branch are welcome.

## Reporting a vulnerability

**Do not report vulnerabilities in public issues, pull requests or discussions.** A public report puts users at risk before a fix exists.

Report privately through GitHub, for all Mumcord software:

**<https://github.com/Mumcord/mumcord/security/advisories/new>**

Name the affected software in the report, fill in the form and submit it. Only you and the maintainers can read the report.

If you cannot use this form, open a normal issue that only says you would like to report a security problem and asks for a private channel. Do not include any details in it.

A good report contains:

- the affected software, its version and your operating system,
- what an attacker can achieve and what they need for it (for example: a malicious server, a malicious plugin, a user in the same channel, access to the local machine),
- steps to reproduce, ideally with a proof of concept,
- any logs, screenshots or crash output that help, with personal data removed.

You may write in English or German.

## What to expect

Mumcord is maintained by a small team in its spare time. We aim for the following and will tell you if we cannot keep to it:

| Step                                                    | Target                   |
| ------------------------------------------------------- | ------------------------ |
| Confirmation that we received your report               | within 3 working days    |
| First assessment (accepted, rejected or more questions) | within 7 days            |
| Fix released for a critical or high severity problem    | within 30 days           |
| Fix released for a medium or low severity problem       | with a following release |

We keep you informed while we work on the fix and tell you when it is released.

## Disclosure

We follow coordinated disclosure:

- Please keep the vulnerability private until a fixed release is available, or until 90 days have passed since your report, whichever comes first. If a fix needs longer, we will ask you for more time and explain why.
- When the fix is released, we publish a GitHub security advisory for the affected repository, request a CVE identifier where it makes sense, and mention the fix in the changelog.
- We credit you in the advisory, unless you prefer to stay anonymous.

There is no bug bounty programme; we cannot pay for reports.

## Rules for testing

We welcome security research done in good faith and will not take legal action against it, as long as you keep to these rules:

- Test only with your own installation, your own accounts and a Mumble server you run yourself or have permission to test. Never test against public or other people's Mumble servers, relay servers or bots.
- Do not access, change or delete other people's data, and do not listen to or record other people's audio or chats.
- Do not disturb services for others (no denial-of-service or load tests against systems you do not own).
- No social engineering, phishing or physical attacks against maintainers, contributors or users.
- If you come across personal data by accident, stop, do not keep or share it, and tell us in your report.

## What counts as a vulnerability

Examples of problems we want to hear about:

- content from a server or another user (names, comments, chat messages, images, links) that runs code, reads local files or takes over the user interface,
- weaknesses in TLS or server certificate checks, or a way to read or misuse a user's client certificate (it is their identity),
- a plugin that escapes its sandbox or uses abilities it was not granted,
- a way to make the updater, an installer or the plugin registry deliver software that we did not publish, or to get past signature or checksum checks,
- a way to use the relay server without being a verified user, to read other users' traffic through it, or to learn an IP address it is meant to hide,
- personal data, chat contents, audio or secrets that end up in logs, crash reports or on the network without the user's consent,
- secrets or signing material exposed in a repository, a build log or a release.

Usually not a vulnerability: problems that need an attacker who already controls the user's computer or account, missing hardening without a way to exploit it, and reports from automated scanners without a working example. Ordinary bugs and crashes without a security impact belong in the public issue tracker.

## How we protect releases

- Dependencies are checked for known advisories in CI, and updates are proposed automatically.
- Releases are built by the release workflow on GitHub-hosted runners, never on a developer's computer. Each release comes with checksums (`SHA256SUMS`), a software bill of materials and a build attestation, and a published release is never changed afterwards. Details: [RELEASING.md](https://github.com/Mumcord/mumcord/blob/main/RELEASING.md).
- Signing of the release builds is described in the [code signing policy](https://github.com/Mumcord/mumcord/blob/main/CODE_SIGNING.md).
- Which data Mumcord stores and which connections it makes is described in the [privacy policy](https://github.com/Mumcord/mumcord/blob/main/PRIVACY.md).

## Changes to this policy

Changes are made in this file and are visible in its history.
