#!/usr/bin/env bash
# Sign + notarize a macOS forge-osh binary.
# Required env: APPLE_ID, APPLE_TEAM_ID, APPLE_PASSWORD,
#               MAC_CERT_BASE64, MAC_CERT_PASSWORD
set -euo pipefail

BIN="$1"
if [ -z "${BIN:-}" ]; then
  echo "usage: $0 <path-to-binary>" >&2
  exit 1
fi

# Decode + import the Developer ID cert into a temp keychain.
echo "$MAC_CERT_BASE64" | base64 -d > cert.p12
security create-keychain -p ci ci.keychain
security default-keychain -s ci.keychain
security unlock-keychain -p ci ci.keychain
security import cert.p12 -k ci.keychain -P "$MAC_CERT_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k ci ci.keychain

codesign --force --options=runtime --timestamp \
  --sign "Developer ID Application: $APPLE_TEAM_ID" "$BIN"

# Notarize (Apple service).
ZIP="$BIN.zip"
ditto -c -k --keepParent "$BIN" "$ZIP"
xcrun notarytool submit "$ZIP" \
  --apple-id "$APPLE_ID" \
  --team-id "$APPLE_TEAM_ID" \
  --password "$APPLE_PASSWORD" \
  --wait

xcrun stapler staple "$BIN" || true
rm -f cert.p12 "$ZIP"
echo "signed + notarized: $BIN"
