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
# look at the checker's internals. An order that puts an `impl` above its `trait` (or a const
# above what it reads) is a registration error (E0107, E0102) that says nothing about the
# `deny!` verdict; such an order is compared only when it also reports E0110.
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
tests/security/attack_resid_actor_handler_fwd.kry
tests/security/attack_resid_method_global_fwd.kry
tests/security/attack_resid_select_store_fwd.kry
tests/security/attack_resid_spawn_store_fwd.kry
tests/security/attack_resid_ring40_fwd.kry
tests/security/attack_resid_trait_default_show_before_store.kry
tests/security/attack_resid_trait_default_selfrec_swap.kry
tests/security/attack_resid_trait_after_main_default_store.kry
tests/security/attack_resid_selfrec_swap_global.kry
tests/security/attack_disp_self_returns_container.kry
tests/security/attack_disp_self_impl_builds_container.kry
tests/security/attack_disp_self_two_impls.kry
tests/security/attack_disp_self_put_registry.kry
tests/security/attack_disp_bound_put_registry.kry
tests/security/attack_disp_bound_return_chain.kry
tests/security/attack_disp_dyn_default_param.kry
tests/security/ctl_resid_actor_handler_fwd.kry
tests/security/ctl_resid_method_global_fwd.kry
tests/security/ctl_resid_select_store_fwd.kry
tests/security/ctl_resid_spawn_store_fwd.kry
tests/security/ctl_resid_ring40_fwd.kry
tests/security/ctl_resid_trait_default_show_before_store.kry
tests/security/ctl_resid_trait_default_selfrec_swap.kry
tests/security/ctl_resid_trait_after_main_default_store.kry
tests/security/ctl_resid_selfrec_swap_global.kry
tests/security/ctl_disp_self_returns_container.kry
tests/security/ctl_disp_self_impl_builds_container.kry
tests/security/ctl_disp_self_two_impls.kry
tests/security/ctl_disp_self_put_registry.kry
tests/security/ctl_disp_bound_put_registry.kry
tests/security/ctl_disp_bound_return_chain.kry
tests/security/ctl_disp_dyn_default_param.kry
tests/security/attack_slot_param_fn_value_factory.kry
tests/security/attack_tdb_factory_captured.kry
tests/security/attack_tdb_factory_two_level.kry
tests/security/attack_tdb_factory_hof.kry
tests/security/attack_tdb_named_factory_cond.kry
tests/security/attack_tdb_named_factory_tuple.kry
tests/security/attack_tdb_put_tuple_default.kry
tests/security/attack_tdb_put_tuple_free.kry
tests/security/attack_tdb_method_factory.kry
tests/security/attack_tdb_bound_factory.kry
tests/security/attack_tdb_factory_relay.kry
tests/security/attack_tdb_factory_param_arg.kry
tests/security/ctl_tdb_factory_captured.kry
tests/security/ctl_tdb_named_factory_cond.kry
tests/security/ctl_tdb_method_factory.kry
tests/security/ctl_tdb_bound_factory.kry
tests/security/ctl_tdb_factory_relay.kry
tests/security/attack_r2sp_struct_tuple_inline.kry
tests/security/attack_r2sp_struct_tuple_relay.kry
tests/security/attack_r2sp_struct_tuple_builder.kry
tests/security/attack_r2sp_struct_tuple_dyn.kry
tests/security/attack_r2sp_struct_tuple_enum.kry
tests/security/attack_r2sp_getter_free.kry
tests/security/attack_r2sp_getter_chain.kry
tests/security/attack_r2sp_getter_forward.kry
tests/security/attack_r2sp_getter_bound.kry
tests/security/attack_r2sp_getter_default.kry
tests/security/attack_r2sp_getter_dyn.kry
tests/security/ctl_r2sp_struct_tuple_relay.kry
tests/security/ctl_r2sp_getter_free.kry
tests/security/ctl_r2sp_getter_default.kry
tests/security/ctl_r2sp_getter_dyn.kry
tests/security/ctl_r2sp_getter_forward.kry
tests/security/attack_r2sp_lambda_nested.kry
tests/security/attack_r2sp_lambda_delegates.kry
tests/security/ctl_r2sp_lambda_delegates.kry
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
