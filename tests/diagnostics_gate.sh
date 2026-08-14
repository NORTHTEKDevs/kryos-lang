#!/usr/bin/env bash
# Diagnostics gate: error-message/error-code quality regressions.
#
# Kryos is evaluated by people writing real programs for the first time; a
# confusing or cascading diagnostic on a common mistake is a real defect even
# when the compiler correctly rejects the program. This gate pins down two
# concrete cascade fixes (an already-reported error must not cascade into a
# wall of unrelated noise) plus the presence of error codes that used to be
# silently missing (so `kryos explain <code>` has something to explain).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# 1. A `[dyn Trait]` array (rejected with E0110) must not ALSO poison a
#    later use of the array (`for h in handlers { h.handle() }`) into a
#    second, unrelated "no method `handle` found for type `i64`" error --
#    the for-loop's element type must inherit the Error poison, not default
#    to i64.
cat > "$TMP/dyn_container_use.kry" <<'KRY'
trait Handler {
    fn handle(self: Self) -> str
}
struct A {}
impl Handler for A {
    fn handle(self: A) -> str { return "a" }
}
struct B {}
impl Handler for B {
    fn handle(self: B) -> str { return "b" }
}
fn main() {
    let handlers: [dyn Handler] = [A{}, B{}]
    for h in handlers {
        println(h.handle())
    }
}
KRY
out="$("$K" check "$TMP/dyn_container_use.kry" 2>&1)"
err_count=$(grep -c 'error\[E[0-9]' <<<"$out")
if [ "$err_count" -eq 1 ] && grep -q "E0110" <<<"$out" && ! grep -q "no method \`handle\`" <<<"$out"; then
    echo "  ok   dyn-container array use doesn't cascade into a bogus E0107"
else
    echo "  FAIL: expected exactly 1 error (E0110), got:"; echo "$out" | sed 's/^/    /'; fail=1
fi

# 2. A reserved keyword used as a name (`let match: i64 = 5`), then
#    referenced later in expression position (`to_string(match)`), must
#    produce the ONE root-cause error plus the one legitimate follow-on
#    error (bare `match` can't start an expression) -- not a wall of
#    "unexpected end of file" errors from the match-expression parser
#    consuming real closing delimiters it doesn't own.
cat > "$TMP/reserved_word.kry" <<'KRY'
fn main() {
    let match: i64 = 5
    println(to_string(match))
}
KRY
out="$("$K" check "$TMP/reserved_word.kry" 2>&1)"
err_count=$(grep -c 'error\[E[0-9]' <<<"$out")
if [ "$err_count" -le 2 ] && grep -q "reserved keyword 'match' cannot be used as a name" <<<"$out" \
   && ! grep -q "end of file" <<<"$out"; then
    echo "  ok   reserved-keyword-as-name doesn't cascade to end-of-file noise ($err_count errors)"
else
    echo "  FAIL: expected <=2 errors, no EOF cascade, got:"; echo "$out" | sed 's/^/    /'; fail=1
fi

# 3. Every diagnostic below must carry an explainable error code (used to be
#    silently code-less, so `kryos explain` had nothing to look up).
cat > "$TMP/semicolon.kry" <<'KRY'
fn main() {
    let x: i64 = 5;
    println(to_string(x))
}
KRY
out="$("$K" check "$TMP/semicolon.kry" 2>&1)"
if grep -q "error\[E0009\]" <<<"$out"; then
    echo "  ok   semicolon error carries E0009"
else
    echo "  FAIL: semicolon error missing a code:"; echo "$out" | sed 's/^/    /'; fail=1
fi

cat > "$TMP/unterminated.kry" <<'KRY'
fn main() {
    let json: str = "{\"a\":1}"
    println(json)
}
KRY
out="$("$K" check "$TMP/unterminated.kry" 2>&1)"
if grep -q "error\[E0009\]" <<<"$out"; then
    echo "  ok   unterminated-string error carries E0009"
else
    echo "  FAIL: unterminated-string error missing a code:"; echo "$out" | sed 's/^/    /'; fail=1
fi

cat > "$TMP/dup_import.kry" <<'KRY'
use std::csv::{parse}
use std::json::{parse}
fn main() { println("x") }
KRY
out="$("$K" check "$TMP/dup_import.kry" 2>&1)"
if grep -q "error\[E0205\]" <<<"$out"; then
    echo "  ok   duplicate-import error carries E0205"
else
    echo "  FAIL: duplicate-import error missing E0205:"; echo "$out" | sed 's/^/    /'; fail=1
fi

cat > "$TMP/unknown_export.kry" <<'KRY'
use std::string::{capitalize_words}
fn main() { println(capitalize_words("hi")) }
KRY
out="$("$K" check "$TMP/unknown_export.kry" 2>&1)"
if grep -q "error\[E0204\]" <<<"$out"; then
    echo "  ok   unknown-export error carries E0204"
else
    echo "  FAIL: unknown-export error missing E0204:"; echo "$out" | sed 's/^/    /'; fail=1
fi

# 4. `kryos explain` must actually resolve the new codes end to end.
for code in E0204 E0205; do
    if "$K" explain "$code" 2>&1 | grep -q "^$code:"; then
        echo "  ok   kryos explain $code resolves"
    else
        echo "  FAIL: kryos explain $code did not resolve"; fail=1
    fi
done

# 5. Non-regression: legitimate match expressions (multi-arm, or-patterns,
#    enum payloads) must still compile and run correctly after the
#    match-scrutinee-failure early-bailout change.
cat > "$TMP/sanity_match.kry" <<'KRY'
enum Shape {
    Circle(f64),
    Rect(f64, f64),
    Empty,
}
fn area(s: Shape) -> f64 {
    match s {
        Circle(r)  => return 3.14159 * r * r,
        Rect(w, h) => return w * h,
        Empty      => return 0.0,
    }
}
fn main() {
    println(to_string(area(Shape.Circle(2.0))))
    match 5 {
        1 | 2 | 3 => println("low"),
        _ => println("high"),
    }
}
KRY
out="$("$K" run "$TMP/sanity_match.kry" 2>&1)"
if [ "$out" = "$(printf '12.56636\nhigh')" ]; then
    echo "  ok   ordinary match expressions still compile and run correctly"
else
    echo "  FAIL: match sanity check regressed:"; echo "$out" | sed 's/^/    /'; fail=1
fi

# 6. A stray token at block-statement level that `parse_primary`'s
#    unexpected-token fallback deliberately does not consume (a bare `,`,
#    or a misplaced `)`/`]`/`}` with no enclosing call/array/struct-literal
#    to eat it) must be REJECTED with a diagnostic, not hang the parser
#    forever. `parse_module` already has a no-progress guard for the exact
#    same class at the top-level-declaration loop (comment there cites a
#    fuzzer-found 2-byte input, "}:"); `parse_block_stmts` lacked the
#    equivalent guard one level down the grammar -- any complete statement
#    followed by a stray `,` (e.g. `map<str, i64>` mistyped as a value
#    expression, or a plain trailing comma) hung `kryos check`/`run`/`build`
#    indefinitely with ZERO output, unkillable except by the OS. Bounded
#    with `timeout` since a `conf_*.kry` conformance test can't express
#    "must not hang" (matches the `docs_status_gate`/`utf8_invalid_string_gate`
#    precedent for assertions a normal exit-0 conformance file can't make).
cat > "$TMP/stray_comma.kry" <<'KRY'
fn main() {
    let x = 5
    ,
    println(to_string(x))
}
KRY
if command -v timeout >/dev/null 2>&1; then
    out="$(timeout 10 "$K" check "$TMP/stray_comma.kry" 2>&1)"
    rc=$?
    if [ "$rc" -eq 124 ]; then
        echo "  FAIL: stray trailing comma HANGS kryos check (timed out after 10s)"; fail=1
    elif grep -q "unexpected token ','" <<<"$out"; then
        echo "  ok   stray trailing comma is rejected promptly (rc=$rc), no hang"
    else
        echo "  FAIL: stray comma didn't hang but also didn't get the expected diagnostic:"
        echo "$out" | sed 's/^/    /'; fail=1
    fi
else
    echo "  skip stray-comma-hang check (no 'timeout' command available)"
fi

# 7. LEDGER item 9 (the `||`-continuation trap, CLAUDE.md hard rule 1): the
#    parser has no newline awareness, so a fresh line starting with `||`
#    silently continues the PREVIOUS statement's expression as boolean-or
#    instead of starting a new statement (e.g. an empty-param closure
#    literal) -- a SILENT WRONG VALUE with no diagnostic. W0001 now detects
#    the dangerous shape (first `||` in the expression, right after a
#    newline) without changing the merge behavior itself (backward compat:
#    the value stays whatever the merge produces). Three shapes, all real:
#    (a) the true bug MUST warn, (b) an established multi-line boolean-or
#    CHAIN (`is_digit`-style, `||` already used earlier in the SAME
#    statement) must NOT warn -- shipped in this repo's own
#    examples/real/*.kry, (c) a multi-line bitwise-or BIT-PACKING chain
#    (single `|`, not `||`) must NOT warn -- shipped in
#    examples/cdp_bot.kry and examples/websocket_client.kry; single `|` is
#    deliberately OUT of scope for W0001 (see the parser's own comment at
#    the warning site) because that exact "first occurrence, newline-led"
#    shape is common and legitimate for bit-packing, unlike `||`.
cat > "$TMP/pipe_pipe_trap.kry" <<'KRY'
fn main() {
    let a: bool = false
    let b: bool = true
    let c: bool = a
    || b
    println(to_string(c))
}
KRY
out="$("$K" run "$TMP/pipe_pipe_trap.kry" 2>&1)"
if grep -q "warning\[W0001\]" <<<"$out" && grep -q "^true$" <<<"$out"; then
    echo "  ok   newline-led first-occurrence \`||\` warns (W0001) AND the documented merge behavior (c=true) is unchanged"
else
    echo "  FAIL: expected a W0001 warning plus unchanged merge output 'true':"; echo "$out" | sed 's/^/    /'; fail=1
fi

cat > "$TMP/pipe_pipe_chain_ok.kry" <<'KRY'
fn is_digit(c: str) -> bool {
    return c == "0" || c == "1" || c == "2" || c == "3" || c == "4"
        || c == "5" || c == "6" || c == "7" || c == "8" || c == "9"
}
fn main() {
    println(to_string(is_digit("5")))
}
KRY
out="$("$K" check "$TMP/pipe_pipe_chain_ok.kry" 2>&1)"
if ! grep -q "W0001" <<<"$out"; then
    echo "  ok   an established multi-line \`||\` chain (is_digit-style) does not false-positive"
else
    echo "  FAIL: legitimate multi-line boolean-or chain triggered W0001:"; echo "$out" | sed 's/^/    /'; fail=1
fi

cat > "$TMP/pipe_bitpack_ok.kry" <<'KRY'
fn pack(a: i64, b: i64, c: i64, d: i64) -> i64 {
    let v = (a << 24)
        | (b << 16)
        | (c << 8)
        | d
    return v
}
fn main() {
    println(to_string(pack(1, 2, 3, 4)))
}
KRY
out="$("$K" check "$TMP/pipe_bitpack_ok.kry" 2>&1)"
if ! grep -q "W0001" <<<"$out"; then
    echo "  ok   a multi-line single-\`|\` bitwise-or bit-packing chain does not false-positive"
else
    echo "  FAIL: legitimate multi-line bitwise-or chain triggered W0001:"; echo "$out" | sed 's/^/    /'; fail=1
fi

if "$K" explain W0001 2>&1 | grep -q "^W0001:"; then
    echo "  ok   kryos explain W0001 resolves"
else
    echo "  FAIL: kryos explain W0001 did not resolve"; fail=1
fi

# 8. LEDGER item 24: `let x: any = <bool>` used to fail the LLVM AOT build
#    outright ("%_0 defined with type 'i1' but expected 'i64'") while the
#    identical source silently misrendered on the Cranelift JIT (printed
#    `1`, not `true`) -- a crash on one backend paired with a silent wrong
#    answer on the other, for the SAME program. `any` has no runtime type
#    tag (item 6, ABI-blocked design note) and a `bool` (i1) or `f64`
#    (double) value's native representation is not i64-shaped like the
#    erased slot expects, so this is now a clean compile-time rejection
#    (E0110) instead of letting either backend discover it downstream. Pin
#    BOTH shapes and BOTH backends so a regression cannot silently reopen
#    either half of the divergence.
cat > "$TMP/any_bool_direct.kry" <<'KRY'
fn main() {
    let x: any = true
    println(to_string(x))
}
KRY
check_out="$("$K" check "$TMP/any_bool_direct.kry" 2>&1)"
run_out="$("$K" run "$TMP/any_bool_direct.kry" 2>&1)"
build_out="$("$K" build --release "$TMP/any_bool_direct.kry" -o "$TMP/any_bool_direct.exe" 2>&1)"
if grep -q "E0110" <<<"$check_out" && grep -q "E0110" <<<"$run_out" && grep -q "E0110" <<<"$build_out"; then
    echo "  ok   \`let x: any = true\` is rejected with E0110 on check/run/build, not a silent-wrong/crash split"
else
    echo "  FAIL: expected E0110 from check+run+build, got:"
    echo "$check_out" | sed 's/^/    check: /'
    echo "$run_out" | sed 's/^/    run:   /'
    echo "$build_out" | sed 's/^/    build: /'
    fail=1
fi

cat > "$TMP/any_f64_direct.kry" <<'KRY'
fn main() {
    let x: any = 3.14
    println(to_string(x))
}
KRY
check_out="$("$K" check "$TMP/any_f64_direct.kry" 2>&1)"
if grep -q "E0110" <<<"$check_out"; then
    echo "  ok   \`let x: any = 3.14\` (same class -- f64 also isn't i64-shaped) is rejected with E0110"
else
    echo "  FAIL: expected E0110, got:"; echo "$check_out" | sed 's/^/    /'; fail=1
fi

# No over-rejection: an i64/str value (already i64-shaped: a plain value or
# a pointer) into an `any` slot must keep compiling and running clean.
cat > "$TMP/any_i64_str_ok.kry" <<'KRY'
fn main() {
    let a: any = 42
    let b: any = "hello"
    println(to_string(a))
}
KRY
check_out="$("$K" check "$TMP/any_i64_str_ok.kry" 2>&1)"
if [ -z "$check_out" ]; then
    echo "  ok   \`let x: any = <i64|str>\` still compiles clean (no over-rejection)"
else
    echo "  FAIL: legitimate i64/str-into-any rejected:"; echo "$check_out" | sed 's/^/    /'; fail=1
fi

if [ $fail -eq 0 ]; then
    echo "diagnostics-gate: PASS"
else
    echo "diagnostics-gate: FAIL"
fi
exit $fail
