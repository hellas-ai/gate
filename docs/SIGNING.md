# macOS signing and App Attest

Gate's App Attest producer is intentionally owned by the signed application,
not by `hellas-attestation` or `hellas-cli`.

The default Tauri build deliberately has no restricted App Attest entitlement.
It is the normal, launchable desktop app and reports App Attest as unavailable.
It may be signed with Developer ID in the usual way without a provisioning
profile.

For the attested provider to start, a provisioned build must use:

- macOS 27 and an executable linked against the macOS 27 SDK or newer;
- bundle identifier `ai.hellas.gate`;
- an Apple provisioning profile whose App ID matches that identifier; and
- the `com.apple.developer.devicecheck.app-attest-opt-in` entitlement with the
  value `CDhash` in that profile.

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
from it. It rejects a profile whose application identifier does not match the
app bundle identifier and rejects entitlements that relax hardened runtime or
library validation. Set `SIGN_IDENTITY` only when more than one Developer ID
Application identity is installed. Do not add the restricted entitlement to
the default Tauri configuration: macOS refuses to launch such a bundle when it
lacks a matching profile.

Gate checks `DCAppAttestService` at runtime and reports unavailable when Apple
rejects the build or machine. It does not fall back to a software root.

App Attest keys are created and retained by Apple's service. Gate keeps the
opaque key reference in its OS secret store and the canonical public enrollment
in its private application data directory. Reuse verifies the enrollment against
the installation's distinct node signer and transport key.

## Native acceptance

Platform compilation does not prove a provisioned App Attest enrollment or a
native passkey prompt. The current authenticator is explicitly selected software
P-256 with user presence only; native passkey integration is not enabled. Never
use software fixtures as evidence of user verification or private execution.

For a provisioned runtime check, launch the signed application, inspect the
reported capability and bind the Apple provider through the authenticated local
control session. Compare the independently obtained app-ID/CDHash and provider
genesis pins on a second installation, perform verified work, restart, and verify
that the persisted enrollment and replay counters are reused. A wrong app-ID,
CDHash, environment, provider genesis or replayed assertion must refuse without
software fallback. Keep the signed application's logs and exact provisioning
metadata with the acceptance evidence; no external API request is implied by
binding alone.
