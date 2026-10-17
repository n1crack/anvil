# Code signing

How Anvil's releases get signed, per platform. The release workflow
(`.github/workflows/release.yml`) builds unsigned when the secrets below are
missing, so signing can be switched on without changing it.

## macOS (first)

Gatekeeper only opens downloaded apps without a warning when they are signed
with a **Developer ID Application** certificate and **notarized** by Apple.

- Needs a paid [Apple Developer Program](https://developer.apple.com/programs/enroll/)
  membership (99 USD a year; free accounts can't notarize). Only the Account
  Holder can create the Developer ID certificate.
- Tauri signs, notarizes and staples during `cargo tauri build` when these are
  set ([Tauri docs](https://v2.tauri.app/distribute/sign/macos/)); hardened
  runtime is on by default.

| Secret | What |
|---|---|
| `APPLE_CERTIFICATE` | The Developer ID Application certificate as a base64 `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Its password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Notarization with an app-specific password |

An App Store Connect API key (`APPLE_API_ISSUER`, `APPLE_API_KEY`,
`APPLE_API_KEY_PATH`) also works for notarization and doesn't depend on a
personal Apple ID; switching to it means writing the `.p8` file in the build
step.

Without a membership, releases are unsigned: macOS blocks the first launch and
users allow it in System Settings › Privacy & Security › Open Anyway
([Apple](https://support.apple.com/en-us/102445)).

## Linux (later)

No certificate authority is involved: sign `SHA256SUMS.txt` with a project GPG
key (`gpg --detach-sign --armor`) and publish the key's fingerprint. AppImages
can carry an embedded signature (`SIGN=1`), but nothing verifies it
automatically ([Tauri docs](https://v2.tauri.app/distribute/sign/linux/)).

## Windows (later): SignPath Foundation

[SignPath Foundation](https://signpath.org/terms) signs Windows builds of
open-source projects for free; the certificate is issued to SignPath
Foundation, which users see as the publisher. What it asks for:

- an OSI-approved license (GPLv3 qualifies) and no proprietary parts;
- a release already published in the form to be signed;
- every job up to the signing request running on GitHub-hosted runners
  ([origin verification](https://about.signpath.io/documentation/trusted-build-systems/github));
- MFA for everyone, reviewed pull requests, and a manual approval per release;
- a "Code signing policy" section on the website with the sentence "Free code
  signing provided by SignPath.io, certificate by SignPath Foundation".

The flow in GitHub Actions: build the installer, upload it with
`actions/upload-artifact`, submit it with
`signpath/github-action-submit-signing-request` (waiting for completion), then
publish the signed file. An MSI is signed in depth (the app inside too); an
NSIS installer is not, so prefer MSI. SignPath doesn't sign or notarize macOS
apps.

## Updates are signed separately

If Anvil adopts the Tauri updater, updates carry a signature from a key
generated with `tauri signer generate` (`TAURI_SIGNING_PRIVATE_KEY`). That's
independent of the OS signatures above and is computed after them. Keep the
private key backed up offline: without it, installed copies can never update
again ([Tauri docs](https://v2.tauri.app/plugin/updater/)).
