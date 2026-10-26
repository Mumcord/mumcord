# Release signing

Release builds (`vX.Y.Z` tags) sign the Windows and macOS bundles only when the matching settings exist; otherwise they are built unsigned (macOS: ad-hoc signed, see below), and the release notes tell users how to open an unsigned app. Test builds are never signed. No signing key, certificate, password or token is ever stored in this repository.

- [Windows signing (Azure Artifact Signing)](#windows-signing-azure-artifact-signing)
- [macOS signing and notarization](#macos-signing-and-notarization)

## Windows signing (Azure Artifact Signing)

Windows releases are signed with [Azure Artifact Signing](https://learn.microsoft.com/azure/artifact-signing/) (formerly Azure Trusted Signing) under the maintainer's company. Microsoft validates the company and issues short-lived certificates in its name; the private keys never leave Microsoft's hardware. Users see the company's validated legal name, "Tobias Vorwachs Einzelunternehmen", as the publisher in the Windows security prompt and under _Properties → Digital Signatures_.

Artifact Signing is open to organizations in the EU; individuals outside the US and Canada are not eligible, which is why the account belongs to the company (fuewa.systems) and not to a private person. The public side of the policy lives in [CODE_SIGNING.md](../CODE_SIGNING.md) and [PRIVACY.md](../PRIVACY.md).

**Costs:** the Basic plan costs about 10 USD per month (5,000 signatures included; a release uses fewer than ten) and needs a paid Azure subscription. Identity validation is free.

### How the workflow signs

When all six repository variables below are set, the Windows job in `.github/workflows/build-and-smoke.yml` (release builds only):

1. compiles the app (`tauri build --no-bundle`),
2. downloads pinned versions of Microsoft's `signtool` and the Artifact Signing plugin it loads, and writes a CI-only Tauri config overlay that sets `bundle.windows.signCommand`,
3. logs in to Azure with GitHub OpenID Connect (`azure/login`): Azure trusts the workflow run itself, so no client secret or password exists anywhere,
4. bundles the NSIS installer with that overlay (`tauri bundle --bundles nsis --config …`). Tauri signs `mumcord.exe` first, then the uninstaller and finally the installer, so the installer contains the signed app. Every file is time-stamped by Microsoft, so signatures stay valid after the short-lived certificate expires.

The smoke test and the draft release then use the signed installer, and the release notes say it is signed. The signing command is never in `src-tauri/tauri.conf.json`, so local and test builds stay unsigned and need no Azure access. If any variable is missing, the job warns and builds the installer unsigned as before. If the Azure login or a signature fails, the release fails and nothing is published.

Signing needs no manual approval: the job can only sign when it runs in the `release` environment of this repository, which only accepts version tags.

### One-time setup (maintainer)

All steps happen in the [Azure portal](https://portal.azure.com) and on GitHub. Turn on multi-factor authentication for your Microsoft (Azure) account and your GitHub account first.

1. **Paid Azure subscription.** Create an Azure account for the company with a pay-as-you-go subscription (a free trial subscription cannot use Artifact Signing). Note the **Subscription ID** (_Subscriptions_) and the **Tenant ID** (_Microsoft Entra ID → Overview_).
2. **Register the resource provider.** _Subscriptions → your subscription → Resource providers_, search `Microsoft.CodeSigning` and choose **Register**.
3. **Create the Artifact Signing account.** Search the portal for _Artifact Signing accounts → Create_: a new resource group (for example `mumcord-signing`), a globally unique account name (3 to 24 letters and digits, for example `fuewasigning`), region **North Europe (or Poland Central; West Europe currently refuses new customers)**, pricing tier **Basic**. Note the account name and the **Account URI** on its overview page (`https://neu.codesigning.azure.net/` for West Europe).
4. **Give yourself the identity role.** On the signing account: _Access control (IAM) → Add role assignment_ → **Artifact Signing Identity Verifier** for your own user. Without it the next step is greyed out.
5. **Validate the organization.** On the signing account: _Identity validations_ → **Organization** → _New identity → Public_, then enter exactly as in the official register or trade registration:
   - legal name: `Tobias Vorwachs Einzelunternehmen`,
   - business identifier (for example the trade registration, VAT ID or tax number the form asks for) and the registered address,
   - primary and secondary email on the company domain (addresses ending in `@fuewa.systems`); Microsoft sends verification links there,
   - website: `https://fuewa.systems`,
   - first and last name of the person representing the company, exactly as on your ID card.

   Use **Certificate subject preview** to check the publisher name before choosing **Create**. When the status changes to **Action Required**, follow the link (also sent to the primary email) to verify yourself with your ID through Microsoft's partner and the Microsoft Authenticator app. Microsoft may also ask for documents (trade registration, domain invoice for fuewa.systems), all issued within the last 12 months. Validation takes 1 to 20 business days; wait for the status **Completed**. The legal name validated here is what users see as the publisher.

6. **Create the certificate profile.** On the signing account: _Certificate profiles → Create → Public Trust_, a name (for example `mumcord`), and the identity validation from step 5. Note the profile name.
7. **Create the app registration for GitHub.** _Microsoft Entra ID → App registrations → New registration_, name `mumcord-github-signing`, single tenant, no redirect URI. Note its **Application (client) ID**. Do not create a client secret.
8. **Trust GitHub (federated credential).** In the app registration: _Certificates & secrets → Federated credentials → Add credential_ → scenario **GitHub Actions deploying Azure resources**:
   - organization `ToBee94`, repository `mumcord`,
   - entity type **Environment**, environment name `release`,
   - name `mumcord-release`.

   This lets only workflow jobs in the `release` environment of `ToBee94/mumcord` sign.

9. **Allow the app to sign.** On the signing account (or only the certificate profile from step 6): _Access control (IAM) → Add role assignment_ → **Artifact Signing Certificate Profile Signer** → members: the app registration `mumcord-github-signing`.
10. **Create the GitHub environment.** Repository _Settings → Environments → New environment_ `release`. Under _Deployment branches and tags_ choose **Selected branches and tags** and add the tag rule `v*`, so only release tags can sign.
11. **Add the GitHub variables.** Repository _Settings → Secrets and variables → Actions → Variables → New repository variable_. These are identifiers, not secrets; exact names:

    | Variable                 | Value                                                                    |
    | ------------------------ | ------------------------------------------------------------------------ |
    | `AZURE_CLIENT_ID`        | Application (client) ID of the app registration (step 7)                 |
    | `AZURE_TENANT_ID`        | Tenant ID (step 1)                                                       |
    | `AZURE_SUBSCRIPTION_ID`  | Subscription ID (step 1)                                                 |
    | `AZURE_SIGNING_ENDPOINT` | Account URI from step 3, for example `https://neu.codesigning.azure.net` |
    | `AZURE_SIGNING_ACCOUNT`  | Artifact Signing account name (step 3)                                   |
    | `AZURE_CERT_PROFILE`     | certificate profile name (step 6)                                        |

    All six must exist. If any is missing, the build warns and produces an unsigned installer. The endpoint must match the account's region, otherwise signing fails with "403 Forbidden".

### Test with a pre-release tag

1. Push a pre-release tag as described under macOS below (for example `v0.3.0-rc.1`).
2. In the Windows build log, check that "Log in to Azure" succeeds and that "Bundle and sign Windows installer" shows signtool reporting "Successfully signed" for each file. The draft release notes then say the Windows build is signed.
3. On Windows, right-click the downloaded installer → _Properties → Digital Signatures_: the signer is "Tobias Vorwachs Einzelunternehmen" and the signature has a time stamp. After installing, check `mumcord.exe` in the install folder the same way, and that _Details_ shows product name "Mumcord" and the version.
4. Delete the draft release and the test tag afterwards.

New certificates have no SmartScreen reputation yet, so the first signed releases may still show a SmartScreen warning until enough users have downloaded them.

### Notes

- Nothing to renew: Microsoft rotates the short-lived certificates automatically. The identity validation must be renewed when Microsoft asks (by email), usually once a year.
- Removing access: delete the federated credential or the role assignment; no secret needs to be revoked.
- The Azure login token is short-lived, so the workflow compiles first and logs in right before signing.

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
  with that one identity: the app's only executable (the universal `Mumcord` binary; there are
  no helpers, sidecars or embedded frameworks), then the `.app` bundle.
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
