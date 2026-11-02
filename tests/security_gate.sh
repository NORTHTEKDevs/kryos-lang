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
# Honor KRYOS_BIN like the other gates do, so a caller pointing at a specific
# binary is not silently ignored (the CI Windows job passes it explicitly).
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# 0. A `select` timeout is a branch like any other. All four Stmt::Select
#    arms in the capability checker dropped the `timeout` field, so neither
#    its body nor its duration expression was ever walked: a gated builtin
#    there ran with nothing declared, in all three modes.
#    Only the two ENFORCING modes are asserted: `permissive` checks nothing
#    but annotated functions by design, and these probes' `main` carries no
#    annotation, so it is not a boundary there.
for esc in attack_select_timeout_body_zero_cap attack_select_timeout_duration_zero_cap; do
    esc_ok=1
    for mode in "" "--strict-capabilities"; do
        if "$K" check $mode "tests/security/$esc.kry" >"$TMP/sel" 2>&1; then
            echo "  FAIL: $esc.kry passed \`check $mode\` -- a select timeout runs unchecked"
            esc_ok=0; fail=1
        elif ! grep -qE "E0505|E0507" "$TMP/sel"; then
            echo "  FAIL: $esc.kry rejected by \`check $mode\`, but not for the capability reason:"
            tail -3 "$TMP/sel" | sed 's/^/    /'; esc_ok=0; fail=1
        fi
    done
    [ $esc_ok -eq 1 ] && echo "  ok   $esc.kry rejected in both enforcing modes"
done
if "$K" check tests/security/ctl_select_timeout_declared_cap.kry >"$TMP/selc" 2>&1; then
    echo "  ok   a declared select timeout still compiles (no over-rejection)"
else
    echo "  FAIL: walking the select timeout rejected a program that declares it:"
    tail -3 "$TMP/selc" | sed 's/^/    /'; fail=1
fi

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

# 1b. The same calls one function, or one `use`, away from `main` must not
#     compile either. The gate used to charge only `main` and annotated
#     functions, and only in the root module, so both of these passed every
#     mode and read memory the program was never handed.
for esc in cap_escape_raw_memory_helper cap_escape_raw_memory_import; do
    esc_ok=1
    for mode in "" "--strict-capabilities" "--capabilities-mode=permissive"; do
        if "$K" check $mode "tests/security/$esc.kry" >"$TMP/esc2" 2>&1; then
            echo "  FAIL: $esc.kry passed \`check $mode\` -- raw memory with no ffi"
            esc_ok=0; fail=1
        elif ! grep -q "str_to_ptr\` requires \`ffi\`" "$TMP/esc2"; then
            echo "  FAIL: $esc.kry rejected by \`check $mode\`, but not for the capability reason:"
            tail -3 "$TMP/esc2" | sed 's/^/    /'; esc_ok=0; fail=1
        fi
    done
    [ $esc_ok -eq 1 ] && echo "  ok   $esc.kry rejected in all three modes (requires \`ffi\`)"
done

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


# 53-59. LEDGER item 1's ASSAULT round 3 residual (2026-08-07): a closure
#        stored in a container BOUND FROM A FACTORY FUNCTION'S RETURN (`let
#        registry = build_registry()`), not a struct/array/map LITERAL, then
#        read back out and invoked -- DIRECTLY, in the SAME function as the
#        `deny!(fs:read)` narrowing it. Distinct from checks #7-10 (which
#        pass the container INTO a separate zero-cap helper as a PARAMETER,
#        tracked via the parameter's own declared type): this residual is a
#        purely LOCAL-variable shape, closed by `resolve_method_field_
#        invoke_caps`'s new type-based fail-closed fallback plus
#        `fn_return_container_lits`' precise literal-splicing for zero-arg
#        factory functions. Each shape below must be REJECTED under both
#        enforcement modes; each `_control_benign` sibling (an all-safe
#        registry, no privileged entry anywhere) must still compile clean.
for shape in local_struct_field local_registry_index_field local_map_of_struct_field \
             local_array_direct local_map_direct local_nested_field_array \
             local_nested_two_hop_field; do
    f="tests/security/cap_escape_closure_launder_$shape.kry"
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "$f" >"$TMP/localres_${shape}_$mode_flag" 2>&1; then
            echo "  FAIL: local-container-launder [$shape] escape COMPILED [$mode_flag]"
            fail=1
        elif grep -q "E0507" "$TMP/localres_${shape}_$mode_flag"; then
            echo "  ok   local-container-launder [$shape] escape rejected (E0507) [$mode_flag]"
        else
            echo "  FAIL: escape rejected, but NOT for the capability reason [$shape, $mode_flag]:"
            tail -3 "$TMP/localres_${shape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 60. No-cascade complement to #53-59 (array-of-struct-field shape,
#     precise-literal-splicing path via named-function field values): an
#     ALL-SAFE registry (no privileged entry anywhere), built the identical
#     way (factory function, array of structs with fn-typed fields), must
#     still compile clean with NO annotation.
if "$K" check --strict-capabilities \
    tests/security/cap_escape_closure_launder_local_registry_index_field_control_benign.kry \
    >"$TMP/localres_nc1" 2>&1; then
    echo "  ok   factory-built array-of-struct-field registry of pure closures needs no annotation (no cascade)"
else
    echo "  FAIL: the local-container fix cascaded into a pure-closure factory registry:"
    tail -5 "$TMP/localres_nc1" | sed 's/^/    /'; fail=1
fi

# 61. No-cascade complement to #53-59 (struct-field shape routed through an
#     intermediate LOCAL inside the factory function, not a bare named-
#     function reference -- exercises `substitute_container_lit_identifiers`'
#     local-inlining, not just the literal/named-function case #60 covers).
if "$K" check --strict-capabilities \
    tests/security/cap_escape_closure_launder_local_struct_field_control_benign.kry \
    >"$TMP/localres_nc2" 2>&1; then
    echo "  ok   factory-built struct-field registry via an intermediate local needs no annotation (no cascade)"
else
    echo "  FAIL: the local-container fix cascaded into a pure-closure factory registry (intermediate local):"
    tail -5 "$TMP/localres_nc2" | sed 's/^/    /'; fail=1
fi

# 62-65. LEDGER item 36: the PIPE operator. `a |> f` INVOKES `f`, but the
#        enforcement walk's PipeExpr arm only recursed into both sides and
#        never treated `right` as a CALLEE -- so the pipe spelling of a call
#        was not gated at all while the identical `f(a)` was fully gated. It
#        defeated `deny!()` under BOTH enforcement modes. The escape and its
#        control (same program, `reader(0)` instead of `0 |> reader`) must
#        BOTH be rejected: the control exists to prove the bypass was
#        specific to the PipeExpr shape, not a property of invoking a bare
#        fn-typed local, so if it ever starts COMPILING the general
#        bare-local enforcement has regressed.
for pshape in attack_deny_pipe_bare_ident_call attack_deny_pipe_bare_ident_call_control; do
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "tests/security/$pshape.kry" >"$TMP/pipe_${pshape}_$mode_flag" 2>&1; then
            echo "  FAIL: pipe-callee [$pshape] COMPILED [$mode_flag]"
            fail=1
        elif grep -q "E0507" "$TMP/pipe_${pshape}_$mode_flag"; then
            echo "  ok   pipe-callee [$pshape] rejected (E0507) [$mode_flag]"
        else
            echo "  FAIL: pipe-callee rejected but NOT for the capability reason [$pshape, $mode_flag]:"
            tail -3 "$TMP/pipe_${pshape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 66. No-cascade complement to #62-65: the PARTIAL-APPLICATION pipe forms
#     (`5 |> padd(10)`, chained `3 |> padd(4) |> padd(100)`) are a DIFFERENT
#     shape -- the real callee is the inner named fn, already gated by name.
#     Treating them as a direct fn-value invocation made them demand `all`
#     and falsely rejected this file, which is how the over-rejection was
#     caught. It must keep compiling clean.
if "$K" check tests/conformance/conf_functions.kry >"$TMP/pipe_nocascade" 2>&1; then
    echo "  ok   partial-application pipes (a |> f(b)) still compile clean (no cascade)"
else
    echo "  FAIL: the pipe fix cascaded into partial-application pipes:"
    tail -5 "$TMP/pipe_nocascade" | sed 's/^/    /'; fail=1
fi

# 67-70. LEDGER items 32/38: a FIELD CHAIN INTO A LOCAL invoked as a callee
#        (`pair.1()`, or a for-bound `x.0()`). The fail-closed direct-invoke
#        path existed but was gated on `segments.len() <= 1`, and a field
#        chain resolves to ["pair","1"] -- so it was skipped entirely and the
#        call went ungated under BOTH modes. The guard now keys on the ROOT
#        (a local/param = a first-class value being invoked) rather than the
#        segment count.
for tshape in attack_verify_tuple_call_general attack_r2_tuple_forloop_index_call; do
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "tests/security/$tshape.kry" >"$TMP/tup_${tshape}_$mode_flag" 2>&1; then
            echo "  FAIL: field-chain-callee [$tshape] COMPILED [$mode_flag]"
            fail=1
        elif grep -q "E0507" "$TMP/tup_${tshape}_$mode_flag"; then
            echo "  ok   field-chain-callee [$tshape] rejected (E0507) [$mode_flag]"
        else
            echo "  FAIL: rejected but NOT for the capability reason [$tshape, $mode_flag]:"
            tail -3 "$TMP/tup_${tshape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 71-80. STAGE 2 (capability-row enforcement, kryos-types/check.rs). A deny!
#        block's accumulated capability ROW is now checked against its denied
#        set. The row is charged from the CALLEE'S OWN TYPE at every call
#        site, so a callee reached through an accessor-call receiver, an
#        if/match receiver, a &/* indirection or a struct field is charged
#        identically to a direct call -- there is no expression shape to
#        enumerate and therefore none to miss. These five shapes defeated the
#        shape-directed checker in kryos-capabilities for weeks; item 37 in
#        particular survived three separate mechanical fix attempts.
for rshape in attack_container_via_accessor_fn_call               attack_container_via_accessor_method_call               attack_ifexpr_receiver_field_call               attack_matchexpr_receiver_field_call               attack_deref_borrow_param_defeats_field_resolver; do
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "tests/security/$rshape.kry" >"$TMP/row_${rshape}_$mode_flag" 2>&1; then
            echo "  FAIL: row-enforced escape [$rshape] COMPILED [$mode_flag]"
            fail=1
        else
            echo "  ok   row-enforced escape [$rshape] rejected [$mode_flag]"
        fi
    done
done

# 81-82. Item 34: a two-hop `let` alias of an actor's fn-bearing state field
#        (`let x = self.b; let y = x; y.f()`). Closed by charging the callee's
#        own capability row at the method/handler dispatch site -- previously
#        an actor handler that called `file_read` accumulated {fs:read}
#        correctly while the `main` invoking it accumulated {}, so anything
#        behind a handler call was invisible to enforcement.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/attack_verify_double_alias.kry >"$TMP/dbl_$mode_flag" 2>&1; then
        echo "  FAIL: actor-state double-alias escape COMPILED [$mode_flag]"
        fail=1
    else
        echo "  ok   actor-state double-alias escape rejected [$mode_flag]"
    fi
done

# 83-84. Item 35: a privileged closure passed to a STATIC impl method
#        (`Invoker::run(reader)`). Impl method bodies were checked with no
#        capability-accumulator frame at all -- unlike plain functions and
#        actor handlers -- so an impl method's `own_cap_var` stayed
#        permanently unbound and every call site resolved it to nothing.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/attack_static_method_hotparam_offset.kry >"$TMP/stat_$mode_flag" 2>&1; then
        echo "  FAIL: static-method hot-param escape COMPILED [$mode_flag]"
        fail=1
    else
        echo "  ok   static-method hot-param escape rejected [$mode_flag]"
    fi
done

# 85-86. Item 32: a closure stashed in a TUPLE inside actor state
#        (`pair: (i64, fn() -> str)`), then invoked via `self.pair.1()`.
#        Reading a fn-BEARING actor state field now yields CapRow::Unknown --
#        actor state is mutable storage any handler may write at any prior
#        dispatch, so which closure is in it at a given read is genuinely not
#        statically knowable. Scoped to `self` inside a handler AND to fields
#        whose type actually contains a function, so ordinary data state is
#        untouched.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/attack_verify_tuple_in_state.kry >"$TMP/tstate_$mode_flag" 2>&1; then
        echo "  FAIL: actor-state tuple escape COMPILED [$mode_flag]"
        fail=1
    else
        echo "  ok   actor-state tuple escape rejected [$mode_flag]"
    fi
done

# 87-89. Independent re-probe of LEDGER item 1's closure-container-launder
#        residual (2026-08-12 close-out wave): three container combinations
#        NOT covered by checks #53-59 -- a struct field typed as a MAP of
#        closures (not array, not a bare fn field); a MAP-OF-ARRAYS-of-
#        closures returned WHOLESALE as a literal from a factory function
#        (exercising the literal-splicing path, distinct from
#        cap_escape_closure_launder_map_of_arrays.kry's local-mutation-
#        tracking path); and a THREE-level nesting (array of struct-of-
#        struct holding the closure field), deeper than any existing shape.
#        Each escape must be REJECTED (E0110 and/or E0507, depending on
#        which fail-closed mechanism fires first -- the deny! block's own
#        row check can reject before the call-site resolver even runs) under
#        both enforcement modes.
for shape in local_struct_field_of_map local_map_literal_of_array local_triple_nested; do
    f="tests/security/cap_escape_closure_launder_$shape.kry"
    for mode_flag in "" "--strict-capabilities"; do
        if "$K" check $mode_flag "$f" >"$TMP/newres_${shape}_$mode_flag" 2>&1; then
            echo "  FAIL: container-launder [$shape] escape COMPILED [$mode_flag]"
            fail=1
        elif grep -qE "E0507|E0110" "$TMP/newres_${shape}_$mode_flag"; then
            echo "  ok   container-launder [$shape] escape rejected [$mode_flag]"
        else
            echo "  FAIL: escape rejected, but NOT for the capability reason [$shape, $mode_flag]:"
            tail -3 "$TMP/newres_${shape}_$mode_flag" | sed 's/^/    /'
            fail=1
        fi
    done
done

# 90. No-cascade complement to #87-89: the identical three-level container
#     shape (array-of-struct-of-struct holding a fn-typed field), factory-
#     built, but with NO privileged closure anywhere -- an UNANNOTATED main
#     must compile clean.
if "$K" check --strict-capabilities \
    tests/security/cap_escape_closure_launder_local_triple_nested_control_benign.kry \
    >"$TMP/newres_nc1" 2>&1; then
    echo "  ok   factory-built triple-nested registry of pure closures needs no annotation (no cascade)"
else
    echo "  FAIL: the container-launder fix cascaded into a pure-closure triple-nested factory registry:"
    tail -5 "$TMP/newres_nc1" | sed 's/^/    /'; fail=1
fi

# 91-92. LEDGER item 33: a closure forwarded actor-to-actor as a MESSAGE
#        PARAMETER (never stored in state -- just passed through a handler
#        param and invoked by the receiving actor). Two defects compounded:
#        (a) the handler body bound its params by RE-RESOLVING the param
#        TypeExpr, minting a second capability-row var unrelated to the one
#        registered in the signature's `generic_cap_var_ids` -- so the body
#        charged a var no caller could remap; (b) a caller declared EARLIER
#        than its callee (`actor Sender` before `actor Receiver`) walked its
#        body while the callee's `own_cap_var` was still unbound, snapshotting
#        a bare var its instantiation map could not touch. Both are SILENT:
#        enforcement runs, resolves an open row, and charges zero.
for mode_flag in "" "--strict-capabilities"; do
    if "$K" check $mode_flag tests/security/attack_verify_actor_to_actor_message.kry \
        >"$TMP/a2a_$mode_flag" 2>&1; then
        echo "  FAIL: actor-to-actor message-param escape COMPILED [$mode_flag]"
        fail=1
    elif grep -qE "E0507|E0110" "$TMP/a2a_$mode_flag"; then
        echo "  ok   actor-to-actor message-param escape rejected [$mode_flag]"
    else
        echo "  FAIL: escape rejected, but NOT for the capability reason [$mode_flag]:"
        tail -3 "$TMP/a2a_$mode_flag" | sed 's/^/    /'
        fail=1
    fi
done

# 93. DECLARATION-ORDER INDEPENDENCE for #91-92. The original defect was
#     order-sensitive: the identical program with `Receiver` declared FIRST
#     was already caught, while `Sender` first escaped. Pin BOTH orders so a
#     future regression cannot hide behind source ordering.
cat > "$TMP/a2a_receiver_first.kry" <<'KRY'
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}
actor Receiver {
    fn receive(self, f: fn() -> str) {
        let leaked = f()
        println("LEAK: " + leaked)
    }
}
actor Sender {
    fn relay(self, target: Receiver, f: fn() -> str) {
        target.receive(f)
    }
}
@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("tests/security/secret_for_closure_launder.txt")
    let s = Sender()
    let r = Receiver()
    deny!(fs:read) {
        s.relay(r, reader)
    }
}
KRY
if "$K" check "$TMP/a2a_receiver_first.kry" >"$TMP/a2a_rf" 2>&1; then
    echo "  FAIL: actor-to-actor escape COMPILED with callee declared first"
    fail=1
else
    echo "  ok   actor-to-actor escape rejected in BOTH declaration orders"
fi

# 94. No-cascade complement to #91-92: the identical actor-to-actor message
#     forwarding shape carrying a PURE closure -- the legitimate
#     supervisor/worker message-passing pattern this residual matters for --
#     must still compile with NO annotation. If this goes red, the handler
#     param-binding change is over-charging ordinary actor messages.
cat > "$TMP/a2a_nocascade.kry" <<'KRY'
fn pure_reader() -> str { return "no secrets here" }
actor Worker {
    fn handle(self, f: fn() -> str) {
        println(f())
    }
}
actor Supervisor {
    fn dispatch(self, target: Worker, f: fn() -> str) {
        target.handle(f)
    }
}
fn main() {
    let sup = Supervisor()
    let w = Worker()
    sup.dispatch(w, pure_reader)
}
KRY
if "$K" check --strict-capabilities "$TMP/a2a_nocascade.kry" >"$TMP/a2a_nc" 2>&1; then
    echo "  ok   actor-to-actor forwarding of a pure closure needs no annotation (no cascade)"
else
    echo "  FAIL: the item-33 fix cascaded into pure actor message passing:"
    tail -5 "$TMP/a2a_nc" | sed 's/^/    /'; fail=1
fi

# The item-41 experiment switch must be inert in a default build: it re-opens
# escape_status rows 41a/41b, and one environment variable on a build machine
# used to be enough (the switch now needs the item41-experiment feature).
if KRYOS_NO_HOTPARAM_ALL=1 "$K" check tests/security/attack_realprogram_deny_blocks_builtin_exfil.kry >"$TMP/hp" 2>&1; then
    echo "  FAIL: KRYOS_NO_HOTPARAM_ALL=1 weakens this build: 41a's escape compiled"
    fail=1
elif grep -q "denied in this block\|requires" "$TMP/hp"; then
    echo "  ok   KRYOS_NO_HOTPARAM_ALL is inert in a default build (41a still rejected)"
else
    echo "  FAIL: 41a rejected with the switch set, but not for the capability reason:"
    tail -3 "$TMP/hp" | sed 's/^/    /'; fail=1
fi

# A reassignment is a binding wherever it sits. The builder walked if/for/while/
# select/try/deny and a bare block, then three expression parents (a block, a
# `match` arm, an if-expression branch) and fell through `_ => {}` for every
# other expression form, so `unsafe { f = reader }`, a `match` used as a call
# argument or an operand, a `while` condition, a match scrutinee, a string
# interpolation (and the rest listed below) left `f` charged as `harmless` and
# `f()` inside `deny!(fs:read)` printed the secret. The walk is now exhaustive
# over `Expr` and `Stmt` with no wildcard arm.
REASSIGN_FORMS="match_stmt_block \
    match_in_deny \
    match_enum_arm \
    match_literal_subject \
    block_in_for_iterable \
    block_in_match_scrutinee \
    block_in_method_receiver \
    ifexpr_as_call_arg \
    ifexpr_in_binop \
    match_as_call_arg \
    match_in_array_literal \
    match_in_assign_rhs \
    match_in_cast \
    match_in_elif_cond \
    match_in_if_cond \
    match_in_index \
    match_in_interpolation \
    match_in_let_tuple \
    match_in_map_literal \
    match_in_match_guard \
    match_in_method_arg \
    match_in_pipe \
    match_in_range \
    match_in_ref \
    match_in_struct_literal \
    match_in_unary \
    match_in_while_cond \
    quantum_block \
    unsafe_block \
    unsafe_in_match_arm"
for form in $REASSIGN_FORMS; do
    esc="attack_reassign_local_$form"
    esc_ok=1
    for mode in "" "--strict-capabilities"; do
        if "$K" check $mode "tests/security/$esc.kry" >"$TMP/rm" 2>&1; then
            echo "  FAIL: $esc.kry passed \`check $mode\` -- a reassignment in this expression form is invisible"
            esc_ok=0; fail=1
        elif ! grep -qE "E0505|E0507" "$TMP/rm"; then
            echo "  FAIL: $esc.kry rejected by \`check $mode\`, but not for the capability reason:"
            tail -3 "$TMP/rm" | sed 's/^/    /'; esc_ok=0; fail=1
        fi
    done
    [ $esc_ok -eq 1 ] && echo "  ok   $esc.kry rejected in default and strict mode"
done
# The same is true of a write into a TRACKED CONTAINER (a map element, an array
# element or push, a struct field): `build_local_container_lits_block` listed a few
# statement forms and skipped a `deny!` body, `unsafe`/`quantum` blocks and every
# expression, so the snapshot stayed `harmless`. Same exhaustive walk.
CONTAINER_FORMS="arr_match_arg \
    arr_push_quantum \
    arr_unsafe \
    map_in_deny_body \
    map_match_arg \
    map_unsafe \
    map_while_cond \
    struct_field_ifexpr_arg"
for form in $CONTAINER_FORMS; do
    esc="attack_reassign_container_$form"
    esc_ok=1
    for mode in "" "--strict-capabilities"; do
        if "$K" check $mode "tests/security/$esc.kry" >"$TMP/rm" 2>&1; then
            echo "  FAIL: $esc.kry passed \`check $mode\` -- a container write in this form is invisible"
            esc_ok=0; fail=1
        elif ! grep -qE "E0505|E0507" "$TMP/rm"; then
            echo "  FAIL: $esc.kry rejected by \`check $mode\`, but not for the capability reason:"
            tail -3 "$TMP/rm" | sed 's/^/    /'; esc_ok=0; fail=1
        fi
    done
    [ $esc_ok -eq 1 ] && echo "  ok   $esc.kry rejected in default and strict mode"
done
# NOT an escape: a lambda captures `f` by value, so calling it never changes the
# outer `f` (the program prints "not a secret"). It is rejected anyway because
# the builder's name -> authority map is flat. Pinned so the over-rejection is a
# known, deliberate cost, not a surprise.
for mode in "" "--strict-capabilities"; do
    if "$K" check $mode tests/security/attack_reassign_local_match_lambda_body.kry >"$TMP/rm" 2>&1; then
        echo "  note: attack_reassign_local_match_lambda_body.kry is now accepted by \`check $mode\` (it was a conservative rejection)"
    elif grep -qE "E0505|E0507" "$TMP/rm"; then
        echo "  ok   attack_reassign_local_match_lambda_body.kry rejected by \`check $mode\` (conservative; not an escape)"
    else
        echo "  FAIL: attack_reassign_local_match_lambda_body.kry rejected by \`check $mode\`, but not for the capability reason:"
        tail -3 "$TMP/rm" | sed 's/^/    /'; fail=1
    fi
done
for ctl in ctl_reassign_pure_in_match ctl_reassign_pure_in_exprforms ctl_reassign_pure_container_exprforms; do
    if "$K" check --strict-capabilities "tests/security/$ctl.kry" >"$TMP/rmc" 2>&1; then
        echo "  ok   $ctl.kry accepted (no cascade)"
    else
        echo "  FAIL: the reassignment walk over-rejects $ctl.kry (pure-only reassignments):"
        tail -3 "$TMP/rmc" | sed 's/^/    /'; fail=1
    fi
done

# KRYOS_STDLIB_DIR chooses where `use std::..` is read from. It must not also
# choose which code is exempt from the raw-memory gate: pointing it at the
# directory holding the program's own modules exempted them, so a module
# using str_to_ptr + ptr_byte_at compiled on a default release build.
SEC_DIR="$(cd tests/security && pwd)"
if KRYOS_STDLIB_DIR="$SEC_DIR" "$K" check tests/security/cap_escape_raw_memory_import.kry >"$TMP/sd" 2>&1; then
    echo "  FAIL: KRYOS_STDLIB_DIR=<user modules> exempted them from the raw-memory gate"
    fail=1
elif grep -q "str_to_ptr\` requires \`ffi\`" "$TMP/sd"; then
    echo "  ok   KRYOS_STDLIB_DIR does not make user modules trusted (still requires \`ffi\`)"
else
    echo "  FAIL: rejected with KRYOS_STDLIB_DIR set, but not for the capability reason:"
    tail -3 "$TMP/sd" | sed 's/^/    /'; fail=1
fi
if KRYOS_STDLIB_DIR="$(cd compiler/stdlib && pwd)" "$K" check "$TMP/nocascade.kry" >"$TMP/sdc" 2>&1; then
    echo "  ok   control: the real stdlib named by KRYOS_STDLIB_DIR is still trusted"
else
    echo "  FAIL: KRYOS_STDLIB_DIR=<real stdlib> now over-rejects std::json:"
    tail -3 "$TMP/sdc" | sed 's/^/    /'; fail=1
fi

# LEDGER root R2: a closure stored into a SLOT must widen the slot's row.
#
# The row layer typed an array / map literal by its FIRST element, bound a struct
# field's declaration-global row var (and an enum payload's, an annotated
# binding's, a generic type variable's) to the first closure stored into it, and
# let every later store meet a resolved row, hit the closed/closed no-op in
# `unify`, and vanish. `deny!` judges that row, so a privileged closure stored
# second was never seen. Every program below was accepted in every mode and
# printed the secret on master dcf72e93 (executed).
#
# Items 65 and 68 (a factory that wraps its parameter in a struct field) close with
# the same root: the factory's body stores its OWN parameter var in the field, and
# the call now replays that store with the caller's closure.
#
# The assertion is on the ROW layer specifically (E0110, "denied in this block"):
# the shape layer is blind to these forms, which is why they escaped, and a
# program that is still rejected only by E0507 would hide a row-layer regression.
# `kryos run` must also refuse to run the program and print nothing.
R2_ESCAPES="attack_for_binder_shadow_closure_deny \
    attack_realprogram_registry_list_wrapper \
    attack_realprogram_registry_foreach \
    attack_map_param_alias_invoke_deny \
    attack_array_param_alias_invoke_deny \
    attack_actor_explicit_self_swapped_params_deny \
    attack_match_bound_var_field_invoke \
    attack_index_field_invoke_factory_element_deny \
    attack_slot_array_push_blinded \
    attack_slot_array_index_assign_blinded \
    attack_slot_struct_field_assign_blinded \
    attack_slot_enum_payload_second_store \
    attack_slot_nested_array_literal \
    attack_slot_tuple_array_literal \
    attack_slot_struct_array_field_literal \
    attack_slot_two_struct_instances \
    attack_slot_empty_array_two_pushes \
    attack_slot_array_from_call_push \
    attack_slot_option_cell_reassign \
    attack_slot_tuple_reassign \
    attack_slot_loop_carried_store_via_helper \
    attack_slot_register_helper \
    attack_slot_register_method \
    attack_slot_param_struct_field_store \
    assault_r1_real_program_plugin_platform_stores_first"
# Revise round of PR 38 (LEDGER root R2): SIGNATURE VARS ARE INPUTS.
#
# The independent verifier of the first R2 build found that a callee which builds a
# container from a pure function and then stores its own closure PARAMETER into it
# (`let mut a = [safe]; a = push(a, g); return a`, or `return [safe, g]`) was still
# accepted and printed the secret. The slot WAS widened, but the original
# unify rule also handed the slot's closed row to the source's unbound vars, so
# the parameter's own row var was bound to the empty row and every caller was told
# the parameter is pure. A signature var is an input of the body (it stands for
# whatever the caller passes): the body may never bind it. The same defect hid in
# a registration helper declared after a pure store, in a lambda's annotated
# parameter (bound in the body, and first-call-wins afterwards), and in the
# functions that return one of several function values (the several `return`s of
# a function and the arms of an `if` / `match` were typed by the first one).
R2_ESCAPES="$R2_ESCAPES \
    attack_slot_ret_push_param \
    attack_slot_ret_literal_param \
    attack_slot_ret_middleware_chain \
    attack_slot_ret_two_arrays \
    attack_slot_ret_captures_param \
    attack_slot_ret_struct_items \
    attack_slot_ret_option_cell \
    attack_slot_ret_enum_payload \
    attack_slot_ret_generic_container \
    attack_slot_ret_generic_fn \
    attack_slot_ret_static_method \
    attack_slot_ret_trait_method \
    attack_slot_ret_method_returns \
    attack_slot_ret_lambda_of_container \
    attack_slot_two_returns \
    attack_slot_if_arms \
    attack_slot_match_arms \
    attack_slot_lambda_param_single_call \
    attack_slot_lambda_param_two_calls \
    attack_slot_register_after_pure_store \
    attack_slot_call_after_narrow \
    attack_slot_ret_recursion \
    attack_slot_ret_result_cell \
    attack_slot_ret_fluent_builder \
    attack_slot_ret_rebuild_from_param_array"
# Parameter positions are contravariant (LEDGER root R2, revise round). A lambda
# with an ANNOTATED closure parameter handed to a higher-order function over an
# array the helper was given (`for_each(fns, |g: fn() -> str| g())`) was never
# linked to the array's element row: the unify direction was the function value's,
# not the parameter's. The default mode rejects it through the shape layer; under
# --strict-capabilities nothing else does, so the pin asserts the ROW layer in both.
R2_ESCAPES="$R2_ESCAPES \
    attack_slot_lambda_param_over_container \
    attack_slot_lambda_param_over_container_map"
# A callee that stores a closure into a container it was HANDED (LEDGER item 112 and
# its map variant from the verifier of PR 38). Arrays and maps are shared handles, so
# the store lands in the CALLER's container; the callee used to bind its own
# parameter's element row (so its reads through the parameter were charged only what it
# stored) and the caller's slot never learned. Now the store is recorded and replayed
# into the slot each call's argument came from.
R2_ESCAPES="$R2_ESCAPES \
    attack_slot_param_container_store \
    attack_slot_param_index_assign \
    attack_slot_param_push_inplace \
    attack_slot_param_map_store \
    attack_slot_param_helper_chain \
    attack_slot_param_store_pure_then_read \
    attack_slot_param_store_closed_privileged \
    attack_slot_param_list_push \
    attack_slot_param_mut_ref \
    attack_slot_param_method \
    attack_slot_param_fn_value"
# The lambda form of item 112: a LAMBDA with an annotated array parameter that stores
# its closure parameter into it. A lambda is one monomorphic value, so there is no
# per-call copy of the parameter row to replay; the stores are written into the slot
# of each argument the parameter is bound to.
R2_ESCAPES="$R2_ESCAPES \
    attack_slot_lambda_container_param_store \
    attack_slot_lambda_container_param_push"
for esc in $R2_ESCAPES; do
    esc_ok=1
    for mode in "" "--strict-capabilities"; do
        if "$K" check $mode "tests/security/$esc.kry" >"$TMP/slot" 2>&1; then
            echo "  FAIL: $esc.kry passed \`check $mode\` -- a closure stored into a slot never widened its row"
            esc_ok=0; fail=1
        elif ! grep -q "is denied in this block" "$TMP/slot"; then
            echo "  FAIL: $esc.kry rejected by \`check $mode\`, but not by the row layer (E0110):"
            tail -3 "$TMP/slot" | sed 's/^/    /'; esc_ok=0; fail=1
        fi
    done
    if timeout 60 "$K" run "tests/security/$esc.kry" >"$TMP/slotrun" 2>&1; then
        echo "  FAIL: $esc.kry RAN (exit 0) -- the run must be refused at check time"
        esc_ok=0; fail=1
    elif grep -qE "TOPSECRET|EVIL-SUPPLY-CHAIN-LEAK" "$TMP/slotrun"; then
        echo "  FAIL: $esc.kry printed the secret"
        esc_ok=0; fail=1
    fi
    [ $esc_ok -eq 1 ] && echo "  ok   $esc.kry rejected by the row layer in default and strict mode, run refused"
done
# The twin of every program above has only pure closures in the slot, or denies a
# capability none of the stored closures uses, and must stay accepted. The cost of
# a per-declaration slot is that a registry mixing a pure and a privileged closure
# is judged by the union: the controls below are the cases where that union is
# still harmless. std::iter HOFs take fn params everywhere and must not be charged.
R2_CONTROLS="ctl_slot_pure_registries_in_deny \
    ctl_slot_mixed_registry_other_deny \
    ctl_slot_mixed_registry_outside_deny \
    ctl_slot_std_iter_pure_in_deny \
    ctl_slot_registered_pure_list_plugins \
    ctl_slot_for_binder_shadow_pure \
    ctl_slot_registry_list_wrapper_pure \
    ctl_slot_registry_foreach_pure \
    ctl_slot_map_param_alias_pure \
    ctl_slot_array_param_alias_pure \
    ctl_slot_actor_explicit_self_pure"
R2_CONTROLS="$R2_CONTROLS \
    ctl_slot_ret_push_param_pure \
    ctl_slot_ret_other_cap_deny \
    ctl_slot_two_returns_pure \
    ctl_slot_if_match_arms_pure \
    ctl_slot_lambda_param_pure_calls \
    ctl_slot_register_after_pure_store_pure"
R2_CONTROLS="$R2_CONTROLS \
    ctl_slot_param_store_pure \
    ctl_slot_param_store_privileged_other_array"
R2_CONTROLS="$R2_CONTROLS \
    ctl_slot_lambda_container_param_store_pure"
for ctl in $R2_CONTROLS; do
    ctl_ok=1
    for mode in "" "--strict-capabilities"; do
        if ! "$K" check $mode "tests/security/$ctl.kry" >"$TMP/slotc" 2>&1; then
            echo "  FAIL: slot widening over-rejects $ctl.kry under \`check $mode\`:"
            tail -3 "$TMP/slotc" | sed 's/^/    /'; ctl_ok=0; fail=1
        fi
    done
    [ $ctl_ok -eq 1 ] && echo "  ok   $ctl.kry accepted in default and strict mode (no over-rejection)"
done
# LEDGER root R3: the deny! verdict is taken when the module is done.
#
# `deny!` is judged on the row its body was charged, and that row mentions vars that
# have no binding yet when the block is checked: the row of a parameter (which only a
# reference to the declaration binds), a callee declared below, a field or a global
# that `main` fills after the method reading it was checked, a local array that a loop
# fills after the block. `concrete_bits()` dropped every one of them, so a block that
# calls a closure it was handed, or reads a registry filled later, was judged to need
# no authority. Every program below was accepted in every mode and printed the secret
# on master 732714ab (executed), except attack_deny_param_factory_guard, _lambda_annotated
# and _pick_passthrough, which the shape layer already rejects there and which are pinned
# so the row layer no longer depends on it.
# The six attack_fwd_* programs are R2 attack programs with `main` moved above everything else:
# the helper is declared AFTER the code that calls it, and the reference found nothing to replay.
# Master accepts four of them in every mode; the two attack_fwd_slot_lambda_param_over_container
# twins are rejected there by the shape layer in the default mode and accepted only under
# --strict-capabilities.
#
# The assertion is on the ROW layer ("is denied in this block"): the shape layer is
# blinded by the outer harmless `f`, which is why these escaped, and a program rejected
# only by E0507 would hide a row-layer regression. `kryos run` must refuse the program
# and print nothing.
R3_ESCAPES="attack_deny_double_indirection_wrapper_local \
    assault_r1_real_program_plugin_platform_nopool \
    assault_r1_real_program_plugin_platform \
    attack_deny_in_callee_before_stores \
    attack_deny_in_method_before_stores \
    attack_deny_loop_carried_store_after_block \
    attack_deny_in_callee_after_main \
    attack_deny_in_lambda_defined_before_store \
    attack_deny_global_slot_reader_before_main \
    attack_deny_show_after_add_before_main \
    attack_reassign_local_via_local_closure_return_lambda \
    attack_reassign_local_via_local_closure_return_nested_fn \
    attack_slot_register_after_main \
    attack_slot_param_fn_value_hof_param \
    attack_slot_param_fn_value_struct_field \
    attack_slot_param_fn_value_map_of_ops \
    attack_slot_lambda_wrapper_calls_put \
    attack_fwd_array_param_alias_invoke_deny \
    attack_fwd_map_param_alias_invoke_deny \
    attack_fwd_slot_call_after_narrow \
    attack_fwd_slot_param_store_pure_then_read \
    attack_fwd_slot_lambda_param_over_container \
    attack_fwd_slot_lambda_param_over_container_map"
# The parameter-row family: the privileged closure reaches the block through a row the
# declaration holding the block never binds, by every route a reference can take.
R3_ESCAPES="$R3_ESCAPES \
    attack_deny_param_alias_wrapper \
    attack_deny_param_hof_wrapper \
    attack_deny_param_actor_handler \
    attack_deny_param_impl_method \
    attack_deny_param_struct_field \
    attack_deny_param_two_hops \
    attack_deny_param_recursion \
    attack_deny_param_method_chain \
    attack_deny_param_fn_value_ref \
    attack_deny_param_lambda_annotated \
    attack_deny_param_hof_compose \
    attack_deny_param_hof_compose_apply_after_main \
    attack_deny_param_struct_field_op \
    attack_deny_param_map_of_ops \
    attack_deny_param_factory_op \
    attack_deny_param_lambda_wrapper \
    attack_deny_param_dyn_dispatch \
    attack_deny_param_bound_dispatch \
    attack_deny_param_in_tuple \
    attack_deny_param_in_option \
    attack_deny_param_in_struct \
    attack_deny_param_in_nested_array \
    attack_deny_param_pick_passthrough \
    attack_deny_param_factory_guard \
    attack_deny_late_if_branch_store"
for esc in $R3_ESCAPES; do
    esc_ok=1
    for mode in "" "--strict-capabilities"; do
        if "$K" check $mode "tests/security/$esc.kry" >"$TMP/dn" 2>&1; then
            echo "  FAIL: $esc.kry passed \`check $mode\` -- the deny! verdict was taken on a row with unbound vars"
            esc_ok=0; fail=1
        elif ! grep -q "is denied in this block" "$TMP/dn"; then
            echo "  FAIL: $esc.kry rejected by \`check $mode\`, but not by the row layer (E0110):"
            tail -3 "$TMP/dn" | sed 's/^/    /'; esc_ok=0; fail=1
        fi
    done
    if timeout 60 "$K" run "tests/security/$esc.kry" >"$TMP/dnrun" 2>&1; then
        echo "  FAIL: $esc.kry RAN (exit 0) -- the run must be refused at check time"
        esc_ok=0; fail=1
    elif grep -qE "TOPSECRET|EVIL-SUPPLY-CHAIN-LEAK" "$TMP/dnrun"; then
        echo "  FAIL: $esc.kry printed the secret"
        esc_ok=0; fail=1
    fi
    [ $esc_ok -eq 1 ] && echo "  ok   $esc.kry rejected by the row layer in default and strict mode, run refused"
done
# The twin of every program above hands the block only pure closures, or denies a
# capability none of the closures uses, and must stay accepted. The verdict is taken on
# the final row, so an unbound var that nothing ever binds (an empty registry, a
# parameter row no reference supplies) must still mean "no authority arrived".
R3_CONTROLS="ctl_deny_param_wrapper_pure \
    ctl_deny_param_callee_pure \
    ctl_deny_param_two_callers_one_pure \
    ctl_deny_param_other_capability \
    ctl_deny_param_chain_pure \
    ctl_deny_param_method_pure \
    ctl_deny_param_lambda_pure \
    ctl_deny_late_forward_helper_pure \
    ctl_deny_late_method_field_pure \
    ctl_deny_late_global_slot_pure \
    ctl_deny_late_loop_store_pure \
    ctl_deny_late_callee_after_main_pure \
    ctl_deny_late_lambda_before_store_pure \
    ctl_deny_late_slot_other_capability \
    ctl_deny_trait_impl_no_param \
    ctl_slot_param_fn_value_hof_param_pure \
    ctl_slot_param_fn_value_struct_field_pure \
    ctl_slot_param_fn_value_map_of_ops_pure \
    ctl_slot_lambda_wrapper_calls_put_pure \
    ctl_deny_plugin_platform_pure"
for ctl in $R3_CONTROLS; do
    ctl_ok=1
    for mode in "" "--strict-capabilities"; do
        if ! "$K" check $mode "tests/security/$ctl.kry" >"$TMP/dnc" 2>&1; then
            echo "  FAIL: the deferred deny! verdict over-rejects $ctl.kry under \`check $mode\`:"
            tail -3 "$TMP/dnc" | sed 's/^/    /'; ctl_ok=0; fail=1
        fi
    done
    [ $ctl_ok -eq 1 ] && echo "  ok   $ctl.kry accepted in default and strict mode (no over-rejection)"
done
# The two shipped registry-dispatch programs are the precision witnesses the
# fail-closed row work is measured against: both hold privileged closures in
# slots and both must keep compiling.
for prog in examples/showcase/repo_auditor.kry ecosystem/kryos-tool-broker; do
    prog_ok=1
    for mode in "" "--strict-capabilities"; do
        if ! "$K" check $mode "$prog" >"$TMP/slotp" 2>&1; then
            echo "  FAIL: slot widening rejects $prog under \`check $mode\`:"
            tail -3 "$TMP/slotp" | sed 's/^/    /'; prog_ok=0; fail=1
        fi
    done
    [ $prog_ok -eq 1 ] && echo "  ok   $prog still accepted in default and strict mode"
done

[ $fail -eq 0 ] && echo "security-gate: PASS" || echo "security-gate: FAIL"
exit $fail
