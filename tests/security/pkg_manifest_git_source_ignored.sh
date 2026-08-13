#!/usr/bin/env bash
# LEDGER item 17 -- REGRESSION PIN (was a RED-TEAM repro; the bug it found is
# now FIXED, see tools/loop/LEDGER.md CLOSED table). This now asserts the
# FIXED behavior instead of the bug.
#
# The bug (historical): a dependency's explicit source (`dep = { git =
# "https://..." }` in kryos.toml, or the equivalent `kryos pkg add
# github:org/repo@1.0.0` CLI form -- both produce a `DepSpec::Remote {
# source, version_req }`) was NEVER READ by `kryos pkg install`/`update`.
# `compiler/crates/kryos-cli/src/commands/pkg.rs`'s handling of
# `DepSpec::Remote` destructured it as `{ .. }` (wildcard -- source/
# version_req discarded) and resolved PURELY BY NAME against the registry
# index, unconditionally synthesizing `github_subdir:NORTHTEKDevs/
# kryos-registry/packages/<name>/<version>` regardless of what the manifest
# actually said.
#
# The fix: `install()`/`update()` now share `add_remote_deps_to_registry()`,
# which honors a NON-EMPTY `DepSpec::Remote.source` directly (via
# `kryos_package::fetch::fetch_explicit_source`) instead of ever calling the
# registry-by-name lookup for that dependency. This script proves BOTH
# directions the original repro found are now honest instead of silent:
#
#   (A) A dependency name NOT in the registry index, explicit git source:
#       `install` must ATTEMPT the declared source (visible in the log) and
#       fail with a git-clone-shaped error naming that source -- NOT "not
#       found in registry" (that message would mean the source was never
#       even tried).
#   (B) A dependency name that DOES exist in the registry (http-router),
#       explicit ATTACKER-CONTROLLED git source: `install` must ATTEMPT
#       (and, since the attacker URL doesn't exist, fail on) that source --
#       it must NEVER silently install the official registry package
#       instead. No success banner, no "installed 1 package" for this dep,
#       and kryos.lock (if it existed before) must be left untouched.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KRYOS="${KRYOS:-$REPO_ROOT/compiler/target/release/kryos}"
if [[ ! -x "$KRYOS" && -x "$KRYOS.exe" ]]; then KRYOS="$KRYOS.exe"; fi
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$REPO_ROOT/compiler/stdlib}"

WORK_DIR="$(mktemp -d)"
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

# Isolate HOME/USERPROFILE so this test never depends on (or pollutes) a
# real ~/.kryos registry cache -- part A is fully offline-deterministic.
export HOME="$WORK_DIR/fake_home"
export USERPROFILE="$WORK_DIR/fake_home"
mkdir -p "$HOME"

PROJ="$WORK_DIR/proj"
mkdir -p "$PROJ/src"
cat > "$PROJ/kryos.toml" <<'EOF'
[package]
name = "git-source-ignored-poc"
version = "0.1.0"

[dependencies]
totally-unregistered-name-zzz = { git = "https://github.com/some-real-org/some-real-repo", version = "0.1.0" }
EOF
cat > "$PROJ/src/main.kry" <<'EOF'
fn main() {
    println("poc")
}
EOF

FAIL=0

echo "=== Part A: no registry synced at all (fresh, isolated HOME) ==="
( cd "$PROJ" && "$KRYOS" pkg install > installA.log 2>&1 )
RC_A=$?
cat "$PROJ/installA.log"
echo "install rc = $RC_A"

if grep -qi 'not found in registry' "$PROJ/installA.log"; then
    echo
    echo "FAIL (A): install fell back to a registry lookup for a dependency that" \
         "declared an explicit git source -- item 17 is NOT fixed (or regressed)."
    FAIL=1
elif ! grep -qi 'some-real-org/some-real-repo' "$PROJ/installA.log"; then
    echo
    echo "FAIL (A): install neither attempted the declared git source NOR fell back" \
         "to the registry -- unexpected failure mode, inspect installA.log."
    FAIL=1
elif [[ $RC_A -eq 0 ]]; then
    echo
    echo "FAIL (A): install unexpectedly SUCCEEDED against a nonexistent" \
         "some-real-org/some-real-repo -- inspect installA.log."
    FAIL=1
else
    echo
    echo "OK (A): install ATTEMPTED the manifest's declared git source" \
         "(some-real-org/some-real-repo appears in the log) and failed honestly" \
         "(git clone of a nonexistent repo), never touching the registry index."
fi

echo
echo "=== Part B (best-effort, needs network + a real registry entry): ==="
echo "    same manifest shape but naming a package (http-router) that IS"
echo "    in the real NORTHTEKDevs/kryos-registry index, with an obviously"
echo "    bogus/attacker git= source -- must ATTEMPT (and fail on) that"
echo "    source, never silently substitute the official package."
sed -i 's/totally-unregistered-name-zzz = .*/http-router = { git = "https:\/\/github.com\/ATTACKER-CONTROLLED\/evil-http-router", version = "0.1.0" }/' "$PROJ/kryos.toml"
rm -f "$PROJ/kryos.lock"

REGISTRY_CLIENT_OK=0
if "$KRYOS" pkg sync > "$PROJ/sync.log" 2>&1; then
    REGISTRY_CLIENT_OK=1
fi

if [[ $REGISTRY_CLIENT_OK -eq 1 ]]; then
    ( cd "$PROJ" && "$KRYOS" pkg install > installB.log 2>&1 )
    RC_B=$?
    cat "$PROJ/installB.log"
    echo "install rc = $RC_B"

    if [[ $RC_B -eq 0 ]]; then
        echo
        echo "FAIL (B): install SUCCEEDED for an attacker-controlled source that does" \
             "not exist -- either the official package was silently substituted" \
             "(item 17 regressed) or some other unexpected success path was taken."
        FAIL=1
    elif ! grep -qi 'ATTACKER-CONTROLLED' "$PROJ/installB.log"; then
        echo
        echo "FAIL (B): install failed, but never mentions the declared" \
             "ATTACKER-CONTROLLED source -- inspect installB.log to see what" \
             "source it actually tried (or fell back to)."
        FAIL=1
    elif [[ -f "$PROJ/kryos.lock" ]]; then
        echo
        echo "FAIL (B): kryos.lock was written despite the install failing -- a" \
             "rejected/failed install must never leave a lock file behind."
        FAIL=1
    else
        echo
        echo "OK (B): install ATTEMPTED the manifest's declared (attacker) git" \
             "source, failed honestly (the repo doesn't exist), and never" \
             "installed the official NORTHTEKDevs/kryos-registry package in its" \
             "place. No kryos.lock was written."
    fi
else
    echo "Part B skipped: no network / registry sync unavailable in this" \
         "environment -- Part A alone already proves the fix deterministically" \
         "offline."
fi

exit $FAIL
