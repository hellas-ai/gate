# macOS signing, passkeys and App Attest

Gate's App Attest producer is intentionally owned by the signed application,
not by `hellas-attestation` or `hellas-cli`.

The default Tauri build has no associated-domain or restricted App Attest
entitlement. It can launch for development but cannot use native passkeys or
App Attest. Native passkeys require macOS 14 or later, the app bundle ID
`ai.hellas.gate`, and a signing profile that authorizes the
`webcredentials:hellas.ai` associated domain. This does not require App Attest.

The optional attested provider additionally requires macOS 27, an executable
linked against the macOS 27 SDK or newer, and the
`com.apple.developer.devicecheck.app-attest-opt-in` entitlement with value
`CDhash` in the provisioning profile. Gate rejects incomplete App Attest evidence.

The Nix shell uses the Xcode selected by `xcode-select`. Set
`GATE_DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer` before
`nix develop` to select another installation. An executable linked against an
older SDK can receive evidence without CDHash extensions; Gate rejects it.

Build the app ad-hoc first, then embed the provisioning profile and apply the
final Developer ID signature in one validated step:

```sh
APPLE_SIGNING_IDENTITY=- cargo tauri build
./macos/sign.sh path/to/gate.provisionprofile \
  target/release/bundle/macos/Hellas\ Gate.app
```

`macos/sign.sh` embeds the profile and signs with the entitlements extracted
from it, narrowing the associated-domains entitlement to `webcredentials:hellas.ai`.
It accepts a passkey-only profile and checks `CDhash` when App Attest is present.
It rejects a profile whose application identifier does not match the app bundle identifier and rejects entitlements that relax hardened runtime or
library validation. Set `SIGN_IDENTITY` only when more than one Developer ID
Application identity is installed. Do not add the restricted entitlement to
the default Tauri configuration: macOS refuses to launch such a bundle when it
lacks a matching profile.

The release workflow requires the `APPLE_PROVISION_PROFILE_BASE64` GitHub secret.
It decodes the profile into the runner's temporary directory, signs the app with
`macos/sign.sh` before notarization, and removes the profile during cleanup. A
missing or incompatible profile fails the release before publication.

Gate checks `DCAppAttestService` at runtime and reports unavailable when Apple
rejects the build or machine. It does not fall back to a software root.

App Attest keys are created and retained by Apple's service. Gate keeps the
opaque key reference in its OS secret store and the canonical public enrollment
in its private application data directory. Reuse verifies the enrollment against
the installation's distinct node signer and transport key.

## Native passkey association

The signed app needs the Associated Domains capability for `webcredentials:hellas.ai`.
The website must serve `https://hellas.ai/.well-known/apple-app-site-association`
as JSON with a `webcredentials.apps` array containing the actual application
identifier from the signing profile: `<Application Identifier Prefix>.ai.hellas.gate`.
Use the prefix from the profile rather than guessing a team identifier. The file
must be publicly reachable over HTTPS without a redirect; Apple may cache it via
its associated-domains CDN. A local entitlement check cannot establish that the
association has propagated or that an Apple Account and iCloud Keychain are ready.
Consult [Apple's passkey setup](https://developer.apple.com/documentation/authenticationservices/connecting-to-a-service-with-passkeys)
and [associated domains documentation](https://developer.apple.com/documentation/xcode/supporting-associated-domains).

## Native acceptance

Platform compilation does not prove a provisioned App Attest enrollment or a
native passkey prompt. On a signed and associated macOS app, register a passkey,
confirm user verification, restart, and approve an assertion for the same owner;
check cancellation and unavailable-association errors. Verify the returned client
data origin and RP ID through the application checks. Never use software fixtures
as evidence of user verification or private execution.

For a provisioned runtime check, launch the signed application, inspect the
reported capability and bind the Apple provider through the authenticated local
control session. Compare the independently obtained app-ID/CDHash and provider
genesis pins on a second installation, perform verified work, restart, and verify
that the persisted enrollment and replay counters are reused. A wrong app-ID,
CDHash, environment, provider genesis or replayed assertion must refuse without
software fallback. Keep the signed application's logs and exact provisioning
metadata with the acceptance evidence; no external API request is implied by
binding alone.
