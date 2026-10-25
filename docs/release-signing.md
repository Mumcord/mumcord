# macOS signing and notarization

Release builds (`vX.Y.Z` tags, `.github/workflows/release.yml`) sign and notarize the universal
`.dmg` when the Apple secrets below exist. Without them the DMG is built unsigned, exactly as
before, and the release notes tell users how to open an unsigned app. Test builds are never signed.

Signing is done by Tauri itself (hardened runtime, `src-tauri/Entitlements.plist`, microphone
usage text in `src-tauri/Info.plist`). It needs no server-side change.

## One-time setup (maintainer)

1. **Join the Apple Developer Program** at <https://developer.apple.com/programs/> (99 USD per year).
   Use the account holder's Apple ID; approval can take a day or two.
2. **Create a certificate signing request (CSR)** on a Mac: open *Keychain Access*, menu
   *Certificate Assistant → Request a Certificate From a Certificate Authority*. Enter your email,
   choose *Saved to disk*, and save the `.certSigningRequest` file.
3. **Create the certificate**: at <https://developer.apple.com/account/resources/certificates>
   choose *Create a certificate → Developer ID Application*, upload the CSR, download the `.cer`
   and double-click it to add it to your login keychain.
4. **Export the `.p12`**: in *Keychain Access → My Certificates*, right-click
   "Developer ID Application: Your Name (TEAMID)" → *Export*, format `.p12`, and set a strong
   password. Keep the file and password out of the repository.
5. **Create an App Store Connect API key** at
   <https://appstoreconnect.apple.com/access/integrations/api> → *Team Keys → Generate API Key*,
   access *Developer*. Note the **Issuer ID** (top of the page) and the **Key ID**, and download
   the `AuthKey_<KEYID>.p8` file (it can be downloaded only once).
6. **Add the GitHub secrets** (repository *Settings → Secrets and variables → Actions → New
   repository secret*). Exact names:

   | Secret                       | Value                                                              |
   | ---------------------------- | ------------------------------------------------------------------ |
   | `APPLE_CERTIFICATE`          | `base64 -i certificate.p12 \| pbcopy` (base64 of the `.p12`)       |
   | `APPLE_CERTIFICATE_PASSWORD` | the password chosen in step 4                                      |
   | `APPLE_SIGNING_IDENTITY`     | `Developer ID Application: Your Name (TEAMID)` (as in Keychain)    |
   | `APPLE_API_ISSUER`           | Issuer ID from step 5                                              |
   | `APPLE_API_KEY`              | Key ID from step 5                                                 |
   | `APPLE_API_KEY_P8`           | the full text of the `AuthKey_<KEYID>.p8` file, including BEGIN/END lines |

   All six must exist. If any is missing, the build warns and produces an unsigned DMG.

## Test with a pre-release tag

1. Bump the version to a pre-release (for example `0.1.0-rc.1`) everywhere the guard checks, merge
   to `main`, and push the tag `v0.1.0-rc.1` (see `RELEASING.md`).
2. Wait for the workflow. The macOS build log should show signing and notarization; the draft
   release notes then say the macOS app is signed and notarized.
3. Download the DMG on a Mac and check:
   `codesign --verify --deep --strict --verbose=2 /Applications/Mumcord.app` and
   `spctl --assess --type execute --verbose /Applications/Mumcord.app` ("accepted, source=Notarized
   Developer ID"). Start the app and confirm the microphone permission prompt appears.
4. Delete the draft release and the test tag afterwards.

## Notes

- The certificate is valid for 5 years; the API key does not expire but can be revoked.
- Rotating a secret: replace it in the GitHub settings; no code change is needed.
- Global shortcuts and mouse buttons need the user to grant *Input Monitoring* or *Accessibility*
  in System Settings; macOS asks at runtime, no entitlement is involved.
