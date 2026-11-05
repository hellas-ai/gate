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
The Mac must also use Full Security with System Integrity Protection enabled.
The known `ai.hellas.gate` passkey profile does not authorize App Attest; a
profile for a differently named spike app is not a substitute. The shipping
attested sharing journey remains untested. See [the privacy boundary](../PRIVACY.md).

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

Install the signed app under `~/Applications` or `/Applications` and launch it
through LaunchServices before testing passkeys. Directly running a build under
`/tmp` can leave the app absent from macOS's associated-domain database even
when its signature and the website association are correct.

Use a fresh data directory to keep an acceptance identity separate from an
existing Gate installation. For example:

```sh
open -n "$HOME/Applications/Hellas Gate Native Acceptance.app" \
  --env "HELLAS_GATE_DATA_DIR=$TMPDIR/gate-native-acceptance" \
  --env RUST_LOG=hellas_gate=debug
```

Keep the same directory for restart and recovery checks. The directory also
scopes the SDK secret-store namespace; the default app data is not migrated or
removed. Do not run two Gate processes against the same data directory.

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

## Debugging the setup flow

Launch the installed app with `RUST_LOG=hellas_gate=debug` to log setup button
clicks, validation failures, submission, approval-dialog decisions and native
registration results. Frontend diagnostics contain fixed event labels only,
without form values, private keys or passkey assertion bytes. Capture stdout and
stderr to a private file alongside the isolated acceptance directory.

## Unattended signing on the Mac (one-time user setup, about 10 minutes)

**UNTESTED:** this dedicated-keychain procedure must pass the plain SSH probe
below before unattended signing is established. It does not enable App Attest on
unsupported hardware or replace the provisioning profile.

**USER:** in Keychain Access, export only the Developer ID Application identity
(certificate **and private key**) to a password-protected `.p12`, kept on the Mac.
Create the real `ai.hellas.gate` profile with `app-attest-opt-in = [CDhash]` and
`webcredentials:hellas.ai`; the existing passkey profile is insufficient.
Do not send the export or its passwords through chat or commit them.

**USER:** in a private bash terminal on the Mac, with shell tracing disabled:

```bash
set +x
umask 077
mkdir -p "$HOME/.gate-signing"
kc="$HOME/Library/Keychains/gate-signing.keychain-db"
security create-keychain "$kc" # prompts for a NEW dedicated keychain password
security unlock-keychain "$kc" # prompts for that password
security import "$HOME/.gate-signing/developer-id.p12" -k "$kc" -T /usr/bin/codesign
# import prompts for the export password; never use -A (allow every app)
read -r -s -p 'Dedicated keychain password: ' gate_keychain_password
printf '\n'
security set-key-partition-list -S apple-tool:,apple:,codesign: -s \
  -k "$gate_keychain_password" "$kc"
# USER approves retaining this unlock secret for unattended use on this Mac.
printf '%s' "$gate_keychain_password" > "$HOME/.gate-signing/keychain-password"
chmod 600 "$HOME/.gate-signing/keychain-password"
unset gate_keychain_password
security set-keychain-settings -lut 21600 "$kc"
security find-identity -v -p codesigning "$kc"
```

`security import` and partition access apply to the dedicated keychain. Leave
the login/default keychain and its search list alone. The password file enables
the logged-in user (including that user's agents) to unlock this signing key;
keep it local, owner-only, and remove it when unattended signing is no longer
wanted. `security` necessarily receives passwords as process arguments: do not
trace these commands or run them in a shared diagnostic session.

**AGENT, after setup:** set `SIGNING_MAC` to your SSH destination and replace
the example Developer ID label below with your identity. Run over **plain SSH**;
the keychain password stays on the signing Mac:

```bash
ssh -T "$SIGNING_MAC" 'bash -s' <<'REMOTE'
set -eu
set +x
kc="$HOME/Library/Keychains/gate-signing.keychain-db"
security unlock-keychain -p "$(cat "$HOME/.gate-signing/keychain-password")" "$kc"
security find-identity -v -p codesigning "$kc"
probe=$(mktemp -d)
trap 'rm -rf "$probe"' EXIT
cp /usr/bin/true "$probe/gate-signing-probe"
codesign --force --options runtime --timestamp --keychain "$kc" \
  --sign 'Developer ID Application: YOUR NAME (TEAMID)' \
  "$probe/gate-signing-probe"
codesign --verify --strict --verbose=2 "$probe/gate-signing-probe"
"$probe/gate-signing-probe"
REMOTE
```

Expected outcome, **not a captured result yet**: the identity is valid,
`codesign --verify` succeeds, and the probe exits zero without a prompt. Record
actual output. Then sign the provisioned Gate bundle with the same explicit
keychain and identity and run the live test:

```sh
SIGN_KEYCHAIN="$HOME/Library/Keychains/gate-signing.keychain-db" \
SIGN_IDENTITY='Developer ID Application: YOUR NAME (TEAMID)' \
./macos/sign.sh /path/to/gate.provisionprofile \
  'target/release/bundle/macos/Hellas Gate.app'
```

The probe alone proves only SSH
access to the signing key. Unlock before each automated job; the six-hour
keychain timeout is not a guarantee that the Mac is awake or reachable.

Apple documents the App Attest server checks in
[Validating apps that connect to your server](https://developer.apple.com/documentation/devicecheck/validating-apps-that-connect-to-your-server).
The device certificate chain, signed challenge and build evidence are security
checks; captured objects are regression inputs, not an alternative trust root.

## Hardware probe without UI interaction

**Hardware execution UNTESTED.** After the signing setup above, this ignored
probe checks enrollment, key reload, counters and wrong-challenge rejection.
Its test executable has its own CDHash; it does not test paid sharing. Set
`APP_ID_PREFIX` from the profile application identifier, which may differ from
the Team ID, and replace the example Developer ID label.

```sh
GATE_DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer nix develop
cargo test --locked --lib --no-run --message-format=json > target/native-artifacts.json
probe="$PWD/target/app-attest-probe/Hellas Gate Tests.app"
mkdir -p "$probe/Contents/MacOS"
python3 - "$probe" <<'PY'
import json, pathlib, plistlib, shutil, sys
artifacts = [json.loads(line) for line in open('target/native-artifacts.json')]
binaries = [a['executable'] for a in artifacts
            if a.get('reason') == 'compiler-artifact' and a.get('executable')
            and a.get('profile', {}).get('test')]
assert len(binaries) == 1, binaries
probe = pathlib.Path(sys.argv[1])
shutil.copy2(binaries[0], probe / 'Contents/MacOS/hellas-gate')
(probe / 'Contents/Info.plist').write_bytes(plistlib.dumps({
    'CFBundleIdentifier': 'ai.hellas.gate', 'CFBundleExecutable': 'hellas-gate',
    'CFBundleName': 'Hellas Gate Tests', 'CFBundlePackageType': 'APPL',
    'CFBundleVersion': '1', 'LSMinimumSystemVersion': '27.0',
}))
PY
SIGN_KEYCHAIN="$HOME/Library/Keychains/gate-signing.keychain-db" \
SIGN_IDENTITY='Developer ID Application: YOUR NAME (TEAMID)' \
./macos/sign.sh /path/to/gate.provisionprofile "$probe"
cdhash=$(codesign --display --verbose=4 "$probe" 2>&1 \
  | sed -n 's/^CandidateCDHashFull sha256=//p')
HELLAS_GATE_TEST_APP_ID="${APP_ID_PREFIX:?set the profile App ID prefix}.ai.hellas.gate" \
HELLAS_GATE_TEST_CDHASH="$cdhash" \
"$probe/Contents/MacOS/hellas-gate" \
  provisioned_apple_root_verifies_live_assertions_after_reload --ignored --nocapture
```

B's sharing policy must pin the shipping executable, never this test hash.
