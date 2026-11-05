# Release signing

Release builds (`vX.Y.Z` tags) sign the Windows and macOS bundles only when the matching settings exist; otherwise they are built unsigned (macOS: ad-hoc signed, see below), and the release notes tell users how to open an unsigned app. Test builds are never signed. No signing key, certificate, password or token is ever stored in this repository.

- [Windows signing (Certum SimplySign)](#windows-signing-certum-simplysign)
- [macOS signing and notarization](#macos-signing-and-notarization)
- [Updater signing (in-app updater)](#updater-signing-in-app-updater)

## Windows signing (Certum SimplySign)

Windows releases are signed with a [Certum](https://www.certum.eu) code signing certificate whose private key lives in Certum's cloud HSM and is used through **SimplySign** ("Code Signing in the cloud"). The key never leaves Certum; signing works only while SimplySign Desktop is logged in with the account name and a one-time code (TOTP) from the SimplySign mobile app. Users see the name on the certificate as the publisher in the Windows security prompt and under _Properties → Digital Signatures_. The public side of the policy lives in [CODE_SIGNING.md](../CODE_SIGNING.md).

### How the workflow signs

When the two secrets and the variable below exist in the `release` environment, the Windows job in `.github/workflows/build-and-smoke.yml` (release builds only):

1. compiles the app (`tauri build --no-bundle`),
2. downloads SimplySign Desktop from Certum's download server (`files.certum.eu`), checks it against the pinned SHA-256 checksum and installs it silently,
3. checks the built-in TOTP generator against the RFC 6238 test vectors (`scripts/ci/totp.test.ps1`) and the signing script's argument handling (`scripts/ci/sign-windows.test.ps1`, also run on every PR),
4. logs SimplySign Desktop in (`scripts/ci/simplysign-login.ps1`): it computes the current code from `CERTUM_TOTP_SECRET`, types the account name and the code into the login window and waits until the certificate with the thumbprint `CERTUM_CERT_SHA1` appears in the Windows certificate store,
5. bundles the NSIS installer with a CI-only Tauri config overlay whose `bundle.windows.signCommand` calls `scripts/sign-windows.ps1`. Tauri signs `mumcord.exe` first, then the uninstaller and finally the installer, so the installer contains the signed app. Each file is signed with

   ```
   signtool sign /sha1 <CERTUM_CERT_SHA1> /fd sha256 /tr http://time.certum.pl /td sha256 /d Mumcord <file>
   signtool verify /pa /v <file>
   ```

   The time stamp from Certum keeps the signatures valid after the certificate expires.

The smoke test and the draft release then use the signed installer, and the release notes say it is signed. The signing command is never in `src-tauri/tauri.conf.json`, so local and test builds stay unsigned. If any of the three settings is missing, the job warns and builds the installer unsigned. If the login or a signature fails, the release fails and nothing is published.

Secrets never appear in the log: GitHub masks the secrets, the workflow masks the account name once more, and the scripts never print the TOTP secret or a code. They exist only in the `release` environment, which accepts only version tags.

### How fragile the login is

SimplySign Desktop has no command-line login, so the workflow drives its login window with simulated keystrokes (`WScript.Shell.SendKeys`), the approach other projects use in CI as well. This can break when:

- Certum changes the login window (title, field order) in a new SimplySign Desktop version. The version is pinned, so this only happens when the pin is updated, or when Certum stops offering the pinned version for download (the job then fails at the download step).
- The runner's desktop shows another window in front (a Windows update notice, a first-start dialog). The script activates the SimplySign window before typing, but cannot handle every dialog.
- The runner's clock drifts by more than a few seconds, so the code is rejected. The script starts typing only with at least 15 seconds of validity left.
- The secret is wrong. The script makes **at most two attempts**, so a wrong secret cannot lock the account through repeated failures.

### When the automated login fails

1. Open the failed run and check the "Log in to SimplySign" step. For a timing problem (window not found, certificate not available), choose **Re-run failed jobs**; the second run often passes.
2. If it keeps failing, sign by hand on a Windows PC (below), or ship this release unsigned for Windows by temporarily deleting the variable `CERTUM_CERT_SHA1` and re-running the release, then fix the automation.

**Signing by hand.** You need a Windows PC with SimplySign Desktop installed and logged in (with the code from the SimplySign mobile app), the Windows SDK (for `signtool`, installed with the Visual Studio C++ tools) and the Mumcord build prerequisites (`README.md`). In PowerShell 7, in a checkout of the release tag:

```powershell
npm ci
$env:CERTUM_CERT_SHA1 = '<thumbprint>'
$sign = Join-Path $PWD 'scripts\sign-windows.ps1'
$overlay = @{ bundle = @{ windows = @{ signCommand = @{ cmd = 'pwsh'; args = @('-NoProfile', '-File', $sign, '%1') } } } } | ConvertTo-Json -Depth 5 -Compress
Set-Content -Encoding utf8 tauri.signing.json $overlay
node scripts/stage-onnxruntime.js
npm run tauri build -- --bundles nsis --config tauri.signing.json --config src-tauri/tauri.onnxruntime.windows.json
Remove-Item tauri.signing.json
```

This runs the same `signtool` calls as the workflow on `mumcord.exe`, the uninstaller and the installer. The bundled DLLs (`onnxruntime.dll` for the camera background effects and the Visual C++ runtime it needs) already carry Microsoft's signature; Tauri only signs resources that are unsigned, so they keep it. To sign or check single files, call the script directly: `pwsh -File scripts/sign-windows.ps1 <file> [<file> …]`. Files signed by hand are not covered by the workflow's later steps: replacing the installer in the draft release also means replacing the portable zip and recreating its updater signature (`.sig`), `latest.json` and `SHA256SUMS`, so prefer re-running the workflow when possible.

### One-time setup (maintainer)

Turn on two-factor authentication for your GitHub account first.

1. **Order the certificate.** In the [Certum shop](https://shop.certum.eu), order a code signing certificate **in the cloud (SimplySign)**: _Open Source Code Signing_ for an individual open source developer, or _Standard Code Signing_ for the company. The name on the certificate (the publisher users see) is the person or company Certum verifies.
2. **Install the SimplySign mobile app** (Android or iOS) and create the SimplySign account with the e-mail address you will use as `CERTUM_USER`.
3. **Activate SimplySign and save the TOTP seed.** During activation Certum shows a **QR code** to scan with the SimplySign mobile app. This QR code contains the TOTP seed as text that starts with `otpauth://totp/…?secret=…`. Before or right after scanning it with the phone, read the text of the same QR code (for example save a screenshot and decode it with a QR reader that shows the text instead of opening it, such as `zbarimg` on Linux). Keep the whole `otpauth://` text: it is the `CERTUM_TOTP_SECRET`. Treat it like a password: never save it in a file in the repository, a chat or a ticket; store it only in GitHub and, if you like, in your password manager. The seed cannot be read back from the app later; if you missed it, ask Certum support to reset the SimplySign token activation and repeat this step.
4. **Get the certificate issued.** Complete Certum's identity verification (ID document, and for a company the registration documents) and, in the Certum panel, generate the certificate in SimplySign. Wait until it is issued.
5. **Find the thumbprint.** On a Windows PC, install SimplySign Desktop (from [Certum's download server](https://files.certum.eu/software/SimplySignDesktop/Windows/), the `-64-bit-en.msi` of the newest version) and log in with the e-mail address and the code from the mobile app. In PowerShell:

   ```powershell
   Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Format-List Subject, Thumbprint, NotAfter
   ```

   The 40-character `Thumbprint` of the Certum certificate is `CERTUM_CERT_SHA1`.

6. **Create the GitHub environment** if it does not exist yet: repository _Settings → Environments → New environment_ `release`. Under _Deployment branches and tags_ choose **Selected branches and tags** and add the tag rule `v*`, so only release tags can sign.
7. **Add the secrets and the variable** in that environment (_Settings → Environments → release_), not as repository secrets. Exact names:

   | Name                 | Kind     | Value                                                       |
   | -------------------- | -------- | ----------------------------------------------------------- |
   | `CERTUM_USER`        | secret   | SimplySign account e-mail address (step 2)                  |
   | `CERTUM_TOTP_SECRET` | secret   | the whole `otpauth://totp/…` text from the QR code (step 3) |
   | `CERTUM_CERT_SHA1`   | variable | certificate thumbprint (step 5)                             |

   All three must exist. If one is missing, the build warns and produces an unsigned installer. A bare base32 seed (only the `secret=` value) also works; the script then assumes Certum's parameters (HMAC-SHA256, 6 digits, 30 seconds), so the full `otpauth://` text is safer.

8. **Remove the old Azure setup.** Delete the repository variables `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT` and `AZURE_CERT_PROFILE`, and in the Azure portal the federated credential, the app registration `mumcord-github-signing` and the Artifact Signing account, so it no longer costs money.

### Test with a pre-release tag

1. Push a pre-release tag as described under macOS below (for example `v0.6.1-rc.1`).
2. In the Windows build log, check that "Log in to SimplySign" reports that the certificate is available and that "Bundle and sign Windows installer" shows "Successfully signed" and "Successfully verified" for each file. The draft release notes then say the Windows build is signed.
3. On Windows, right-click the downloaded installer → _Properties → Digital Signatures_: the signer is the name on the Certum certificate and the signature has a time stamp. After installing, check `mumcord.exe` in the install folder the same way, and that _Details_ shows product name "Mumcord" and the version.
4. Delete the draft release and the test tag afterwards.

New certificates have no SmartScreen reputation yet, so the first signed releases may still show a SmartScreen warning until enough users have downloaded them.

### Notes

- **Renewal:** the certificate is valid for the period you ordered (one to three years). After renewing, update `CERTUM_CERT_SHA1` to the new thumbprint; the TOTP seed stays the same unless the SimplySign token is re-activated.
- **Updating SimplySign Desktop:** change `SIMPLYSIGN_VERSION` in the "Install SimplySign Desktop" step and set `SIMPLYSIGN_SHA256` to the checksum of the new installer (`SimplySignDesktop-<version>-64-bit-en.msi` from `https://files.certum.eu/software/SimplySignDesktop/Windows/<version>/`), then test with a pre-release tag, because the login window may have changed.
- **Removing access:** delete the two secrets. If the TOTP seed may have leaked, ask Certum to reset the SimplySign token activation; the old seed then stops working.
- The SimplySign session is opened after compiling, right before signing, so it cannot time out during the build.

## macOS signing and notarization

Release builds (`vX.Y.Z` tags, `.github/workflows/release.yml`) sign and notarize the universal
`.dmg` when the Apple secrets below exist. Without them the app gets an ad-hoc signature instead
(see below), and the release notes tell users how to open an app that is not notarized. Test
builds are never signed with the Developer ID.

**Status:** the Apple Developer account exists; the certificate, API key and secrets are not set
up yet (issue #16), so releases are ad-hoc signed and not notarized.

Signing is done by Tauri itself. It needs no server-side change.

- **Identity:** `bundle.macOS.signingIdentity` in `src-tauri/tauri.conf.json` is `-` (ad-hoc).
  The `APPLE_SIGNING_IDENTITY` secret overrides it for signed releases. Tauri signs inside out
  with that one identity: the ONNX Runtime library in `Contents/Frameworks` (camera background
  effects; `src-tauri/tauri.onnxruntime.macos.json`, staged by `scripts/stage-onnxruntime.js`
  as one universal file), then the app's only executable (the universal `Mumcord` binary; there
  are no helpers or sidecars), then the `.app` bundle. The smoke test checks that the library
  is signed by the same team as the app.
- **Hardened runtime** (Tauri's default) with `src-tauri/Entitlements.plist`:
  - `com.apple.security.device.audio-input`: the microphone; without it the hardened runtime
    blocks capture.
  - `com.apple.security.cs.disable-library-validation`: positional audio loads Mumble game
    plugins (third-party `.dylib` files); library validation would refuse them.
  - Nothing else is needed: WebKit runs its JIT in Apple's own WebContent process, the network
    needs no entitlement outside the App Sandbox (Mumcord is not sandboxed), and screen capture,
    Input Monitoring and Accessibility are runtime permissions, not entitlements.
- **Usage texts** in `src-tauri/Info.plist`: microphone and local network. Screen recording has no
  usage text; macOS shows its own prompt.

### Why the ad-hoc signature matters

macOS remembers microphone access (TCC) and "Always Allow" on keychain items for a code
signature. A bundle without a valid signature has no stable identity, so macOS asked for the
microphone and for the login password on every keychain access, and "Allow" never stuck (issue
#15). The ad-hoc signature is stable for one build: the answers are remembered until the next
update, which asks once more. Only the Developer ID signature keeps them across updates.

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
   Developer ID") and `xcrun stapler validate /Applications/Mumcord.app` ("The validate action
   worked!"). `codesign -dvv /Applications/Mumcord.app` names the Developer ID certificate under
   `Authority`, shows `flags=0x10000(runtime)` and the team ID, and
   `codesign -d --entitlements - /Applications/Mumcord.app` lists the two entitlements above.
4. Start the app: the microphone prompt appears once (wizard microphone step or first connect).
   On a fresh Mac the keychain is never asked; items written by an earlier unsigned build are
   asked for once each ("Always Allow"). Quit, start again and connect: no prompt at all. Install
   the next pre-release over it: still no prompt.
5. Delete the draft release and the test tag afterwards.

For an ad-hoc signed (unsigned) build the same `codesign --verify --deep --strict` must pass, and
`codesign -dvv` shows `Signature=adhoc`. macOS asks for the microphone once and for each keychain
item once ("Always Allow"); after quitting and starting again nothing is asked.

### Notes

- The certificate is valid for 5 years; the API key does not expire but can be revoked.
- Rotating a secret: replace it in the GitHub settings; no code change is needed.
- Global shortcuts and mouse buttons need the user to grant _Input Monitoring_ or _Accessibility_
  in System Settings; macOS asks at runtime, no entitlement is involved.

## Updater signing (in-app updater)

The in-app updater (issue #168) installs only bundles signed with Mumcord's own **updater key**: a minisign (Ed25519) key pair as required by Tauri's updater plugin. It is separate from the Windows and macOS code signing above and does not depend on it. The app carries the **public key** (`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`); the **private key** exists only as a GitHub Actions secret and never in this repository, on a build machine's disk or in a log.

### What the app checks

Before anything is installed, the app downloads the bundle from the release's own download folder (`https://github.com/Mumcord/mumcord/releases/download/vX.Y.Z/`), then

1. verifies the minisign signature against the public key it carries, including the version the signature was made for (so an old signed bundle cannot be passed off as a newer release),
2. compares the bundle's SHA-256 with the release's `SHA256SUMS`.

Any mismatch discards the download and shows an error; the installed version stays as it is. As long as `pubkey` is empty (the placeholder in the repository today), the app cannot verify anything and only notifies about new versions with a link to the release page.

### How the workflow signs

The **Create draft release** job in `.github/workflows/release.yml` runs `scripts/sign-updater-bundles.sh` on the finished release files:

- it signs `Mumcord_X.Y.Z_amd64.AppImage`, `Mumcord_X.Y.Z_aarch64.AppImage`, `Mumcord_X.Y.Z_x64-setup.exe` (after its Windows signature) and `Mumcord_X.Y.Z_universal.app.tar.gz` (the signed and notarized `.app`, packed by the macOS build job) with `tauri signer sign --app-version X.Y.Z`, which writes a `.sig` next to each file,
- it checks every signature against the public key in `src-tauri/tauri.conf.json` with `scripts/verify-updater-signature.js`, so a private key that does not match the app fails the release instead of every user's update,
- it writes the update manifest `latest.json` (Tauri's static format; keys `linux-x86_64-appimage`, `linux-aarch64-appimage`, `windows-x86_64-nsis`, `darwin-aarch64-app`, `darwin-x86_64-app`).

The `.sig` files, `latest.json` and the `.app.tar.gz` are release assets and are listed in `SHA256SUMS`, so `releases/X.Y.Z.json` (post-release sync) records them like every other asset. The release **fails** when the secret is missing or `pubkey` is empty: a release never ships without updater signatures. The `.deb`, `.rpm`, AUR package and the portable Windows zip are not signed for the updater; they are updated by the package manager or by hand.

### One-time setup (maintainer)

1. On your own computer (not a shared or CI machine), with the project's npm dependencies installed, create the key pair with a strong password:

   ```sh
   npx tauri signer generate -w ~/.tauri/mumcord-updater.key
   ```

   It writes the private key to `~/.tauri/mumcord-updater.key` and the public key to `~/.tauri/mumcord-updater.key.pub`. Never commit or share the private key file.

2. Store a backup of the private key file and its password in your password manager. **If the private key is lost, installed apps can no longer be updated automatically**: users would have to install the next version by hand once.
3. In the repository _Settings → Secrets and variables → Actions → Secrets_, add two repository secrets with exactly these names:

   | Secret                               | Value                                      |
   | ------------------------------------ | ------------------------------------------ |
   | `TAURI_SIGNING_PRIVATE_KEY`          | the whole content of `mumcord-updater.key` |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the password chosen in step 1              |

4. Put the content of `mumcord-updater.key.pub` (one base64 line) into `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` and merge that change through a normal pull request. The public key is not secret.
5. Delete the private key file from your computer once the backup from step 2 is safe.

The first release built after these steps carries the public key, so it is the first version that can update itself; versions before it show a notice with a link to the release page.

### Rotating the key

Rotate when the private key may have leaked, its password is known to someone who should not have it, or a maintainer with access leaves.

1. Create a new key pair (step 1 above).
2. Release one version **signed with the old key** whose app already carries the **new** public key: put the new public key into `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` (normal pull request), keep the old private key in the secrets, and add the repository **variable** `UPDATER_ROTATION_OLD_PUBKEY` with the old public key, so the release job checks the signatures against the key the installed apps still trust. Installed apps accept this release; once it is installed they trust the new key. Delete the variable after this release.
3. Replace both secrets with the new private key and password. Every later release is signed with the new key.
4. If the old key leaked, skip step 2: replace the key and public key at once and tell users in the release notes that this one update must be installed by hand from the release page. A leaked key must not sign anything again.

Never reuse a key after rotation, and never put a private key into a pull request, an issue, a log or a chat.
