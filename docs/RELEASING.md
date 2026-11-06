# Releasing

1. Bump the version everywhere (`Cargo.toml`, `crates/daemon/Cargo.toml` path deps,
   `apps/desktop/src-tauri/{Cargo.toml,tauri.conf.json}`, `apps/*/package.json`).
2. Commit, wait for CI, then tag: `git tag -a v0.1.0 -m "Bantu 0.1.0" && git push origin v0.1.0`.
   Tags with a suffix (`v0.1.0-alpha.4`) become pre-releases.

The release workflow builds `bantud` for macOS and Linux, plus the Mac app (DMG), and
publishes them with `SHA256SUMS`.

## Signing and notarizing the Mac app

Without these secrets the app is ad-hoc signed, and users must clear the quarantine flag
after installing. With them, it's signed with your Developer ID and notarized by Apple, so it
opens normally. Secrets go straight into GitHub (`gh secret set` prompts without echoing);
never paste them into chats or issues.

**1. Developer ID Application certificate** (once; needs the Account Holder role)

- Apple Developer → Certificates → **+** → *Developer ID Application* (not "Apple
  Development"). Create it from a CSR made in Keychain Access, then download and open it.
- Keychain Access → My Certificates → right-click it → Export → `bantu-devid.p12` with a
  strong password.

```sh
R=Sreenivas1323/bantu
base64 -i bantu-devid.p12 | gh secret set APPLE_CERTIFICATE -R $R
gh secret set APPLE_CERTIFICATE_PASSWORD -R $R      # the export password
security find-identity -v -p codesigning            # copy the "Developer ID Application: … (TEAMID)" name
gh secret set APPLE_SIGNING_IDENTITY -R $R
rm bantu-devid.p12
```

**2. App Store Connect API key for notarization**

- App Store Connect → Users and Access → Integrations → App Store Connect API → Team
  Keys → **+**, role *Developer*. Download `AuthKey_<KEYID>.p8` (it can only be downloaded once).

```sh
base64 -i AuthKey_<KEYID>.p8 | gh secret set APPLE_API_PRIVATE_KEY -R $R
gh secret set APPLE_API_KEY -R $R       # the key ID
gh secret set APPLE_API_ISSUER -R $R    # the issuer ID shown above the keys list
```

The next tagged release is then signed and notarized.

## Launch Stack templates

The Launch Stack links in [AWS.md](AWS.md) point at copies of the templates in the public
bucket `bantu-templates-050451378836` (us-east-1), which allows anonymous reads of `*.yaml`
only. When a template under `deploy/aws/` changes, update the copies:

```sh
for f in deploy/aws/*.yaml; do
  aws s3 cp --content-type text/yaml "$f" "s3://bantu-templates-050451378836/$(basename "$f")"
done
```

To have each release do this instead:

1. Create a bucket that allows public reads of `*.yaml`.
2. Create an IAM role GitHub Actions can assume through OIDC, allowed to `s3:PutObject` on it.
3. In the repository's Actions settings, set the variable `AWS_TEMPLATE_BUCKET` and the secret
   `AWS_TEMPLATE_ROLE_ARN`. You can also set `AWS_TEMPLATE_REGION` (default `us-east-1`).

Without the variable, the release skips this step.
