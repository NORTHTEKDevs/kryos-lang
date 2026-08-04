#!/usr/bin/env bash
# RED TEAM round 1 (toolchain-supply lens, 2026-08-04): `kryos pkg install`
# NEVER reads an existing `kryos.lock` -- it always re-resolves live against
# the manifest's version requirement and silently OVERWRITES the lock with
# whatever it finds, even when kryos.lock is present and pins a different
# version. This defeats the documented supply-chain protection: CLAUDE.md
# says "pinning a specific version still depends on committing kryos.lock",
# but committing it provides ZERO enforcement -- grep the CLI source and
# `LockFile::from_file` is called only by `kryos pkg outdated`, never by
# `install()` (compiler/crates/kryos-cli/src/commands/pkg.rs).
#
# The checksum verification added for LEDGER item 1b (kryos-package/src/
# fetch.rs::verify_package_checksum) only protects against a package's
# content being tampered with AFTER a specific version is resolved -- it
# does nothing to stop `install()` from silently resolving to a DIFFERENT
# (newer, possibly compromised or maliciously re-published) version than
# the one a team previously locked and committed. There is no warning, no
# diff, no prompt: the tool prints the same "installed 1 package" success
# banner whether or not the resolved version matches the lock.
#
# This repro uses an offline PATH dependency (no network required) to
# demonstrate the mechanism deterministically: kryos.lock pins v1.0.0,
# the dependency is bumped to v2.0.0 on disk with no manifest/version-req
# change, and a second `kryos pkg install` run (lock file present and
# untouched) silently re-locks to v2.0.0 with no diagnostic of any kind.
# The exact same code path (`resolve()` called fresh from the manifest,
# `fetch_resolved` fetching+verifying whatever `resolve()` picked) is used
# for Remote/registry dependencies, where "v2.0.0" would be a newly
# published (or force-pushed) index entry instead of a hand-edited local
# kryos.toml -- i.e. this is the exact shape of a supply-chain version-drift
# attack that a committed lock file is supposed to prevent and does not.
#
# Classify by the printed `kryos.lock` CONTENT (the observable a real
# consumer would rely on), not by exit code -- `kryos pkg install` exits 0
# in both the honest and the drifted case.

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
echo "=== install #2 output (no warning/diff printed about the version drift) ==="
cat "$PROJ/install2.log"
echo

if echo "$LOCKED_V2" | grep -q '2\.0\.0'; then
    echo
    echo "CONFIRMED: kryos pkg install silently overwrote a committed kryos.lock"
    echo "(v1.0.0 -> v2.0.0) with NO warning, diff, or prompt of any kind."
    echo "The lock file provides no enforcement -- LEDGER item 12."
    exit 0
else
    echo "NOT REPRODUCED: lock still pins v1.0.0 after the dependency drifted"
    echo "(got: $LOCKED_V2) -- this would mean the bug is already fixed."
    exit 1
fi
