#!/usr/bin/env bash
# LEDGER item 12 -- REGRESSION PIN (was a RED-TEAM repro; the bug it found is
# now FIXED, see tools/loop/LEDGER.md CLOSED table). This now asserts the
# FIXED behavior instead of the bug.
#
# The bug (historical): `kryos pkg install` never opened `kryos.lock` at
# all. It always re-resolved live against the manifest and silently
# OVERWROTE the lock with whatever it found, even when kryos.lock was
# present and pinned a different version -- defeating the documented
# supply-chain protection (CLAUDE.md said "pinning a specific version still
# depends on committing kryos.lock", which provided zero enforcement).
#
# The fix: `install()` now reads an existing `kryos.lock` first. If it
# already covers every dependency the manifest declares, the install is
# PINNED -- it fetches exactly what the lock says (checksum-verified, per
# LEDGER item 1b) and does NOT touch the registry or rewrite the lock at
# all, matching `npm ci` / `cargo install --locked` semantics. `kryos pkg
# update` remains the explicit, deliberate re-resolve operation.
#
# This repro uses an offline PATH dependency (no network required) to
# demonstrate the mechanism deterministically -- the exact same pinned-vs-
# fresh code path is used for Remote/registry dependencies, where "v2.0.0"
# would be a newly published (or force-pushed) index entry instead of a
# hand-edited local kryos.toml.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KRYOS="${KRYOS:-$REPO_ROOT/compiler/target/release/kryos}"
if [[ ! -x "$KRYOS" && -x "$KRYOS.exe" ]]; then KRYOS="$KRYOS.exe"; fi
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$REPO_ROOT/compiler/stdlib}"

WORK_DIR="$(mktemp -d)"
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

DEP="$WORK_DIR/dep"
PROJ="$WORK_DIR/proj"
mkdir -p "$DEP/src" "$PROJ/src"

cat > "$DEP/kryos.toml" <<'EOF'
[package]
name = "dep"
version = "1.0.0"
EOF
cat > "$DEP/src/main.kry" <<'EOF'
fn dep_fn() -> str {
    return "v1-SAFE"
}
EOF

cat > "$PROJ/kryos.toml" <<'EOF'
[package]
name = "proj"
version = "0.1.0"

[dependencies]
dep = { path = "../dep" }
EOF
cat > "$PROJ/src/main.kry" <<'EOF'
fn main() {
    println("proj ok")
}
EOF

FAIL=0

( cd "$PROJ" && "$KRYOS" pkg install > install1.log 2>&1 )
LOCKED_V1="$(grep -A2 'name = "dep"' "$PROJ/kryos.lock" | grep version || true)"
if ! echo "$LOCKED_V1" | grep -q '1\.0\.0'; then
    echo "FAIL: setup did not produce the expected v1.0.0 lock entry" >&2
    cat "$PROJ/install1.log" >&2
    exit 1
fi
echo "OK  first install locked: $LOCKED_V1"

# Bump the dependency to a "malicious" v2.0.0 with no change whatsoever to
# proj/kryos.toml and no deletion of proj/kryos.lock -- exactly what a
# compromised/force-pushed registry version would look like from the
# consumer's point of view (their manifest and lock are untouched by them).
cat > "$DEP/kryos.toml" <<'EOF'
[package]
name = "dep"
version = "2.0.0"
EOF
cat > "$DEP/src/main.kry" <<'EOF'
fn dep_fn() -> str {
    return "v2-MALICIOUS-PAYLOAD"
}
EOF

( cd "$PROJ" && "$KRYOS" pkg install > install2.log 2>&1 )
LOCKED_V2="$(grep -A2 'name = "dep"' "$PROJ/kryos.lock" | grep version || true)"

echo
echo "=== kryos.lock after 2nd install (lock file was present, dep drifted) ==="
cat "$PROJ/kryos.lock"
echo
echo "=== install #2 output ==="
cat "$PROJ/install2.log"
echo

if echo "$LOCKED_V2" | grep -q '2\.0\.0'; then
    echo "FAIL: kryos pkg install silently overwrote a committed kryos.lock" >&2
    echo "(v1.0.0 -> v2.0.0) -- item 12 is NOT fixed (or regressed)." >&2
    FAIL=1
elif ! echo "$LOCKED_V2" | grep -q '1\.0\.0'; then
    echo "FAIL: kryos.lock no longer pins v1.0.0 but also doesn't show v2.0.0 --" >&2
    echo "unexpected state, inspect kryos.lock/install2.log above." >&2
    FAIL=1
elif ! grep -qi 'pinned' "$PROJ/install2.log"; then
    echo "FAIL: kryos.lock correctly still pins v1.0.0, but install2.log never" >&2
    echo "says anything about a pinned install -- the RIGHT answer for the WRONG" >&2
    echo "reason is not proof; inspect install2.log." >&2
    FAIL=1
else
    echo "OK: kryos pkg install kept kryos.lock pinned at v1.0.0 across the drift" \
         "and reported the pinned install explicitly (not silent)."
fi

# The escape hatch: a DELIBERATE `kryos pkg update` must still pick up the
# new version -- the fix must not brick legitimate, reviewed version bumps,
# only the SILENT/automatic ones.
( cd "$PROJ" && "$KRYOS" pkg update > update.log 2>&1 )
( cd "$PROJ" && "$KRYOS" pkg install > install3.log 2>&1 )
LOCKED_V3="$(grep -A2 'name = "dep"' "$PROJ/kryos.lock" | grep version || true)"
echo
echo "=== kryos.lock after explicit 'kryos pkg update' + install ==="
cat "$PROJ/kryos.lock"
if echo "$LOCKED_V3" | grep -q '2\.0\.0'; then
    echo "OK: an EXPLICIT 'kryos pkg update' still deliberately moves the lock to" \
         "v2.0.0 -- the fix blocks silent drift, not legitimate re-resolution."
else
    echo "FAIL: 'kryos pkg update' did not move the lock to v2.0.0 as expected --" >&2
    echo "inspect update.log/install3.log above." >&2
    FAIL=1
fi

exit $FAIL
