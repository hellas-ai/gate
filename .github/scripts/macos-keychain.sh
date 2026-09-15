#!/usr/bin/env bash
set -euo pipefail

: "${RUNNER_TEMP:?This script requires a GitHub Actions runner}"
keychain="$RUNNER_TEMP/gate-signing.keychain-db"
certificate="$RUNNER_TEMP/gate-signing.p12"
search_list="$RUNNER_TEMP/gate-keychain-search-list.txt"

case "${1:-}" in
  setup)
    : "${APPLE_CERTIFICATE:?Missing APPLE_CERTIFICATE}"
    : "${APPLE_CERTIFICATE_PASSWORD:?Missing APPLE_CERTIFICATE_PASSWORD}"
    : "${APPLE_SIGNING_IDENTITY:?Missing APPLE_SIGNING_IDENTITY}"
    : "${KEYCHAIN_PASSWORD:?Missing KEYCHAIN_PASSWORD}"
    [[ "$APPLE_SIGNING_IDENTITY" == "Developer ID Application:"* ]]
    umask 077
    /usr/bin/security list-keychains -d user > "$search_list"
    printf '%s' "$APPLE_CERTIFICATE" | /usr/bin/base64 --decode > "$certificate"
    /usr/bin/security create-keychain -p "$KEYCHAIN_PASSWORD" "$keychain"
    /usr/bin/security set-keychain-settings -lut 21600 "$keychain"
    /usr/bin/security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$keychain"
    /usr/bin/security import "$certificate" -P "$APPLE_CERTIFICATE_PASSWORD" \
      -k "$keychain" -t cert -f pkcs12 -T /usr/bin/codesign
    /usr/bin/security set-key-partition-list -S apple-tool:,apple:,codesign: \
      -s -k "$KEYCHAIN_PASSWORD" "$keychain"
    keychains=("$keychain")
    while IFS= read -r previous; do
      keychains+=("$previous")
    done < <(sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//' "$search_list")
    /usr/bin/security list-keychains -d user -s "${keychains[@]}"
    /usr/bin/security find-identity -v -p codesigning "$keychain"
    rm -f "$certificate"
    ;;
  cleanup)
    if [[ -f "$search_list" ]]; then
      keychains=()
      while IFS= read -r previous; do
        keychains+=("$previous")
      done < <(sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//' "$search_list")
      /usr/bin/security list-keychains -d user -s "${keychains[@]}"
    fi
    if [[ -f "$keychain" ]]; then
      /usr/bin/security delete-keychain "$keychain"
    fi
    rm -f "$certificate"
    ;;
  *)
    echo "Usage: macos-keychain.sh setup|cleanup" >&2
    exit 1
    ;;
esac
