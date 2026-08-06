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

# 35-42. ROUND 5: the decoy-vs-real companion attack (round 4's
#        `find_companion_container_arg` shape-based relief, matching a
#        callback's DECLARED element type against another parameter's
#        DECLARED container type first-match-wins, was defeated by an EMPTY
#        DECOY container of the same declared shape -- the real container
#        went uncharged). Deleted with no shape-based successor; the
#        replacement (`hot_param_companions`) traces the callee's OWN fixed
#        body instead of the call site's argument shapes, so a decoy cannot
#        change the answer. Every variant below must be REJECTED, under
#        BOTH enforcement modes, across the decoy shape itself (a second
#        array, a map, a container read out of another container, 3+
#        containers, the predicate/mapper-style std::iter siblings, a
#        method receiver with >2 arrays) and the calling CONTEXT (an actor
#        message handler, a `spawn` capture, a `dyn Trait` method).
for shape in decoy_map_companion decoy_container_read_source decoy_three_containers decoy_iter_siblings decoy_method_receiver decoy_actor_message decoy_spawn_capture decoy_dyn_trait_method; do
    f="tests/security/cap_escape_$shape.kry"
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "$f" >"$TMP/decoy_${shape}_$mode_flag" 2>&1; then
            echo "  FAIL: decoy-companion [$shape] escape COMPILED [$mode_flag]"
            fail=1
        elif grep -q "E0507" "$TMP/decoy_${shape}_$mode_flag"; then
            echo "  ok   decoy-companion [$shape] escape rejected (E0507) [$mode_flag]"
        else
            echo "  FAIL: escape rejected, but NOT for the capability reason [$shape, $mode_flag]:"
            tail -3 "$TMP/decoy_${shape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 43. ROUND 5's SECOND, independent bug: the long-standing "a hot argument
#     that is one of the CURRENT function's own parameters defers its
#     charge to that function's own call sites" rule is unsound when the
#     deferring function narrows its OWN scope with `deny!` between
#     receiving the parameter and invoking it -- confirmed with NO decoy,
#     NO generic, NO container at all, the plainest possible direct forward.
#     Must be REJECTED under both modes.
cat > "$TMP/scope_defer_escape.kry" <<'KRY'
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}
fn zero_cap_tool(reader: fn() -> str) -> str {
    return reader()
}
fn outer(reader: fn() -> str) -> str {
    deny!(fs:read) {
        return zero_cap_tool(reader)
    }
}
@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("tests/security/secret_for_closure_launder.txt")
    println("recovered INSIDE deny via a scope-deferred parameter: " + outer(reader))
}
KRY
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag "$TMP/scope_defer_escape.kry" >"$TMP/scope_defer" 2>&1; then
        echo "  FAIL: scope-narrowed deferred-param escape COMPILED [$mode_flag]"
        fail=1
    elif grep -q "E0507" "$TMP/scope_defer"; then
        echo "  ok   scope-narrowed deferred-param escape rejected (E0507) [$mode_flag]"
    else
        echo "  FAIL: escape rejected, but NOT for the capability reason [$mode_flag]:"
        tail -3 "$TMP/scope_defer" | sed 's/^/    /'; fail=1
    fi
done

# 44. No-cascade complement to #35-43: the SAME transparent-forwarding-lambda
#     shape the decoy attack targets, over an array of PURE closures, called
#     from INSIDE a `deny!` block narrowing an UNRELATED capability, must
#     still need zero annotation -- guards against the scope-depth fix
#     (#43) over-firing on `resolve_closure_caps`'s purely structural
#     self-classification of a fresh lambda literal.
cat > "$TMP/decoy_nocascade.kry" <<'KRY'
fn pure_a() -> str { return "a" }
fn pure_b() -> str { return "b" }
fn apply_to_second<T>(decoy: [T], real: [T], f: fn(T) -> str) -> str {
    return f(real[0])
}
fn main() {
    let real: [fn() -> str] = [pure_a, pure_b]
    let decoy: [fn() -> str] = []
    deny!(net) {
        println(apply_to_second(decoy, real, |c| c()))
    }
}
KRY
if "$K" check --strict-capabilities "$TMP/decoy_nocascade.kry" >"$TMP/decoy_nc" 2>&1; then
    echo "  ok   decoy-companion fix over pure closures inside an unrelated deny! needs no annotation (no cascade)"
else
    echo "  FAIL: the round-5 fix cascaded into a pure-closure companion call inside an unrelated deny!:"
    tail -5 "$TMP/decoy_nc" | sed 's/^/    /'; fail=1
fi

# 45. LEDGER item 18: a privileged closure STORED into an actor's own state
#     field by one message (`h.stash(reader)`) and read+invoked from a
#     SEPARATE, independently-dispatched message (`h.invoke()`) must NOT
#     defeat `deny!(fs:read)`, under EITHER enforcement mode. This is the
#     actor-state-storage residual of check #4/#7-10: `self.<field>()` is not
#     covered by the ordinary struct-field-mutation tracker (`self` is never
#     a locally-tracked container literal), so it must fall closed to `all`
#     on its own -- see `resolve_actor_self_field_invoke_caps` in
#     kryos-capabilities/src/checker.rs.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/attack_actor_state_stored_closure.kry \
        >"$TMP/actor_state_$mode_flag" 2>&1; then
        echo "  FAIL: actor-state-stored-closure escape COMPILED [$mode_flag]"
        fail=1
    elif grep -q "E0507" "$TMP/actor_state_$mode_flag"; then
        echo "  ok   actor-state-stored-closure escape rejected (E0507) [$mode_flag]"
    else
        echo "  FAIL: escape rejected, but NOT for the capability reason [$mode_flag]:"
        tail -3 "$TMP/actor_state_$mode_flag" | sed 's/^/    /'
        fail=1
    fi
done

# 46. No-cascade complement to #45: an actor with a fn-typed state field that
#     is NEVER invoked (only stored/read-and-passed-around, or invoked with
#     the actor properly annotated `@capabilities(all)`) must not spuriously
#     break unrelated actor usage that has nothing to do with the escape --
#     an ordinary actor with only SCALAR state, dispatched from a `deny!`
#     block narrowing an unrelated capability, still needs zero annotation.
cat > "$TMP/actor_state_nocascade.kry" <<'KRY'
actor Counter {
    total: i64

    fn bump(self, n: i64) {
        self.total = self.total + n
    }
}
fn main() {
    let c = Counter()
    deny!(net) {
        c.bump(5)
        sleep(10)
    }
    println("ok")
}
KRY
if "$K" check --strict-capabilities "$TMP/actor_state_nocascade.kry" >"$TMP/actor_nc" 2>&1; then
    echo "  ok   ordinary scalar-state actor dispatch inside an unrelated deny! needs no annotation (no cascade)"
else
    echo "  FAIL: the actor-state fix cascaded into ordinary scalar-state actor dispatch:"
    tail -5 "$TMP/actor_nc" | sed 's/^/    /'; fail=1
fi

# 47. LEDGER item 18 residual (structural sweep, 2026-08-05): ONE level of
#     struct indirection (`self.b.f()` where `b: Box`, `Box.f: fn()->str`)
#     defeated item 18's fix entirely -- see attack_actor_state_nested_field
#     _closure.kry for the full root-cause writeup. Must reject at ANY
#     access depth, both enforcement modes.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/attack_actor_state_nested_field_closure.kry \
        >"$TMP/nested_$mode_flag" 2>&1; then
        echo "  FAIL: nested-actor-state-field closure escape COMPILED [$mode_flag]"
        fail=1
    elif grep -q "E0507" "$TMP/nested_$mode_flag"; then
        echo "  ok   nested-actor-state-field closure escape rejected (E0507) [$mode_flag]"
    else
        echo "  FAIL: escape rejected, but NOT for the capability reason [$mode_flag]:"
        tail -3 "$TMP/nested_$mode_flag" | sed 's/^/    /'
        fail=1
    fi
done

# 48. Adversarial variant of #47: alias the fn-bearing state field into a
#     LOCAL before invoking through it (`let x = self.b; x.f()`) -- see
#     attack_actor_state_aliased_local_closure.kry.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/attack_actor_state_aliased_local_closure.kry \
        >"$TMP/alias_$mode_flag" 2>&1; then
        echo "  FAIL: aliased-local actor-state closure escape COMPILED [$mode_flag]"
        fail=1
    elif grep -q "E0507" "$TMP/alias_$mode_flag"; then
        echo "  ok   aliased-local actor-state closure escape rejected (E0507) [$mode_flag]"
    else
        echo "  FAIL: escape rejected, but NOT for the capability reason [$mode_flag]:"
        tail -3 "$TMP/alias_$mode_flag" | sed 's/^/    /'
        fail=1
    fi
done

# 49. LEDGER item 10 (HIGHEST PRIORITY, blocked the "capability-safe" launch
#     claim): a zero-capability wrapper function that returns a FRESH
#     closure literal calling its own (captured) fn-typed parameter --
#     `fn wrap_once(inner: fn()->str) -> fn()->str { return || inner() }` --
#     must not defeat deny!() when the wrapped closure is invoked later at a
#     wholly separate call site. See cap_escape_closure_wraps_closure.kry.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/cap_escape_closure_wraps_closure.kry \
        >"$TMP/wrap_$mode_flag" 2>&1; then
        echo "  FAIL: wrapper-closure escape COMPILED [$mode_flag]"
        fail=1
    elif grep -q "E0507" "$TMP/wrap_$mode_flag"; then
        echo "  ok   wrapper-closure escape rejected (E0507) [$mode_flag]"
    else
        echo "  FAIL: escape rejected, but NOT for the capability reason [$mode_flag]:"
        tail -3 "$TMP/wrap_$mode_flag" | sed 's/^/    /'
        fail=1
    fi
done

# 50. No-cascade complement to #47-49: an ordinary zero-capability wrapper
#     decorator (the exact `wrap_once` shape, legitimate use) must still
#     compile with NO annotation when the wrapped closure genuinely carries
#     no authority, and a nested actor-state struct field that is NOT
#     fn-bearing must not spuriously fall into the new checks.
cat > "$TMP/wrap_nocascade.kry" <<'KRY'
fn pure_greeter() -> fn() -> str {
    return || "hello"
}
fn wrap_once(inner: fn() -> str) -> fn() -> str {
    return || inner()
}
struct Box2 {
    n: i64
}
actor Holder2 {
    b: Box2
    fn stash(self, nb: Box2) { self.b = nb }
    fn read(self) { println(to_string(self.b.n)) }
}
fn main() {
    let greeter = pure_greeter()
    let wrapped = wrap_once(greeter)
    deny!(fs:read) {
        println(wrapped())
        let h = Holder2()
        h.stash(Box2 { n: 42 })
        h.read()
        sleep(10)
    }
}
KRY
if "$K" check --strict-capabilities "$TMP/wrap_nocascade.kry" >"$TMP/wrap_nc" 2>&1; then
    echo "  ok   pure wrapper closure + non-fn-bearing nested actor field need no annotation (no cascade)"
else
    echo "  FAIL: the wrapper-closure/nested-field fixes cascaded into ordinary pure code:"
    tail -8 "$TMP/wrap_nc" | sed 's/^/    /'; fail=1
fi

# 51. RESOURCE-DOS (LEDGER item 19): a generic that pairs its own type
#     parameter into a tuple return, chained (`dup(dup(dup(x)))`), used to
#     make `kryos check` hang for 65s+ at depth 24 with no diagnostic (the
#     real blowup site is `kryos-types`'s `InferenceEngine::resolve`, not
#     MIR lowering -- `kryos check` never reaches MIR lowering at all).
#     Bounded via a 4096-node type-tree budget; assert it now fails FAST
#     (well under the un-bounded ~65s) with the resource-limit diagnostic,
#     not silently or by hanging.
t0=$(date +%s)
if timeout 15 "$K" check tests/security/attack_monomorphization_tuple_doubling_explosion.kry \
    >"$TMP/mono19" 2>&1; then
    echo "  FAIL: monomorphization tuple-doubling explosion compiled clean (should be rejected)"
    fail=1
else
    rc=$?
    t1=$(date +%s)
    elapsed=$((t1 - t0))
    if [ "$rc" -eq 124 ]; then
        echo "  FAIL: monomorphization tuple-doubling explosion HUNG past the 15s bound (no cap fired)"
        fail=1
    elif grep -q "E0113" "$TMP/mono19" && [ "$elapsed" -le 10 ]; then
        echo "  ok   monomorphization tuple-doubling explosion rejected fast (${elapsed}s, error[E0113])"
    else
        echo "  FAIL: rejected, but not cleanly (no E0113) or too slow (${elapsed}s):"
        tail -3 "$TMP/mono19" | sed 's/^/    /'; fail=1
    fi
fi

# 52. RESOURCE-DOS (LEDGER item 23): a genuinely self-recursive generic
#     function whose recursive call instantiates a NEW, larger concrete type
#     at every level (`fn f<T>(x: T) { f(wrap(x)) }`) used to grow past
#     3.2GB RSS in 15s and keep climbing, with no cap and no diagnostic --
#     distinct from #51: the source is FIXED-length and the blowup comes
#     from the compiler's OWN recursive `monomorphize` call stack (MIR
#     lowering), only reached by `run`/`build`, not `check`. Bounded via a
#     300-deep monomorphization-recursion-depth cap; assert it now fails
#     FAST with the resource-limit diagnostic instead of hanging/OOMing.
t0=$(date +%s)
if timeout 15 "$K" run tests/security/attack_monomorphization_self_recursive_growth.kry \
    >"$TMP/mono23" 2>&1; then
    echo "  FAIL: self-recursive type-growing generic ran to completion (should be rejected)"
    fail=1
else
    rc=$?
    t1=$(date +%s)
    elapsed=$((t1 - t0))
    if [ "$rc" -eq 124 ]; then
        echo "  FAIL: self-recursive type-growing generic HUNG/OOMed past the 15s bound (no cap fired)"
        fail=1
    elif grep -q "E0113" "$TMP/mono23" && [ "$elapsed" -le 10 ]; then
        echo "  ok   self-recursive type-growing generic rejected fast (${elapsed}s, error[E0113])"
    else
        echo "  FAIL: rejected, but not cleanly (no E0113) or too slow (${elapsed}s):"
        tail -3 "$TMP/mono23" | sed 's/^/    /'; fail=1
    fi
fi

[ $fail -eq 0 ] && echo "security-gate: PASS" || echo "security-gate: FAIL"
exit $fail
