#!/usr/bin/env bash
set -euo pipefail

: "${CARGO_TARGET_DIR:?Missing CARGO_TARGET_DIR}"
: "${RUNNER_TEMP:?This script requires a GitHub Actions runner}"
: "${APPLE_SIGNING_IDENTITY:?Missing APPLE_SIGNING_IDENTITY}"
: "${APPLE_ID:?Missing APPLE_ID}"
: "${APPLE_PASSWORD:?Missing APPLE_PASSWORD}"
: "${APPLE_TEAM_ID:?Missing APPLE_TEAM_ID}"
: "${GITHUB_REF_NAME:?Missing release tag}"

version=$(python3 .github/scripts/release-version.py "$GITHUB_REF_NAME")
app="$CARGO_TARGET_DIR/aarch64-apple-darwin/release/bundle/macos/Hellas Gate.app"
release_dir=$(mktemp -d "$RUNNER_TEMP/gate-release.XXXXXX")
staging=$(mktemp -d "$RUNNER_TEMP/gate-dmg.XXXXXX")
mountpoint=$(mktemp -d "$RUNNER_TEMP/gate-mount.XXXXXX")
extraction=$(mktemp -d "$RUNNER_TEMP/gate-zip.XXXXXX")
archive="$release_dir/Hellas-Gate_${version}_aarch64.app.zip"
dmg="$release_dir/Hellas-Gate_${version}_aarch64.dmg"
mounted=false

cleanup() {
  if [[ "$mounted" == true ]]; then
    /usr/bin/hdiutil detach "$mountpoint" || true
  fi
}
trap cleanup EXIT

notarize() {
  local artifact=$1
  local report=$2
  if ! /usr/bin/xcrun notarytool submit "$artifact" \
    --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" \
    --wait --timeout 30m --output-format json > "$report"; then
    cat "$report"
    return 1
  fi
  cat "$report"
  if [[ "$(/usr/bin/plutil -extract status raw -o - "$report")" != Accepted ]]; then
    local submission
    submission=$(/usr/bin/plutil -extract id raw -o - "$report")
    /usr/bin/xcrun notarytool log "$submission" \
      --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true
    return 1
  fi
}

bash .github/scripts/verify-macos-app.sh "$app"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
notarize "$archive" "$release_dir/app-notarization.json"
/usr/bin/xcrun stapler staple "$app"
bash .github/scripts/verify-macos-app.sh "$app" --notarized

# Build the disk image from the stapled app so both delivery formats work offline.
/usr/bin/ditto "$app" "$staging/Hellas Gate.app"
ln -s /Applications "$staging/Applications"
/usr/bin/hdiutil create -volname 'Hellas Gate' -srcfolder "$staging" -format UDZO "$dmg"
/usr/bin/codesign --force --sign "$APPLE_SIGNING_IDENTITY" --timestamp "$dmg"
/usr/bin/codesign --verify --strict --verbose=2 "$dmg"
notarize "$dmg" "$release_dir/dmg-notarization.json"
/usr/bin/xcrun stapler staple "$dmg"
/usr/bin/xcrun stapler validate "$dmg"
/usr/sbin/spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
/usr/bin/hdiutil verify "$dmg"
/usr/bin/hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$mountpoint"
mounted=true
bash .github/scripts/verify-macos-app.sh "$mountpoint/Hellas Gate.app" --notarized
/usr/bin/hdiutil detach "$mountpoint"
mounted=false

# Recreate the zip after stapling; ditto preserves executable modes and symlinks.
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
/usr/bin/ditto -x -k "$archive" "$extraction"
bash .github/scripts/verify-macos-app.sh "$extraction/Hellas Gate.app" --notarized
test -s "$dmg"
test -s "$archive"
printf 'GATE_RELEASE_DIR=%s\n' "$release_dir" >> "$GITHUB_ENV"
