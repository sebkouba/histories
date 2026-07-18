# Releasing Histories

Source releases and downloadable macOS binaries have different trust requirements.
Do not publish an ad-hoc-signed `.app` as a binary release.

## Prerequisites

- Apple Developer Program membership
- A `Developer ID Application` certificate exported as a password-protected
  PKCS#12 file
- Apple notarization credentials
- A clean checkout with all CI checks passing

The GitHub release workflow requires these repository secrets:

- `APPLE_CERTIFICATE`: base64-encoded PKCS#12 certificate
- `APPLE_CERTIFICATE_PASSWORD`
- `KEYCHAIN_PASSWORD`: an arbitrary strong password for the temporary CI keychain
- `APPLE_ID`
- `APPLE_PASSWORD`: an app-specific Apple ID password
- `APPLE_TEAM_ID`

## Version and release

1. Update the version in `Cargo.toml`, `package.json`, and
   `src-tauri/tauri.conf.json`.
2. Update `CHANGELOG.md` and run the complete verification suite.
3. Commit the release, tag it as `vX.Y.Z`, and push the tag.
4. The release workflow builds native Apple Silicon and Intel artifacts, signs
   them with Developer ID, submits them for notarization, and creates a draft
   GitHub release.
5. Download both draft artifacts and validate them on clean Macs before
   publishing the release.

Validation commands:

```bash
codesign --verify --deep --strict --verbose=2 Histories.app
codesign -dvvv --entitlements :- Histories.app
spctl --assess --type execute --verbose=4 Histories.app
xcrun stapler validate Histories.app
```

The signature should show `Developer ID Application`, a Team ID, and the runtime
flag. `spctl` and `stapler` must both accept the artifact.
