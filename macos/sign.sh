#!/bin/sh
set -eu

profile=${1:?usage: macos/sign.sh PROFILE APP}
app=${2:?usage: macos/sign.sh PROFILE APP}
identity=${SIGN_IDENTITY:-}
contents="$app/Contents"

test -f "$profile"
test -d "$contents"
test -f "$contents/Info.plist"

temporary=$(mktemp -d /tmp/hellas-gate-sign.XXXXXX)
trap 'rm -r "$temporary"' EXIT HUP INT TERM
decoded="$temporary/profile.plist"
entitlements="$temporary/entitlements.plist"

security cms -D -i "$profile" >"$decoded"
plutil -extract Entitlements xml1 -o "$entitlements" "$decoded"

application_id=$(/usr/libexec/PlistBuddy \
    -c 'Print :com.apple.application-identifier' "$entitlements")
profile_bundle_id=${application_id#*.}
app_bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
    "$contents/Info.plist")
if [ "$profile_bundle_id" != "$app_bundle_id" ]; then
    echo "error: profile is for $profile_bundle_id, app is $app_bundle_id" >&2
    exit 1
fi

if /usr/libexec/PlistBuddy -c 'Print :com.apple.developer.devicecheck.app-attest-opt-in' \
    "$entitlements" >/dev/null 2>&1; then
    if [ "$(/usr/libexec/PlistBuddy \
        -c 'Print :com.apple.developer.devicecheck.app-attest-opt-in:0' \
        "$entitlements")" != CDhash ]; then
        echo "error: App Attest entitlement must include CDhash" >&2
        exit 1
    fi
fi

# A profile is an entitlement allowlist, not the exact entitlement to sign.
# Accept only an explicit domain or Apple wildcard grant, then request exactly
# this app's relying party in the resulting signature.
python3 - "$entitlements" <<'PYTHON'
import plistlib
import sys
path = sys.argv[1]
with open(path, "rb") as source:
    entitlements = plistlib.load(source)
key = "com.apple.developer.associated-domains"
allowed = entitlements.get(key)
if not (allowed == "*" or (
    isinstance(allowed, list) and any(
        entry in ("*", "webcredentials:hellas.ai") for entry in allowed
    )
)):
    raise SystemExit("error: profile does not authorize webcredentials:hellas.ai")
entitlements[key] = ["webcredentials:hellas.ai"]
with open(path, "wb") as output:
    plistlib.dump(entitlements, output)
PYTHON

for entitlement in \
    com.apple.security.get-task-allow \
    com.apple.security.cs.allow-jit \
    com.apple.security.cs.allow-dyld-environment-variables \
    com.apple.security.cs.disable-library-validation \
    com.apple.security.cs.allow-unsigned-executable-memory \
    com.apple.security.cs.disable-executable-page-protection \
    com.apple.security.cs.debugger
do
    if /usr/libexec/PlistBuddy -c "Print :$entitlement" \
        "$entitlements" >/dev/null 2>&1
    then
        echo "error: forbidden runtime-relaxation entitlement: $entitlement" >&2
        exit 1
    fi
done

if [ -z "$identity" ]; then
    identity=$(security find-identity -v -p codesigning \
        | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' \
        | head -n 1)
fi
test -n "$identity"

cp "$profile" "$contents/embedded.provisionprofile"
codesign --force --options runtime --timestamp --entitlements "$entitlements" \
    --sign "$identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
signed_entitlements="$temporary/signed-entitlements.plist"
codesign --display --entitlements :- "$app" >"$signed_entitlements" 2>/dev/null
if [ "$(/usr/libexec/PlistBuddy \
    -c 'Print :com.apple.developer.associated-domains:0' \
    "$signed_entitlements")" != webcredentials:hellas.ai ]; then
    echo "error: signed app does not contain the required passkey domain" >&2
    exit 1
fi
