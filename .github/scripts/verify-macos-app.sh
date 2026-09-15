#!/usr/bin/env bash
set -euo pipefail

app=${1:?Usage: verify-macos-app.sh APP [--notarized]}
version=$(python3 .github/scripts/release-version.py)
plist="$app/Contents/Info.plist"
test -f "$plist"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")" == ai.hellas.gate ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist")" == "$version" ]]
executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$plist")
test -x "$app/Contents/MacOS/$executable"
[[ "$(/usr/bin/lipo -archs "$app/Contents/MacOS/$executable")" == arm64 ]]
/usr/bin/codesign --verify --deep --strict --verbose=2 "$app"

if [[ "${2:-}" == --notarized ]]; then
  : "${APPLE_TEAM_ID:?APPLE_TEAM_ID is required to verify a release}"
  details=$(/usr/bin/codesign --display --verbose=4 "$app" 2>&1)
  printf '%s\n' "$details"
  printf '%s\n' "$details" | grep -Fqx "TeamIdentifier=$APPLE_TEAM_ID"
  printf '%s\n' "$details" | grep -Eq '^CodeDirectory .*flags=.*runtime'
  /usr/bin/xcrun stapler validate "$app"
  /usr/sbin/spctl --assess --type execute --verbose=2 "$app"
fi
