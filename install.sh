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
#   KRYOS_VERSION      -- pin a specific release tag (default: v1.0.0-beta.1)
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

# Resolve release tag. GitHub's releases/latest is NOT used directly: the
# repo carries legacy v2.x/v4.x tags that outrank v1.0.0 semantically. Query
# the release list and take the newest v1.0.0* release by publish order,
# falling back to a pinned floor if the API is unreachable. Override with
# KRYOS_VERSION to install any specific tag.
FALLBACK_VERSION="v1.0.0-beta.7"
if [ -n "${KRYOS_VERSION:-}" ]; then
    TAG="$KRYOS_VERSION"
    echo "Installing pinned version: $TAG"
else
    TAG=$(curl -fsSL ${AUTH_HEADER:+-H "$AUTH_HEADER"}         "https://api.github.com/repos/$REPO/releases?per_page=30" 2>/dev/null         | grep -o '"tag_name": *"v1.0.0[^"]*"'         | head -1 | grep -o 'v1.0.0[^"]*')
    if [ -z "$TAG" ]; then
        TAG="$FALLBACK_VERSION"
        echo "Installing default version (API unavailable, pinned floor): $TAG"
    else
        echo "Installing latest 1.0 release: $TAG"
    fi
fi

# Download release asset. Private repos reject the browser download URL for
# API tokens -- the asset must be fetched through the API asset endpoint
# (api.github.com/.../releases/assets/<id>) with Accept: octet-stream.
TMPDIR=$(mktemp -d)
if [ -n "$AUTH_HEADER" ]; then
    ASSET_URL=$(curl_auth "https://api.github.com/repos/$REPO/releases/tags/$TAG"         | tr ',' '
'         | grep -B0 -A0 '"url"\|"name"'         | paste - -         | grep "\"name\": *\"${ARTIFACT}.tar.gz\""         | grep -o 'https://api.github.com/repos/[^"]*/assets/[0-9]*'         | head -1)
    if [ -z "$ASSET_URL" ]; then
        # Fallback: scan the raw JSON for the asset id adjacent to the name.
        ASSET_URL=$(curl_auth "https://api.github.com/repos/$REPO/releases/tags/$TAG"             | python3 -c "import sys, json; r = json.load(sys.stdin); print(next((a['url'] for a in r.get('assets', []) if a['name'] == '${ARTIFACT}.tar.gz'), ''))" 2>/dev/null || true)
    fi
    if [ -z "$ASSET_URL" ]; then
        echo "Error: release $TAG has no asset ${ARTIFACT}.tar.gz"
        exit 1
    fi
    echo "Downloading ${ARTIFACT}.tar.gz (API asset endpoint)..."
    curl -fsSL -H "$AUTH_HEADER" -H "Accept: application/octet-stream" "$ASSET_URL" -o "$TMPDIR/kryos.tar.gz"
else
    URL="https://github.com/$REPO/releases/download/$TAG/${ARTIFACT}.tar.gz"
    echo "Downloading $URL..."
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
