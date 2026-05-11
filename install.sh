#!/bin/sh
# Kryos Language Installer
#
# Public-repo usage:
#   curl -fsSL https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.sh | sh
#
# Private-repo usage (auth required for downloads):
#   GITHUB_TOKEN=<your-PAT> curl -fsSL <url> | sh
#
# Environment variables:
#   KRYOS_INSTALL_DIR  -- override install prefix (default: ~/.kryos)
#   KRYOS_VERSION      -- pin a specific release tag (default: latest)
#   GITHUB_TOKEN       -- PAT for downloading release assets from private repos
#   GH_TOKEN           -- alternative auth env var (gh CLI compatible)

set -e

REPO="NORTHTEKDevs/kryos-lang"
INSTALL_DIR="${KRYOS_INSTALL_DIR:-$HOME/.kryos}"
BIN_DIR="$INSTALL_DIR/bin"

# Auth header for private-repo access.
AUTH_HEADER=""
if [ -n "${GITHUB_TOKEN:-}" ]; then
    AUTH_HEADER="Authorization: Bearer ${GITHUB_TOKEN}"
elif [ -n "${GH_TOKEN:-}" ]; then
    AUTH_HEADER="Authorization: Bearer ${GH_TOKEN}"
fi

curl_auth() {
    if [ -n "$AUTH_HEADER" ]; then
        curl -fsSL -H "$AUTH_HEADER" "$@"
    else
        curl -fsSL "$@"
    fi
}

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

# Resolve release tag.
if [ -n "${KRYOS_VERSION:-}" ]; then
    TAG="$KRYOS_VERSION"
    echo "Installing pinned version: $TAG"
else
    TAG=$(curl_auth "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
        | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$TAG" ]; then
        echo "Error: could not determine latest release."
        echo "If the repo is private, set GITHUB_TOKEN or GH_TOKEN to a PAT with 'repo' scope."
        exit 1
    fi
    echo "Latest release: $TAG"
fi

# Download release asset. For private repos GitHub requires the
# Accept header on the asset URL itself.
URL="https://github.com/$REPO/releases/download/$TAG/${ARTIFACT}.tar.gz"
echo "Downloading $URL..."
TMPDIR=$(mktemp -d)
if [ -n "$AUTH_HEADER" ]; then
    curl -fsSL -H "$AUTH_HEADER" -H "Accept: application/octet-stream" "$URL" -o "$TMPDIR/kryos.tar.gz"
else
    curl -fsSL "$URL" -o "$TMPDIR/kryos.tar.gz"
fi

# Extract
mkdir -p "$BIN_DIR"
mkdir -p "$INSTALL_DIR/stdlib"
mkdir -p "$INSTALL_DIR/lib"
tar xzf "$TMPDIR/kryos.tar.gz" -C "$TMPDIR"
cp "$TMPDIR/kryos" "$BIN_DIR/kryos"
chmod +x "$BIN_DIR/kryos"

# Copy stdlib if present
if [ -d "$TMPDIR/stdlib" ]; then
    cp -R "$TMPDIR/stdlib/." "$INSTALL_DIR/stdlib/"
fi

# Copy runtime static libs if shipped alongside the binary -- the kryos
# compiler looks for these at <prefix>/../lib/libkryos_rt.a relative to
# the binary location.
for lib in libkryos_rt.a libkryos_stdlib_native.a; do
    if [ -f "$TMPDIR/$lib" ]; then
        cp "$TMPDIR/$lib" "$INSTALL_DIR/lib/$lib"
    fi
done

# Cleanup
rm -rf "$TMPDIR"

echo ""
echo "Kryos $TAG installed to $BIN_DIR/kryos"
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
