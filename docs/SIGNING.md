# macOS signing and App Attest

Gate's App Attest producer is intentionally owned by the signed application,
not by `hellas-attestation` or `hellas-cli`.

The default Tauri build deliberately has no restricted App Attest entitlement.
It is the normal, launchable desktop app and reports App Attest as unavailable.
It may be signed with Developer ID in the usual way without a provisioning
profile.

For the attested provider to start, a provisioned build must use:

- bundle identifier `ai.hellas.gate`;
- an Apple provisioning profile whose App ID matches that identifier; and
- the `com.apple.developer.devicecheck.app-attest-opt-in` entitlement with the
  value `CDhash` in that profile.

Build the app ad-hoc first, then embed the provisioning profile and apply the
final Developer ID signature in one validated step:

```sh
APPLE_SIGNING_IDENTITY=- cargo tauri build
./macos/sign.sh path/to/gate.provisionprofile \
  /tmp/hellas-gate-target/release/bundle/macos/Hellas\ Gate.app
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

App Attest keys are created and retained by Apple's service. Gate persists only
the opaque key identifier and the canonical Hellas enrollment bundle in its
private application data directory. A persisted enrollment is fully verified
against the current Gate transport/caller identity before it is reused.
