#!/usr/bin/env bash
# audit_parse_failure_gate.sh -- `kryos audit` must never report a clean
# bill for a file it could not parse.
#
# WHY: `kryos audit` is the tool a user runs to inspect a package's extern
# surface and capability usage BEFORE trusting it. `scan_file` used to do
#     let tokens = kryos_lexer::Lexer::new(&source, 0).tokenize();
#     let Ok(module) = kryos_parser::parse(tokens) else { return };
# which silently discarded lexer diagnostics (`.tokenize()` drops them) and
# on a parse error returned BEFORE the extern/capability AST walk AND
# BEFORE `check_cap_violations` ran (that call sat after the early return).
# A file containing `extern "C" { fn kryos_dangerous_native_thing(..) }`
# plus `@capabilities(all)`, wrapped in a `main` missing a close-paren,
# audited as "(no extern blocks)" / "(none detected)" / rc=0 -- byte-
# identical in shape to an actually safe file. Fixed: a lex/parse failure
# is now a loud "Parse failures" section plus a nonzero exit, and every
# other section notes it is incomplete for the unparseable file(s).
#
# This gate pins every shape found while fixing the class:
#   - a clean parse error (missing paren) with a dangerous extern block
#   - a clean file (control: must stay rc=0, clean)
#   - a lexer-level error (unterminated string)
#   - an empty file (control: must stay rc=0, clean -- NOT a parse failure)
#   - a directory of several files where exactly one fails to parse (the
#     broken file must not silently vanish from the report; the OTHER
#     files must still be scanned and reported)
#   - `--format=json` must emit a `parse_failures` array and stay valid
#     JSON even when it is non-empty
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT

# A file that fails to PARSE (missing close-paren in main) and declares a
# dangerous extern block + @capabilities(all) -- the exact incident repro.
cat > "$W/broken.kry" <<'EOF'
extern "C" {
    fn kryos_dangerous_native_thing(x: i64) -> i64
}

@capabilities(all)
fn very_dangerous(path: str) -> str {
    let x = file_read(path)
    let y = file_write(path, x)
    return x
}

fn main() {
    println("broken but should still be flagged"
}
EOF

# A genuinely clean, parseable file -- must stay rc=0.
cat > "$W/good.kry" <<'EOF'
@capabilities(fs:read)
fn read_it(path: str) -> str {
    return file_read(path)
}

fn main() {
    println("good")
}
EOF

# A lexer-level error (unterminated string literal) -- distinct failure
# stage from a parser error, must ALSO be caught (previously `.tokenize()`
# silently dropped lexer diagnostics entirely).
cat > "$W/lex_bad.kry" <<'EOF'
fn main() {
    let s: str = "unterminated
}
EOF

# An empty file -- must stay clean (0 declarations is not a parse failure).
: > "$W/empty.kry"

# Another good file, for the directory-with-mixed-files case.
cat > "$W/other_good.kry" <<'EOF'
fn main() { println("hi") }
EOF

pass=0; fail=0; failed=""
ok() { pass=$((pass + 1)); printf '  ok    %s\n' "$1"; }
bad() { fail=$((fail + 1)); failed="$failed $1"; printf '  FAIL  %s -- %s\n' "$1" "$2"; }

# --- single broken file: must fail loudly, never rc=0 ---
out="$(timeout 30 "$K" audit "$W/broken.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ]; then
    bad "broken_rc" "rc=0 (a parse-failing file must never be reported clean)"
else
    ok "broken_rc (rc=$rc)"
fi
if printf '%s' "$out" | grep -qi "parse failures"; then
    ok "broken_has_parse_failures_section"
else
    bad "broken_has_parse_failures_section" "output missing a Parse failures section"
fi
if printf '%s' "$out" | grep -q "broken.kry"; then
    ok "broken_names_the_file"
else
    bad "broken_names_the_file" "output never names broken.kry -- it would have silently vanished"
fi
# The dangerous extern block must NOT be reported as absent-and-therefore-
# clean; the incomplete-section caveat must be present instead.
if printf '%s' "$out" | grep -A2 "== Extern blocks ==" | grep -qi "excluded from this section"; then
    ok "broken_extern_section_caveated"
else
    bad "broken_extern_section_caveated" "Extern blocks section did not disclose it is incomplete"
fi

# --- clean file: must stay rc=0, no parse-failure noise ---
out="$(timeout 30 "$K" audit "$W/good.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ]; then ok "good_rc"; else bad "good_rc" "rc=$rc, want 0"; fi
if printf '%s' "$out" | grep -A1 "== Parse failures" | grep -qi "none"; then
    ok "good_no_parse_failures"
else
    bad "good_no_parse_failures" "clean file reported a parse failure"
fi

# --- lexer-level error: must also fail loudly ---
out="$(timeout 30 "$K" audit "$W/lex_bad.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ]; then
    bad "lex_bad_rc" "rc=0 (a lexer error must never be reported clean)"
else
    ok "lex_bad_rc (rc=$rc)"
fi
if printf '%s' "$out" | grep -qi "parse failures"; then
    ok "lex_bad_has_parse_failures_section"
else
    bad "lex_bad_has_parse_failures_section" "output missing a Parse failures section"
fi

# --- empty file: must stay clean (not a parse failure) ---
out="$(timeout 30 "$K" audit "$W/empty.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ]; then ok "empty_rc"; else bad "empty_rc" "rc=$rc, want 0"; fi

# --- directory with one broken file among several: must not vanish, must
#     fail the whole run, and must still scan the OTHER files ---
DIR="$W/mixed"
mkdir -p "$DIR"
cp "$W/broken.kry" "$W/good.kry" "$W/other_good.kry" "$DIR/"
out="$(timeout 30 "$K" audit "$DIR" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ]; then
    bad "dir_rc" "rc=0 on a directory containing an unparseable file"
else
    ok "dir_rc (rc=$rc)"
fi
if printf '%s' "$out" | grep -q "scanned 3 file"; then
    ok "dir_scanned_all_three"
else
    bad "dir_scanned_all_three" "did not report scanning all 3 files"
fi
if printf '%s' "$out" | grep -q "broken.kry"; then
    ok "dir_broken_file_named"
else
    bad "dir_broken_file_named" "broken.kry vanished from a directory report"
fi
if printf '%s' "$out" | grep -q "fs:read"; then
    ok "dir_good_file_still_scanned"
else
    bad "dir_good_file_still_scanned" "good.kry's fs:read annotation missing -- other files stopped being scanned"
fi

# --- JSON format: must include parse_failures and stay parseable ---
out="$(timeout 30 "$K" audit "$W/broken.kry" --format=json 2>&1)"
json_line="$(printf '%s\n' "$out" | grep '^{' | head -1)"
if printf '%s' "$json_line" | grep -q '"parse_failures":\[{'; then
    ok "json_has_parse_failures"
else
    bad "json_has_parse_failures" "JSON output missing a populated parse_failures array"
fi
if command -v python3 >/dev/null 2>&1; then
    if printf '%s' "$json_line" | python3 -c "import json,sys; json.loads(sys.stdin.read())" 2>/dev/null; then
        ok "json_is_valid"
    else
        bad "json_is_valid" "audit --format=json emitted invalid JSON"
    fi
fi

echo
if [ "$fail" -eq 0 ]; then
    echo "audit-parse-failure: $pass/$pass checks pass"
    exit 0
fi
echo "audit-parse-failure: $fail FAILED --$failed"
exit 1
