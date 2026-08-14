#!/usr/bin/env bash
# cli_smoke_gate.sh -- every advertised `kryos` subcommand must actually run.
#
# WHY: LEDGER item 26 found that ~20 of the ~35 CLI subcommands had ZERO test
# references anywhere in tests/. The CLI surface is advertised in the README as
# "30+ subcommands" and frozen for 1.x in VERSIONING.md, yet most of it was
# never executed by any gate -- a `kryos fmt` or `kryos doc` that regressed
# would ship silently and be the first thing a new user hits.
#
# This is a SMOKE gate, deliberately: it proves each subcommand starts, parses
# its arguments, and reaches its normal exit path. It does not assert output
# correctness -- the per-feature gates do that. Cheap enough to run in tier 1.
#
# NOTE ON EXPECTED NON-ZERO EXITS: four subcommands correctly refuse to do
# anything in a bare directory, and that refusal IS the contract being pinned:
#   test       rc=1 with no @test functions -- refusing to report success for a
#              file it never verified is the whole point (a vacuous PASS is the
#              failure mode this repo cares most about)
#   tree       rc=1 outside a project -- names kryos.toml and tells you to init
#   changelog  rc=1 outside a git repo -- `git tag` genuinely cannot run
#   config     rc=2 with no subcommand -- clap usage, standard CLI behaviour
# Asserting rc=0 for these would pin the WRONG behaviour.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
cat > "$W/h.kry" <<'EOF'
/// Adds two numbers.
fn add(a: i64, b: i64) -> i64 { return a + b }

@test
fn t_add() { assert(add(1, 2) == 3, "add works") }

fn main() { println("hi " + to_string(add(1, 2))) }
EOF

# A valid program with NO @test functions, for the vacuous-pass check below.
cat > "$W/empty_no_tests.kry" <<'EOF'
fn main() { println("no tests here") }
EOF

pass=0; fail=0; failed=""
t() {
    local n="$1" want="$2"; shift 2
    out="$(timeout 90 "$@" 2>&1)"; rc=$?
    if [ "$rc" = "$want" ]; then
        pass=$((pass + 1)); printf '  ok    %-11s rc=%s\n' "$n" "$rc"
    else
        fail=$((fail + 1)); failed="$failed $n"
        printf '  FAIL  %-11s rc=%s (want %s)\n' "$n" "$rc" "$want"
        printf '%s\n' "$out" | head -4 | sed 's/^/          /'
    fi
}

cd "$W" || exit 1

# --- compile / run pipeline ---
t version   0 "$K" version
t help      0 "$K" help
t check     0 "$K" check h.kry
t run       0 "$K" run h.kry
t build     0 "$K" build --release h.kry -o out_bin
t eval      0 "$K" eval '1 + 1'

# --- source tooling ---
t fmt       0 "$K" fmt h.kry
t lint      0 "$K" lint h.kry
t doc       0 "$K" doc h.kry
t explain   0 "$K" explain E0100
t audit     0 "$K" audit h.kry
t test      0 "$K" test h.kry
t bench     0 "$K" bench h.kry
t coverage  0 "$K" coverage h.kry

# --- refuses-without-context (the refusal IS the contract; see header) ---
t test_none    1 "$K" test empty_no_tests.kry
t tree_noproj  1 "$K" tree
t changelog_ng 1 "$K" changelog
t config_bare  2 "$K" config

# --- environment / project ---
t info      0 "$K" info
t doctor    0 "$K" doctor
t cheat     0 "$K" cheat
t welcome   0 "$K" welcome
t new       0 "$K" new demo_proj
t clean     0 "$K" clean

# --- in-project commands, run where they are meant to be run ---
if [ -d demo_proj ]; then
    ( cd demo_proj && timeout 90 "$K" tree >/dev/null 2>&1 )
    rc=$?
    if [ "$rc" = 0 ]; then pass=$((pass + 1)); printf '  ok    %-11s rc=0\n' "tree_proj"
    else fail=$((fail + 1)); failed="$failed tree_proj"; printf '  FAIL  %-11s rc=%s (want 0)\n' "tree_proj" "$rc"; fi
fi

# --- long-running / server subcommands: --help only, they would block ---
for c in bindgen diff pack profile trace workspace manifest pkg watch dap lsp repl doc-serve; do
    t "$c" 0 "$K" "$c" --help
done

echo
if [ "$fail" -eq 0 ]; then
    echo "cli-smoke: $pass/$pass subcommands behave as contracted"
    exit 0
fi
echo "cli-smoke: $fail FAILED --$failed"
exit 1
