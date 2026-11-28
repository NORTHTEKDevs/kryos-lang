#!/usr/bin/env bash
# assault_f6_receiver_pin.sh -- ASSAULT-F6 (LEDGER, "Close-out r2-misc, assault_f6"): a method call is charged
# the capabilities of the method of the RECEIVER'S OWN type, not the union over every method that shares its
# name. On master every method table was keyed by the bare method name, so a capability-free
# `PlainFormatter.run` was charged the fs:read of the unrelated `SecretLoader.run`, and
# `deny!(fs:read) { f.run() }` on a PlainFormatter was rejected (E0507).
#
# The receiver is resolved by type only when the function body PROVES it: a parameter with a declared struct or
# enum type, `Self` in an impl, a `let` with a type annotation or a struct literal initializer, and every
# binding of that name in the function agrees. Everything else keeps the old union: a `dyn`, a generic
# parameter, a call result, a field, a name bound twice with different types, a lambda or match binder, a name
# that is also a top-level let, a name a blanket impl or a trait default body also answers.
#
#   controls  accepted by `check` in both modes, and `run` prints the marker.
#   attacks   rejected by `check` in both modes with E0507, `run` refused, the secret never printed.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# Controls: capability-safe code that must be accepted in both modes and run.
for ctl in ctl_assault_f6_local_receiver:ASSAULT-F6-LOCAL-OK \
           ctl_assault_f6_param_receiver:ASSAULT-F6-PARAM-OK; do
    prog="${ctl%%:*}"; marker="${ctl##*:}"
    ctl_ok=1
    for mode in "" "--strict-capabilities"; do
        if ! "$K" check $mode "tests/security/$prog.kry" >"$TMP/c" 2>&1; then
            echo "  FAIL: $prog.kry rejected by \`check $mode\` (a call on a receiver of a proven type is charged the whole name):"
            tail -3 "$TMP/c" | sed 's/^/    /'; ctl_ok=0; fail=1
        fi
    done
    if ! "$K" run "tests/security/$prog.kry" 2>&1 | grep -q "$marker"; then
        echo "  FAIL: $prog.kry did not print $marker under \`run\`"; ctl_ok=0; fail=1
    fi
    [ $ctl_ok -eq 1 ] && echo "  ok   $prog.kry accepted in both modes and prints its marker"
done

# The finding's own program (default mode only: its SecretLoader.run is unannotated, which strict mode
# rejects by contract at the call of load_secret inside it, whatever any receiver is).
prog=assault_round2_probe_trait_method_name_conflation
if ! "$K" check "tests/security/$prog.kry" >"$TMP/c" 2>&1; then
    echo "  FAIL: $prog.kry rejected by \`check\`:"; tail -3 "$TMP/c" | sed 's/^/    /'; fail=1
elif ! "$K" run "tests/security/$prog.kry" 2>&1 | grep -q "UNRELATED-TRAIT CONTROL (should run fine): formatted-output"; then
    echo "  FAIL: $prog.kry did not print its control line under \`run\`"; fail=1
else
    echo "  ok   $prog.kry accepted by \`check\` and prints its control line"
fi

# Attacks: the receiver is, or may be, the privileged type; each must stay rejected in both modes.
for esc in attack_assault_f6_privileged_receiver attack_assault_f6_privileged_param \
           attack_assault_f6_rebound_receiver attack_assault_f6_branch_rebound \
           attack_assault_f6_global_shadow attack_assault_f6_blanket_impl \
           attack_assault_f6_trait_default_body attack_assault_f6_generic_bound \
           attack_assault_f6_lambda_param_shadow attack_assault_f6_match_binder; do
    esc_ok=1
    for mode in "" "--strict-capabilities"; do
        if "$K" check $mode "tests/security/$esc.kry" >"$TMP/a" 2>&1; then
            echo "  FAIL: $esc.kry passed \`check $mode\`"; esc_ok=0; fail=1
        elif ! grep -q "E0507" "$TMP/a"; then
            echo "  FAIL: $esc.kry rejected by \`check $mode\`, but not with E0507:"
            tail -3 "$TMP/a" | sed 's/^/    /'; esc_ok=0; fail=1
        fi
        if "$K" run $mode "tests/security/$esc.kry" >"$TMP/r" 2>&1; then
            echo "  FAIL: $esc.kry RAN (exit 0) under \`run $mode\` -- the run must be refused at check time"
            esc_ok=0; fail=1
        elif grep -q "TOPSECRET" "$TMP/r"; then
            echo "  FAIL: $esc.kry printed the secret under \`run $mode\`"; esc_ok=0; fail=1
        fi
    done
    [ $esc_ok -eq 1 ] && echo "  ok   $esc.kry rejected in both modes with E0507, run refused"
done

[ $fail -eq 0 ] && echo "assault-f6-receiver-pin: PASS" || echo "assault-f6-receiver-pin: FAIL"
exit $fail
