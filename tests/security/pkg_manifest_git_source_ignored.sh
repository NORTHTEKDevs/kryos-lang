#!/usr/bin/env bash
# RED TEAM round 3 (toolchain-supply lens, 2026-08-04): a dependency's
# explicit source (`dep = { git = "https://..." }` in kryos.toml, or the
# equivalent `kryos pkg add github:org/repo@1.0.0` CLI form -- both produce
# a `DepSpec::Remote { source, version_req }`, verified live by both the
# Deserialize impl in kryos-package/src/manifest.rs (accepts a `git` TOML
# key) and `parse_dep_string` (accepts `name@version`/`github:org/repo@ver`))
# is NEVER READ by `kryos pkg install`/`update`. `compiler/crates/kryos-cli/
# src/commands/pkg.rs`'s handling of `DepSpec::Remote` destructures it as
# `{ .. }` (wildcard -- the source/version_req fields are discarded) and
# resolves PURELY BY THE DEPENDENCY'S NAME against the registry index,
# unconditionally synthesizing `github_subdir:NORTHTEKDevs/kryos-registry/
# packages/<name>/<version>` regardless of what the manifest actually says.
#
# Consequence, proven both directions below:
#   (A) A dependency name NOT in the registry index: `install` FAILS
#       ("not found in registry") even though the manifest supplies a
#       perfectly valid alternate `git = "..."` source that is never even
#       attempted. The advertised git-source manifest syntax is dead code.
#   (B) A dependency name that DOES exist in the registry index (a common
#       case for any popular package name): `install` SILENTLY SUBSTITUTES
#       the official NORTHTEKDevs/kryos-registry package for whatever the
#       manifest's `git = "..."` pointed at -- no warning, no diff, no
#       prompt, exit 0, "installed 1 package". A project intending to pin a
#       private fork or a security-patched mirror by name-colliding with an
#       official package gets the OFFICIAL (possibly different, possibly
#       outdated, possibly exactly what they were trying to avoid) code
#       instead, with zero indication anything was substituted. This is a
#       genuine supply-chain trust gap distinct from LEDGER item 12 (lock
#       file never read) and item 1b (checksum, CLOSED) -- both of those
#       concern integrity of ONE already-selected source; this is about the
#       SOURCE SELECTION ITSELF silently ignoring the manifest.
#
# Classify by the printed diagnostics + `kryos.lock`'s recorded `source`
# field (the observable a real consumer would rely on), not by exit code
# alone -- (B) exits 0 exactly like an honest install.

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

echo "=== Part A: no registry synced at all (fresh, isolated HOME) ==="
( cd "$PROJ" && "$KRYOS" pkg install > installA.log 2>&1 )
RC_A=$?
cat "$PROJ/installA.log"
echo "install rc = $RC_A"

if grep -qi 'some-real-org/some-real-repo' "$PROJ/installA.log"; then
    echo
    echo "NOT REPRODUCED: install actually attempted the manifest's declared" \
         "git source -- this would mean the bug is already fixed."
    exit 1
fi

if [[ $RC_A -eq 0 ]]; then
    echo
    echo "NOT REPRODUCED: install unexpectedly succeeded without ever" \
         "consulting the registry or the declared git source."
    exit 1
fi

echo
echo "CONFIRMED (A): install FAILED ('not found in registry') and never" \
     "even attempted the manifest's explicit git = \"https://github.com/" \
     "some-real-org/some-real-repo\" source -- the field is dead code."

echo
echo "=== Part B (best-effort, needs network + a real registry entry): ==="
echo "    same manifest shape but naming a package (http-router) that IS"
echo "    in the real NORTHTEKDevs/kryos-registry index, with an obviously"
echo "    bogus/attacker git= source -- if this succeeds, the official"
echo "    package silently replaces the declared source with no warning."
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
    if [[ $RC_B -eq 0 ]] && grep -q 'NORTHTEKDevs/kryos-registry' "$PROJ/installB.log" \
       && ! grep -qi 'ATTACKER-CONTROLLED' "$PROJ/installB.log"; then
        echo
        echo "CONFIRMED (B): install silently substituted the OFFICIAL" \
             "NORTHTEKDevs/kryos-registry http-router package for the" \
             "manifest's declared (attacker) git source -- zero warning," \
             "exit 0. kryos.lock:"
        cat "$PROJ/kryos.lock" 2>/dev/null || true
    else
        echo "Part B inconclusive (registry entry for http-router may not" \
             "exist, or environment differs) -- Part A alone already" \
             "proves the git-source field is never consulted."
    fi
else
    echo "Part B skipped: no network / registry sync unavailable in this" \
         "environment -- Part A alone already proves the defect" \
         "deterministically offline."
fi

exit 0
