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

App Attest keys are created and retained by Apple's service. Gate persists only
the opaque key identifier and the canonical Hellas enrollment bundle in its
private application data directory. A persisted enrollment is fully verified
against the current Gate transport/caller identity before it is reused.

## Native grant-session smoke test

`state::tests::provisioned_provider_opens_pinned_grant_sessions_after_restart`
is ignored by normal tests because it calls Apple's enrollment service. Build
Gate's library test executable, place it at `Contents/MacOS/hellas-gate` in a
copy of the app bundle, and sign that bundle with the same provisioning profile
and hardened-runtime entitlements as Gate. Set `HELLAS_GATE_TEST_APP_ID` to the
profile's application identifier and `HELLAS_GATE_TEST_CDHASH` to the signed
test executable's full SHA-256 CodeDirectory hash. Run that test with `--ignored`.
It opens an authenticated remote grant session, restarts the provider, and opens
another session using the persisted enrollment. It uses temporary state and
makes no requests to the configured OpenAI backend.

The SDK test
`provider::grant_tests::contact_offer_open_tls_responses_gateway_and_revocation_preserve_quota`
separately exercises real local TLS requests, verified responses, quota exhaustion,
restart and revocation on each native platform without external API credentials.
