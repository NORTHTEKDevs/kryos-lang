#!/usr/bin/env bash
# Registry HTTP server end-to-end smoke test.
#
# 1. Build `kryos-registry-server` (tools/registry).
# 2. Create a fake index directory with one package (`http-router`).
# 3. Start the server on 127.0.0.1:18080 in the background.
# 4. Curl every documented v1 endpoint and assert response correctness.
# 5. Kill the server.
#
# Used by CI to prove the protocol path actually works end-to-end —
# the registry CLI client and server roundtrip over real HTTP, no
# filesystem shortcuts.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

ok()   { printf '  \033[32mOK\033[0m   %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n  %s\n' "$1" "$2" >&2; cleanup; exit 1; }
step() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

# Globals so cleanup() can see them.
SERVER_PID=""
WORK_DIR=""

cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT INT TERM

# -----------------------------------------------------------------------------
# Step 1: build the server.
# -----------------------------------------------------------------------------
step "Step 1 — build kryos-registry-server"

SERVER_BIN="$REPO_ROOT/tools/registry/target/release/kryos-registry-server"
if [[ "${OS:-}" == "Windows_NT" || "$(uname -s 2>/dev/null)" =~ MINGW|MSYS|CYGWIN ]]; then
    SERVER_BIN="$SERVER_BIN.exe"
fi
if [[ ! -x "$SERVER_BIN" ]]; then
    ( cd tools/registry && cargo build --release ) \
        || fail "cargo build" "tools/registry build failed"
fi
[[ -x "$SERVER_BIN" ]] || fail "server binary missing" "$SERVER_BIN"
ok "built $SERVER_BIN"

# -----------------------------------------------------------------------------
# Step 2: create a fake index.
#
# Layout per the server's read_index_entry:
#   {index_root}/{first 2 chars of name}/{name}.json
# The JSON file contains one version per line (NDJSON).
# -----------------------------------------------------------------------------
step "Step 2 — fake index"

WORK_DIR="$(mktemp -d)"
INDEX="$WORK_DIR/index"
mkdir -p "$INDEX/ht"

cat > "$INDEX/ht/http-router.json" <<'EOF'
{"name":"http-router","version":"1.0.0","checksum":"sha256:abc123","yanked":false}
{"name":"http-router","version":"1.1.0","checksum":"sha256:def456","yanked":false}
EOF

mkdir -p "$INDEX/js"
cat > "$INDEX/js/json.json" <<'EOF'
{"name":"json","version":"0.5.0","checksum":"sha256:f00ba2","yanked":false}
EOF

ok "wrote $INDEX with 2 packages (http-router, json)"

# -----------------------------------------------------------------------------
# Step 3: start the server on a non-default port.
# -----------------------------------------------------------------------------
step "Step 3 — start server on 127.0.0.1:18080"

"$SERVER_BIN" --index "$INDEX" --addr 127.0.0.1:18080 --no-pull \
    > "$WORK_DIR/server.log" 2>&1 &
SERVER_PID=$!

# Wait for the listening socket — poll /v1/health up to 5 seconds.
for _ in $(seq 1 50); do
    if curl -sf -m 1 http://127.0.0.1:18080/v1/health > /dev/null 2>&1; then
        break
    fi
    sleep 0.1
done

# Final assertion that the server actually came up.
if ! curl -sf -m 1 http://127.0.0.1:18080/v1/health > /dev/null 2>&1; then
    fail "server failed to start within 5s" "$(cat "$WORK_DIR/server.log")"
fi
ok "server up (pid $SERVER_PID)"

# -----------------------------------------------------------------------------
# Step 4: exercise every documented v1 endpoint.
# -----------------------------------------------------------------------------
step "Step 4 — endpoint coverage"

# /v1/health
got=$(curl -sf -m 5 http://127.0.0.1:18080/v1/health)
[[ "$got" == "ok" ]] || fail "/v1/health body" "got: '$got' expected 'ok'"
ok "/v1/health → ok"

# /v1/packages/<name> — NDJSON list of versions
got=$(curl -sf -m 5 http://127.0.0.1:18080/v1/packages/http-router)
echo "$got" | grep -q '"version":"1.0.0"' \
    || fail "/v1/packages/http-router missing 1.0.0" "$got"
echo "$got" | grep -q '"version":"1.1.0"' \
    || fail "/v1/packages/http-router missing 1.1.0" "$got"
ok "/v1/packages/http-router → 2 versions present"

# /v1/packages/<name>/<version> — single version, JSON
got=$(curl -sf -m 5 http://127.0.0.1:18080/v1/packages/http-router/1.0.0)
echo "$got" | grep -q '"version":"1.0.0"' \
    || fail "/v1/packages/http-router/1.0.0 mismatch" "$got"
ok "/v1/packages/http-router/1.0.0 → version 1.0.0"

# /v1/packages/<name>/<version> for the OTHER version
got=$(curl -sf -m 5 http://127.0.0.1:18080/v1/packages/http-router/1.1.0)
echo "$got" | grep -q '"version":"1.1.0"' \
    || fail "/v1/packages/http-router/1.1.0 mismatch" "$got"
ok "/v1/packages/http-router/1.1.0 → version 1.1.0"

# /v1/search?q=...
got=$(curl -sf -m 5 'http://127.0.0.1:18080/v1/search?q=http')
echo "$got" | grep -q "http-router" \
    || fail "/v1/search?q=http missing http-router" "$got"
ok "/v1/search?q=http → http-router found"

got=$(curl -sf -m 5 'http://127.0.0.1:18080/v1/search?q=json')
echo "$got" | grep -q "json" \
    || fail "/v1/search?q=json missing json" "$got"
ok "/v1/search?q=json → json found"

# Unknown package — should be 404.
http_code=$(curl -s -o /dev/null -m 5 -w "%{http_code}" \
    http://127.0.0.1:18080/v1/packages/nonexistent-pkg-9k3x)
[[ "$http_code" == "404" ]] \
    || fail "/v1/packages/<unknown> should be 404" "got status: $http_code"
ok "/v1/packages/<unknown> → 404"

# -----------------------------------------------------------------------------
# Done.
# -----------------------------------------------------------------------------
echo
printf '\033[1mregistry-smoke: PASS\033[0m\n'
