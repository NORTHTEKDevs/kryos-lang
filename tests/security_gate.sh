#!/usr/bin/env bash
# Security gate: capability attenuation must actually attenuate.
#
# Kryos is deployed as capability-attenuated infrastructure for agent tooling.
# If a program declaring NO capabilities can reach memory it was never handed,
# every attenuation guarantee above it is void. This gate asserts the boundary
# holds, in BOTH directions -- the escape is rejected AND legitimate declared
# use still works, so the fix cannot be "reject everything".
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# 1. The escape must NOT compile.
if "$K" run tests/security/cap_escape_raw_memory.kry >"$TMP/esc" 2>&1; then
    echo "  FAIL: the raw-memory capability escape COMPILED AND RAN"
    grep -i recovered "$TMP/esc" | head -2 | sed 's/^/    /'
    fail=1
elif grep -q "requires \`ffi\`" "$TMP/esc"; then
    echo "  ok   raw-memory escape rejected (requires \`ffi\`)"
else
    echo "  FAIL: escape rejected, but NOT for the capability reason:"
    tail -3 "$TMP/esc" | sed 's/^/    /'
    fail=1
fi

# 2. Declared use must still work (guards against over-rejection).
cat > "$TMP/ok.kry" <<'KRY'
@capabilities(ffi)
fn main() {
  let p = alloc(32)
  ptr_write_i64(p, 0, 7)
  println(to_string(ptr_read_i64(p, 0)))
  free_bytes(p, 32)
}
KRY
if [ "$("$K" run "$TMP/ok.kry" 2>/dev/null | tail -1)" = "7" ]; then
    echo "  ok   @capabilities(ffi) still permits raw memory"
else
    echo "  FAIL: declared ffi code was rejected -- the gate over-rejects"; fail=1
fi

# 3. The stdlib must NOT cascade: a plain std::json user needs no ffi.
cat > "$TMP/nocascade.kry" <<'KRY'
use std::json::{parse, stringify}
fn main() { println(stringify(parse("{{\"a\":1}}"))) }
KRY
if "$K" check "$TMP/nocascade.kry" >"$TMP/nc" 2>&1; then
    echo "  ok   stdlib users need no ffi (no cascade)"
else
    echo "  FAIL: gating cascaded out of the stdlib:"; tail -3 "$TMP/nc" | sed 's/^/    /'; fail=1
fi


# 4. The closure/fn-value laundering escape must NOT compile under EITHER
#    enforcement mode. See tests/security/cap_escape_closure_launder_deny.kry
#    for why the DENY-BLOCK variant, not the original repro, is the decisive
#    proof (the original's `main` legitimately holds `fs:read` regardless).
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/cap_escape_closure_launder_deny.kry \
        >"$TMP/closure_$mode_flag" 2>&1; then
        echo "  FAIL: closure-laundering deny!(fs:read) escape COMPILED [$mode_flag]"
        fail=1
    elif grep -q "E0507" "$TMP/closure_$mode_flag"; then
        echo "  ok   closure-laundering deny!(fs:read) escape rejected (E0507) [$mode_flag]"
    else
        echo "  FAIL: escape rejected, but NOT for the capability reason [$mode_flag]:"
        tail -3 "$TMP/closure_$mode_flag" | sed 's/^/    /'
        fail=1
    fi
done

# 5. No cascade: std::iter HOFs (map/filter/fold -- every one calls its
#    callback parameter directly, so each is "hot") must still compile with
#    NO annotation when the closure argument is pure. This is the naive
#    "Capability::All on any fn-typed param" fix's rejected failure mode --
#    verify the real fix does not reintroduce it.
cat > "$TMP/hof_nocascade.kry" <<'KRY'
use std::iter::{map, filter, fold}
fn main() {
    let xs: [i64] = [1, 2, 3, 4, 5]
    let doubled = map(xs, |x| x * 2)
    let evens = filter(xs, |x| x % 2 == 0)
    let total = fold(xs, 0, |acc, x| acc + x)
    println(to_string(total) + to_string(len(doubled)) + to_string(len(evens)))
}
KRY
if "$K" check --strict-capabilities "$TMP/hof_nocascade.kry" >"$TMP/hof_nc" 2>&1; then
    echo "  ok   std::iter HOFs with pure closures need no annotation (no cascade)"
else
    echo "  FAIL: the fix cascaded into ordinary HOF usage:"; tail -5 "$TMP/hof_nc" | sed 's/^/    /'; fail=1
fi

# 6. Soundness complement to #5: a PRIVILEGED closure passed through the SAME
#    HOF must still correctly require the capability at the call site (proves
#    #5 passes because the closure is genuinely pure, not because the whole
#    mechanism is inert).
cat > "$TMP/hof_privileged.kry" <<'KRY'
use std::iter::{map}
fn main() {
    let paths: [str] = ["tests/security/secret_for_closure_launder.txt"]
    let contents = map(paths, |p| file_read(p))
    println(contents[0])
}
KRY
if "$K" check --strict-capabilities "$TMP/hof_privileged.kry" >"$TMP/hof_priv" 2>&1; then
    echo "  FAIL: a privileged closure through map() compiled with no fs:read declared"
    fail=1
elif grep -q "fs:read" "$TMP/hof_priv"; then
    echo "  ok   a privileged closure through map() correctly requires fs:read"
else
    echo "  FAIL: rejected, but not for fs:read:"; tail -3 "$TMP/hof_priv" | sed 's/^/    /'; fail=1
fi

# 7-10. Container residual of the closure/fn-value laundering escape (LEDGER
#       item 1): a closure stored in a struct field / array element / map
#       value / nested combination, read back out and invoked, must NOT
#       compile under EITHER enforcement mode -- same decisive-proof shape as
#       check #4 (a deny!(fs:read) block the escape must not defeat).
for shape in container array map nested; do
    f="tests/security/cap_escape_closure_launder_$shape.kry"
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "$f" >"$TMP/container_${shape}_$mode_flag" 2>&1; then
            echo "  FAIL: container-closure-launder [$shape] escape COMPILED [$mode_flag]"
            fail=1
        elif grep -q "E0507" "$TMP/container_${shape}_$mode_flag"; then
            echo "  ok   container-closure-launder [$shape] escape rejected (E0507) [$mode_flag]"
        else
            echo "  FAIL: escape rejected, but NOT for the capability reason [$shape, $mode_flag]:"
            tail -3 "$TMP/container_${shape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 11. No cascade complement to #7-10: a container of PURE closures (the
#     legitimate plugin-registry / router-table / dispatch-map shape this
#     residual matters for) must still compile with NO annotation.
cat > "$TMP/registry_nocascade.kry" <<'KRY'
struct Registry { reader: fn() -> str }
fn pure_reader() -> str { return "no secrets here" }
fn zero_cap_tool(reg: Registry) -> str { return reg.reader() }
fn zero_cap_array(readers: [fn() -> str]) -> str { return readers[0]() }
fn zero_cap_map(handlers: map<str, fn() -> str>) -> str { return handlers["x"]() }
fn main() {
    let reg = Registry { reader: pure_reader }
    let readers: [fn() -> str] = [pure_reader]
    let handlers: map<str, fn() -> str> = {"x": pure_reader}
    println(zero_cap_tool(reg) + zero_cap_array(readers) + zero_cap_map(handlers))
}
KRY
if "$K" check --strict-capabilities "$TMP/registry_nocascade.kry" >"$TMP/registry_nc" 2>&1; then
    echo "  ok   struct/array/map registries of pure closures need no annotation (no cascade)"
else
    echo "  FAIL: the container fix cascaded into a pure-closure registry:"
    tail -5 "$TMP/registry_nc" | sed 's/^/    /'; fail=1
fi

# 12-19. LIVE bypass of the container-literal fix (checks #7-10): a
#        container built empty/placeholder then MUTATED afterward (push,
#        map/array index-assign, struct field-assign, and nested
#        combinations of these, plus a std::collections wrapper and a HOF
#        forwarding through a named adapter function) was invisible to the
#        literal tracker -- worse than the documented `Unknown` fallback,
#        because the STALE (usually empty) initial snapshot survived and
#        confidently asserted "no authority here". Same decisive-proof shape
#        as #4/#7-10 (a deny!(fs:read) block the escape must not defeat).
for shape in push map_insert index_assign field_mutate nested_push map_of_arrays stdlib_collection hof_forward; do
    f="tests/security/cap_escape_closure_launder_$shape.kry"
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "$f" >"$TMP/mut_${shape}_$mode_flag" 2>&1; then
            echo "  FAIL: container-mutation-launder [$shape] escape COMPILED [$mode_flag]"
            fail=1
        elif grep -q "E0507" "$TMP/mut_${shape}_$mode_flag"; then
            echo "  ok   container-mutation-launder [$shape] escape rejected (E0507) [$mode_flag]"
        else
            echo "  FAIL: escape rejected, but NOT for the capability reason [$shape, $mode_flag]:"
            tail -3 "$TMP/mut_${shape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 20. No cascade complement to #12-19: a registry of PURE closures built via
#     the SAME mutation shapes (push / map-insert / field-assign /
#     index-assign) -- not just literal construction -- must still compile
#     with NO annotation. Guards against the mutation-tracking fix
#     over-invalidating (falling back to `Unknown` -> requiring `all`) for
#     ordinary, non-privileged mutation-built containers.
cat > "$TMP/mut_nocascade.kry" <<'KRY'
struct Registry { handler: fn() -> str }
fn pure_a() -> str { return "a" }
fn pure_b() -> str { return "b" }
fn call_nth(tools: [fn() -> str], i: i64) -> str { return tools[i]() }
fn call_key(m: map<str, fn() -> str>, k: str) -> str { return m[k]() }
fn call_field(r: Registry) -> str { return r.handler() }
fn main() {
    let mut tools: [fn() -> str] = []
    tools = push(tools, pure_a)
    let mut m: map<str, fn() -> str> = {}
    m["k"] = pure_b
    let mut r = Registry { handler: pure_a }
    r.handler = pure_b
    let mut idxed: [fn() -> str] = [pure_a, pure_a]
    idxed[0] = pure_b
    println(call_nth(tools, 0) + call_key(m, "k") + call_field(r) + call_nth(idxed, 0))
}
KRY
if "$K" check --strict-capabilities "$TMP/mut_nocascade.kry" >"$TMP/mut_nc" 2>&1; then
    echo "  ok   mutation-built registries of pure closures need no annotation (no cascade)"
else
    echo "  FAIL: the mutation-tracking fix cascaded into a pure-closure registry:"
    tail -5 "$TMP/mut_nc" | sed 's/^/    /'; fail=1
fi

# 21-31. FAIL-CLOSED HARDENING (three failed enumeration rounds -- see
#        docs/capability-roadmap.md and tools/loop/LEDGER.md): the checker's
#        entire enforcement mechanism up to this fix was keyed on a CALL TO
#        A NAMED FUNCTION (so `hot_params` could look it up by name). A
#        first-class fn-value invoked DIRECTLY -- a bare local (no
#        parameter, no container, not even an intermediate function call)
#        -- was never checked AT ALL, no matter what it carried. This is
#        structurally different from #1-20 above (all of which route
#        through a named-function parameter boundary); the fix inverts the
#        default so ANY unresolvable direct invocation requires `all`,
#        closing every shape below without enumerating shapes.
for shape in direct_local_call struct_field_direct_call container_chained_direct container_intermediate_local hof_inline_lambda collections_deque_dict option_result_payload user_hof_three_level; do
    f="tests/security/cap_escape_$shape.kry"
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "$f" >"$TMP/fc_${shape}_$mode_flag" 2>&1; then
            echo "  FAIL: fail-closed-hardening [$shape] escape COMPILED [$mode_flag]"
            fail=1
        elif grep -q "E0507" "$TMP/fc_${shape}_$mode_flag"; then
            echo "  ok   fail-closed-hardening [$shape] escape rejected (E0507) [$mode_flag]"
        else
            echo "  FAIL: escape rejected, but NOT for the capability reason [$shape, $mode_flag]:"
            tail -3 "$TMP/fc_${shape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 32. Every std::iter elementwise HOF sibling (filter/fold/reduce/find), not
#     just map -- a privileged closure passed directly as the callback must
#     be rejected for each of them under BOTH directions.
if "$K" check tests/security/cap_escape_hof_siblings.kry >"$TMP/sib1" 2>&1; then
    echo "  FAIL: hof-siblings escape COMPILED"
    fail=1
elif [ "$(grep -c 'E0507' "$TMP/sib1")" -ge 4 ]; then
    echo "  ok   hof-siblings (filter/fold/reduce/find) all rejected (E0507) []"
else
    echo "  FAIL: hof-siblings did not reject all 4 (got $(grep -c 'E0507' "$TMP/sib1")):"
    tail -5 "$TMP/sib1" | sed 's/^/    /'; fail=1
fi
if "$K" check --strict-capabilities tests/security/cap_escape_hof_siblings.kry >"$TMP/sib2" 2>&1; then
    echo "  FAIL: hof-siblings escape COMPILED [--strict-capabilities]"
    fail=1
elif [ "$(grep -c 'E0507' "$TMP/sib2")" -ge 4 ]; then
    echo "  ok   hof-siblings (filter/fold/reduce/find) all rejected (E0507) [--strict-capabilities]"
else
    echo "  FAIL: hof-siblings did not reject all 4 [--strict-capabilities]:"
    tail -5 "$TMP/sib2" | sed 's/^/    /'; fail=1
fi

# 33. Fail-closed hardening also holds under --capabilities-mode=permissive
#     (an annotated `main` is still an enforcing boundary there).
if "$K" check --capabilities-mode=permissive tests/security/cap_escape_permissive_mode.kry >"$TMP/perm" 2>&1; then
    echo "  FAIL: fail-closed-hardening [permissive_mode] escape COMPILED"
    fail=1
elif grep -q "E0507" "$TMP/perm"; then
    echo "  ok   fail-closed-hardening [permissive_mode] escape rejected (E0507)"
else
    echo "  FAIL: escape rejected, but NOT for the capability reason [permissive_mode]:"
    tail -3 "$TMP/perm" | sed 's/^/    /'; fail=1
fi

# 34. No-cascade complement to #25 (hof_inline_lambda): the SAME
#     `map(tools, |f| f())` shape with PURE closures (the tested,
#     legitimate "map over an array of functions" conformance pattern) must
#     still compile with ZERO annotation.
if "$K" check --strict-capabilities tests/security/cap_escape_hof_inline_lambda_nocascade.kry >"$TMP/hil_nc" 2>&1; then
    echo "  ok   inline-lambda-forwarding HOF with pure closures needs no annotation (no cascade)"
else
    echo "  FAIL: the transparent-forwarding-lambda fix cascaded into a pure closure array:"
    tail -5 "$TMP/hil_nc" | sed 's/^/    /'; fail=1
fi

[ $fail -eq 0 ] && echo "security-gate: PASS" || echo "security-gate: FAIL"
exit $fail
