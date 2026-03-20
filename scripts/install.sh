#!/bin/bash
set -e

REPO="osh/forge-osh"
BINARY="forge-osh"
INSTALL_DIR="${FORGE_INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS-$ARCH" in
  linux-x86_64)   ARTIFACT="forge-osh-linux-x86_64" ;;
  darwin-x86_64)  ARTIFACT="forge-osh-macos-x86_64" ;;
  darwin-arm64)   ARTIFACT="forge-osh-macos-arm64" ;;
  *)              echo "Unsupported platform: $OS-$ARCH"; exit 1 ;;
esac

# Get latest release URL
echo "Detecting latest release..."
RELEASE_URL=$(curl -s https://api.github.com/repos/$REPO/releases/latest \
  | grep "browser_download_url.*$ARTIFACT" \
  | cut -d '"' -f 4)

if [ -z "$RELEASE_URL" ]; then
  echo "Could not find release for $ARTIFACT"
  exit 1
fi

echo "Downloading $BINARY..."
curl -L "$RELEASE_URL" -o "/tmp/$BINARY"
chmod +x "/tmp/$BINARY"

echo "Installing to $INSTALL_DIR/$BINARY..."
sudo mv "/tmp/$BINARY" "$INSTALL_DIR/$BINARY"

echo ""
echo "Installed! Run: $BINARY"
echo "First-time setup: $BINARY config keys set anthropic YOUR_API_KEY"
