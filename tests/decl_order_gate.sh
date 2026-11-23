#!/usr/bin/env bash
# Declaration-order gate: a `deny!` verdict must not depend on where a function is declared.
#
# LEDGER items 124 and 125 (root F). A function declared ABOVE the callee it hands its
# parameter (or its stores) to read the callee's row before the callee's body was checked,
# so the caller's copy of the callee's parameter vars was never mapped and the verdict
# differed between a program and its reordering. `check_module` now checks the bodies once
# more in call-graph order (callee before caller) when the module holds a `deny!` whose row
# can still change; see `InferenceEngine::callee_first_order`.
#
# This gate re-orders the top-level items of the escape repros (and of the forward-chain
# pins and controls) in the original, the reversed and 24 seeded random orders and requires
# one verdict, default and strict mode, for every order. It is the observable: it does not
# look at the checker's internals.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
K="$(cd "$(dirname "$K")" && pwd)/$(basename "$K")"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
PY=python3; command -v python3 >/dev/null 2>&1 || PY=python

PROGRAMS="
tests/security/attack_fwd_param_chain_invoke_in_deny.kry
tests/security/attack_fwd_param_chain_container_in_deny.kry
tests/security/attack_fwd_param_chain_method_in_deny.kry
tests/security/attack_fwd_param_chain_actor_in_deny.kry
tests/security/attack_fwd_rev_hotparam_forward_via_local_alias.kry
tests/security/attack_fwd_rev_slot_param_helper_chain.kry
tests/security/attack_fwd_mainfirst_verify_actor_to_actor_message.kry
tests/security/attack_fwd_store_relay_two_hop.kry
tests/security/attack_fwd_store_relay_global_slot.kry
tests/security/attack_fwd_store_relay_method.kry
tests/security/attack_fwd_builder_relay_returns_container.kry
tests/security/attack_fwd_fill_relay_struct_field_after_main.kry
tests/security/attack_fwd_fill_relay_struct_field_before_main.kry
tests/security/attack_fwd_three_hop_param_chain.kry
tests/security/attack_fwd_mutual_recursion_param_chain.kry
tests/security/attack_fwd_three_hop_store_relay.kry
tests/security/attack_fwd_impl_method_chain.kry
tests/security/ctl_fwd_param_chain_pure.kry
tests/security/ctl_fwd_store_relay_pure.kry
"
fail=0
# shellcheck disable=SC2086
if ! "$PY" tests/decl_order_permute.py "$K" "$TMP" 24 $PROGRAMS; then
    fail=1
fi
if [ $fail -ne 0 ]; then
    echo "decl_order_gate: FAIL (a verdict changed with declaration order)"
    exit 1
fi
echo "decl_order_gate: PASS"
