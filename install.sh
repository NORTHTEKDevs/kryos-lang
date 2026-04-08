#!/bin/sh
# Kryos Language Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/FrostbyteDevTeam/kryos-lang/master/install.sh | sh

set -e

REPO="FrostbyteDevTeam/kryos-lang"
INSTALL_DIR="${KRYOS_INSTALL_DIR:-$HOME/.kryos}"
BIN_DIR="$INSTALL_DIR/bin"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)   PLATFORM="linux" ;;
    Darwin)  PLATFORM="macos" ;;
    *)       echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *)             echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

ARTIFACT="kryos-${PLATFORM}-${ARCH}"

echo "Installing Kryos ($PLATFORM-$ARCH)..."

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST" ]; then
    echo "Error: could not determine latest release"
    exit 1
fi
echo "Latest release: $LATEST"

# Download
URL="https://github.com/$REPO/releases/download/$LATEST/${ARTIFACT}.tar.gz"
echo "Downloading $URL..."
TMPDIR=$(mktemp -d)
curl -fsSL "$URL" -o "$TMPDIR/kryos.tar.gz"

# Extract
mkdir -p "$BIN_DIR"
mkdir -p "$INSTALL_DIR/stdlib"
tar xzf "$TMPDIR/kryos.tar.gz" -C "$TMPDIR"
cp "$TMPDIR/kryos" "$BIN_DIR/kryos"
chmod +x "$BIN_DIR/kryos"

# Copy stdlib if present
if [ -d "$TMPDIR/stdlib" ]; then
    cp -r "$TMPDIR/stdlib/"* "$INSTALL_DIR/stdlib/"
fi

# Cleanup
rm -rf "$TMPDIR"

echo ""
echo "Kryos $LATEST installed to $BIN_DIR/kryos"
echo ""

# Check PATH
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        echo "Add Kryos to your PATH by adding this to your shell profile:"
        echo ""
        echo "  export PATH=\"$BIN_DIR:\$PATH\""
        echo ""
        ;;
esac

echo "Run 'kryos --version' to verify the installation."
