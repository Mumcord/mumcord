# Release signing

Release builds (`vX.Y.Z` tags) sign the Windows and macOS bundles only when the matching secrets exist; otherwise they are built unsigned, exactly as before, and the release notes tell users how to open an unsigned app. Test builds are never signed. No signing key, certificate or token is ever stored in this repository.

- [Windows signing (SignPath Foundation)](#windows-signing-signpath-foundation)
- [macOS signing and notarization](#macos-signing-and-notarization)

## Windows signing (SignPath Foundation)

SignPath Foundation signs open-source projects for free; the certificate is issued to SignPath Foundation. Its conditions are at <https://signpath.org/terms>. The public side of those conditions lives in [CODE_SIGNING.md](../CODE_SIGNING.md) (roles, release process, MFA) and [PRIVACY.md](../PRIVACY.md).

### How the workflow signs

When `SIGNPATH_API_TOKEN` and `SIGNPATH_ORGANIZATION_ID` are set, the Windows job in `.github/workflows/build-and-smoke.yml` (release builds only):

1. builds the app without an installer (`tauri build --no-bundle`) and uploads `mumcord.exe` as a workflow artifact,
2. submits it to SignPath (`signpath/github-action-submit-signing-request`) and waits for your approval,
3. bundles the NSIS installer around the signed exe (`tauri bundle --bundles nsis`),
4. uploads the installer, submits it as a second signing request and waits again,
5. replaces the unsigned installer with the signed one. The smoke test and the draft release then use the signed installer, and the release notes say it is signed.

Two passes are needed because SignPath cannot sign files inside a finished NSIS installer. If either secret is missing, the job warns and builds the installer unsigned as before. If a request is denied or not approved within 90 minutes, the release fails and nothing is published.

### One-time setup (maintainer)

1. **Turn on MFA** for your GitHub account and, after step 2, for your SignPath account. Both are required by SignPath Foundation for every team member.
2. **Apply** at <https://signpath.org/apply> with the repository `https://github.com/ToBee94/mumcord`, the homepage `https://mumcord.app` and the code signing policy URL `https://github.com/ToBee94/mumcord/blob/main/CODE_SIGNING.md`. Wait for approval; SignPath then creates the organization.
3. **Link the code signing policy on the website**: the home page and the download page of mumcord.app must show a "Code signing policy" link to `CODE_SIGNING.md` (the README and the release notes already do).
4. **Set up the project in SignPath** (<https://app.signpath.io>):
   - Trusted build system: add the predefined **GitHub.com** trusted build system to the organization and link it to the project.
   - Project slug: `mumcord`, repository URL `https://github.com/ToBee94/mumcord`.
   - Artifact configuration slug: `initial`, with the content below. It is used for both requests: each workflow artifact is a ZIP holding exactly one `.exe`.
   - Signing policy slug: `release-signing`, with the SignPath Foundation certificate, **manual approval** and yourself as the only approver.
   - Create a CI user (or API token) with _submitter_ permission for the `release-signing` policy.

   ```xml
   <?xml version="1.0" encoding="utf-8"?>
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <parameters>
       <parameter name="version" required="true" />
     </parameters>
     <zip-file>
       <pe-file path="*.exe" product-name="Mumcord" product-version="${version}">
         <authenticode-sign />
       </pe-file>
     </zip-file>
   </artifact-configuration>
   ```

   The workflow passes the app version (for example `0.3.0`) as `version`, so SignPath refuses any file whose product name or product version does not match.

5. **Add the GitHub settings** (repository _Settings → Secrets and variables → Actions_):

   | Name                       | Kind                                   | Value                                |
   | -------------------------- | -------------------------------------- | ------------------------------------ |
   | `SIGNPATH_API_TOKEN`       | secret                                 | API token of the CI user from step 4 |
   | `SIGNPATH_ORGANIZATION_ID` | variable (or secret, either one works) | organization ID shown in SignPath    |

### Test with a pre-release tag

1. Push a pre-release tag as described under macOS below.
2. Approve both signing requests in SignPath while the Windows build waits.
3. On Windows, right-click the downloaded installer → _Properties → Digital Signatures_: the signer is SignPath Foundation. After installing, check `mumcord.exe` in the install folder the same way, and that _Details_ shows product name "Mumcord" and the version.
4. Delete the draft release and the test tag afterwards.

New certificates have no SmartScreen reputation yet, so the first signed releases may still show a SmartScreen warning until enough users have downloaded them.

## macOS signing and notarization

Release builds (`vX.Y.Z` tags, `.github/workflows/release.yml`) sign and notarize the universal
`.dmg` when the Apple secrets below exist. Without them the DMG is built unsigned, exactly as
before, and the release notes tell users how to open an unsigned app. Test builds are never signed.

Signing is done by Tauri itself (hardened runtime, `src-tauri/Entitlements.plist`, microphone
usage text in `src-tauri/Info.plist`). It needs no server-side change.

### One-time setup (maintainer)

1. **Join the Apple Developer Program** at <https://developer.apple.com/programs/> (99 USD per year).
   Use the account holder's Apple ID; approval can take a day or two.
2. **Create a certificate signing request (CSR)** on a Mac: open _Keychain Access_, menu
   _Certificate Assistant → Request a Certificate From a Certificate Authority_. Enter your email,
   choose _Saved to disk_, and save the `.certSigningRequest` file.
3. **Create the certificate**: at <https://developer.apple.com/account/resources/certificates>
   choose _Create a certificate → Developer ID Application_, upload the CSR, download the `.cer`
   and double-click it to add it to your login keychain.
4. **Export the `.p12`**: in _Keychain Access → My Certificates_, right-click
   "Developer ID Application: Your Name (TEAMID)" → _Export_, format `.p12`, and set a strong
   password. Keep the file and password out of the repository.
5. **Create an App Store Connect API key** at
   <https://appstoreconnect.apple.com/access/integrations/api> → _Team Keys → Generate API Key_,
   access _Developer_. Note the **Issuer ID** (top of the page) and the **Key ID**, and download
   the `AuthKey_<KEYID>.p8` file (it can be downloaded only once).
6. **Add the GitHub secrets** (repository _Settings → Secrets and variables → Actions → New
   repository secret_). Exact names:

   | Secret                       | Value                                                                     |
   | ---------------------------- | ------------------------------------------------------------------------- |
   | `APPLE_CERTIFICATE`          | `base64 -i certificate.p12 \| pbcopy` (base64 of the `.p12`)              |
   | `APPLE_CERTIFICATE_PASSWORD` | the password chosen in step 4                                             |
   | `APPLE_SIGNING_IDENTITY`     | `Developer ID Application: Your Name (TEAMID)` (as in Keychain)           |
   | `APPLE_API_ISSUER`           | Issuer ID from step 5                                                     |
   | `APPLE_API_KEY`              | Key ID from step 5                                                        |
   | `APPLE_API_KEY_P8`           | the full text of the `AuthKey_<KEYID>.p8` file, including BEGIN/END lines |

   All six must exist. If any is missing, the build warns and produces an unsigned DMG.

### Test with a pre-release tag

1. Bump the version to a pre-release (for example `0.1.0-rc.1`) everywhere the guard checks, merge
   to `main`, and push the tag `v0.1.0-rc.1` (see `RELEASING.md`).
2. Wait for the workflow. The macOS build log should show signing and notarization; the draft
   release notes then say the macOS app is signed and notarized.
3. Download the DMG on a Mac and check:
   `codesign --verify --deep --strict --verbose=2 /Applications/Mumcord.app` and
   `spctl --assess --type execute --verbose /Applications/Mumcord.app` ("accepted, source=Notarized
   Developer ID"). Start the app and confirm the microphone permission prompt appears.
4. Delete the draft release and the test tag afterwards.

### Notes

- The certificate is valid for 5 years; the API key does not expire but can be revoked.
- Rotating a secret: replace it in the GitHub settings; no code change is needed.
- Global shortcuts and mouse buttons need the user to grant _Input Monitoring_ or _Accessibility_
  in System Settings; macOS asks at runtime, no entitlement is involved.
