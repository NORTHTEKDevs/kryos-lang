#!/usr/bin/env bash
# Ground truth: which OPEN capability-escape repros still escape?
# An ESCAPE = `kryos check` accepts (rc=0) a program that reaches gated
# authority. We report check rc under both enforcement modes plus whether
# running it prints a leak marker.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos.exe"

files="
41a|attack_realprogram_deny_blocks_builtin_exfil
41b|attack_realprogram_tool_registry
38|attack_r2_tuple_forloop_index_call
37|attack_deref_borrow_param_defeats_field_resolver
36|attack_deny_pipe_bare_ident_call
32a|attack_verify_tuple_call_general
32b|attack_verify_tuple_in_state
33|attack_verify_actor_to_actor_message
34|attack_verify_double_alias
35|attack_static_method_hotparam_offset
30a|attack_container_via_accessor_fn_call
30b|attack_container_via_accessor_method_call
30c|attack_ifexpr_receiver_field_call
30d|attack_matchexpr_receiver_field_call
10a|attack_wrap_closure_actor
10b|attack_wrap_closure_generic
10c|attack_wrap_closure_impl_method
10d|attack_wrap_closure_inference_bypass
10e|cap_escape_closure_wraps_closure
55|attack_select_timeout_body_zero_cap
55b|attack_select_timeout_transitive_zero_cap_main
55c|attack_select_timeout_inside_deny
55d|attack_select_timeout_rawmem_ffi_gate
56|attack_select_timeout_duration_zero_cap
57|attack_select_arm_shadow_closure_deny
58|attack_spawn_lambda_shadow_closure_deny
59|attack_for_binder_shadow_closure_deny
60|assault_round2_strip_reassigned_local_deny
61|attack_deny_bare_closure_reassign_escape
62|attack_reassign_local_defeats_hotparam
63|attack_round3_trycatch_deny_narrowed_catch
64|attack_deny_double_indirection_wrapper_local
65|attack_match_bound_var_field_invoke
66|attack_realprogram_registry_list_wrapper
67|attack_realprogram_registry_foreach
68|attack_index_field_invoke_factory_element_deny
69|attack_map_param_alias_invoke_deny
70|attack_array_param_alias_invoke_deny
71|attack_actor_explicit_self_swapped_params_deny
72|assault_r1_real_program_plugin_platform_nopool
73|attack_reassign_local_match_stmt_block
74|attack_reassign_local_match_in_deny
75|attack_reassign_local_match_enum_arm
76|attack_reassign_local_match_literal_subject
77a|attack_reassign_local_via_local_closure_return_lambda
77b|attack_reassign_local_via_local_closure_return_nested_fn
78|attack_reassign_local_block_in_for_iterable
79|attack_reassign_local_block_in_match_scrutinee
80|attack_reassign_local_block_in_method_receiver
81|attack_reassign_local_ifexpr_as_call_arg
82|attack_reassign_local_ifexpr_in_binop
83|attack_reassign_local_match_as_call_arg
84|attack_reassign_local_match_in_array_literal
85|attack_reassign_local_match_in_assign_rhs
86|attack_reassign_local_match_in_cast
87|attack_reassign_local_match_in_elif_cond
88|attack_reassign_local_match_in_if_cond
89|attack_reassign_local_match_in_index
90|attack_reassign_local_match_in_interpolation
91|attack_reassign_local_match_in_let_tuple
92|attack_reassign_local_match_in_map_literal
93|attack_reassign_local_match_in_match_guard
94|attack_reassign_local_match_in_method_arg
95|attack_reassign_local_match_in_pipe
96|attack_reassign_local_match_in_range
97|attack_reassign_local_match_in_ref
98|attack_reassign_local_match_in_struct_literal
99|attack_reassign_local_match_in_unary
100|attack_reassign_local_match_in_while_cond
101|attack_reassign_local_quantum_block
102|attack_reassign_local_unsafe_block
103|attack_reassign_local_unsafe_in_match_arm
104|attack_reassign_container_arr_match_arg
105|attack_reassign_container_arr_push_quantum
106|attack_reassign_container_arr_unsafe
107|attack_reassign_container_map_in_deny_body
108|attack_reassign_container_map_match_arg
109|attack_reassign_container_map_unsafe
110|attack_reassign_container_map_while_cond
111|attack_reassign_container_struct_field_ifexpr_arg
112|attack_slot_param_container_store
113|attack_slot_param_struct_field_store
114|attack_slot_register_after_main
115|attack_deny_in_callee_before_stores
116|attack_deny_in_method_before_stores
slot01|attack_slot_array_push_blinded
slot02|attack_slot_array_index_assign_blinded
slot03|attack_slot_struct_field_assign_blinded
slot04|attack_slot_enum_payload_second_store
slot05|attack_slot_nested_array_literal
slot06|attack_slot_tuple_array_literal
slot07|attack_slot_struct_array_field_literal
slot08|attack_slot_two_struct_instances
slot09|attack_slot_empty_array_two_pushes
slot10|attack_slot_array_from_call_push
slot11|attack_slot_option_cell_reassign
slot12|attack_slot_tuple_reassign
slot13|attack_slot_loop_carried_store_via_helper
slot14|attack_slot_register_helper
slot15|attack_slot_register_method
slot16|assault_r1_real_program_plugin_platform_stores_first
slot17|attack_slot_ret_push_param
slot18|attack_slot_ret_literal_param
slot19|attack_slot_ret_middleware_chain
slot20|attack_slot_ret_two_arrays
slot21|attack_slot_ret_captures_param
slot22|attack_slot_ret_struct_items
slot23|attack_slot_ret_option_cell
slot24|attack_slot_ret_enum_payload
slot25|attack_slot_ret_generic_container
slot26|attack_slot_ret_generic_fn
slot27|attack_slot_ret_static_method
slot28|attack_slot_ret_trait_method
slot29|attack_slot_ret_method_returns
slot30|attack_slot_ret_lambda_of_container
slot31|attack_slot_two_returns
slot32|attack_slot_if_arms
slot33|attack_slot_match_arms
slot34|attack_slot_lambda_param_single_call
slot35|attack_slot_lambda_param_two_calls
slot36|attack_slot_register_after_pure_store
slot37|attack_slot_call_after_narrow
slot38|attack_slot_ret_recursion
slot39|attack_slot_ret_result_cell
slot40|attack_slot_ret_fluent_builder
slot41|attack_slot_ret_rebuild_from_param_array
slot42|attack_slot_lambda_param_over_container
slot43|attack_slot_lambda_param_over_container_map
slot44|attack_slot_param_index_assign
slot45|attack_slot_param_push_inplace
slot46|attack_slot_param_map_store
slot47|attack_slot_param_helper_chain
slot48|attack_slot_param_store_pure_then_read
slot49|attack_slot_param_store_closed_privileged
slot50|attack_slot_param_list_push
slot51|attack_slot_param_mut_ref
slot52|attack_slot_param_method
slot53|attack_slot_param_fn_value
slot54|attack_slot_lambda_container_param_store
slot55|attack_slot_lambda_container_param_push
118a|attack_slot_param_fn_value_hof_param
118b|attack_slot_param_fn_value_struct_field
123a|attack_slot_param_fn_value_factory
118d|attack_slot_param_fn_value_map_of_ops
118e|attack_slot_lambda_wrapper_calls_put
123b|attack_slot_param_trait_bound_dispatch
123c|attack_slot_dyn_registrar_store
119a|attack_slot_param_container_in_tuple
119b|attack_slot_param_container_in_result
119c|attack_slot_param_array_of_option
119d|attack_slot_param_option_of_nested_array
119e|attack_slot_param_tuple_of_two_arrays
120a|attack_deny_loop_carried_store_after_block
120b|attack_deny_in_callee_after_main
120c|attack_deny_in_lambda_defined_before_store
120d|attack_deny_global_slot_reader_before_main
120e|attack_deny_show_after_add_before_main
121|attack_deny_coarse_family_row_blind
122|attack_deny_trait_default_method_unchecked
r3-01|attack_deny_param_alias_wrapper
r3-02|attack_deny_param_hof_wrapper
r3-03|attack_deny_param_actor_handler
r3-04|attack_deny_param_impl_method
r3-05|attack_deny_param_struct_field
r3-06|attack_deny_param_two_hops
r3-07|attack_deny_param_recursion
r3-08|attack_deny_param_method_chain
r3-09|attack_deny_param_fn_value_ref
r3-10|attack_deny_param_lambda_annotated
r3-11|attack_deny_param_hof_compose
r3-12|attack_deny_param_hof_compose_apply_after_main
r3-13|attack_deny_param_struct_field_op
r3-14|attack_deny_param_map_of_ops
r3-15|attack_deny_param_factory_op
r3-16|attack_deny_param_lambda_wrapper
r3-17|attack_deny_param_dyn_dispatch
r3-18|attack_deny_param_bound_dispatch
r3-19|attack_deny_param_in_tuple
r3-20|attack_deny_param_in_option
r3-21|attack_deny_param_in_struct
r3-22|attack_deny_param_in_nested_array
r3-23|attack_deny_param_pick_passthrough
r3-24|attack_deny_param_factory_guard
r3-25|attack_deny_late_if_branch_store
r3-26|attack_fwd_array_param_alias_invoke_deny
r3-27|attack_fwd_map_param_alias_invoke_deny
r3-28|attack_fwd_slot_call_after_narrow
r3-29|attack_fwd_slot_param_store_pure_then_read
r3-30|attack_fwd_slot_lambda_param_over_container
r3-31|attack_fwd_slot_lambda_param_over_container_map
124a|attack_fwd_param_chain_invoke_in_deny
124b|attack_fwd_param_chain_container_in_deny
124c|attack_fwd_param_chain_method_in_deny
124d|attack_fwd_param_chain_actor_in_deny
124e|attack_fwd_rev_hotparam_forward_via_local_alias
124f|attack_fwd_rev_slot_param_helper_chain
124g|attack_fwd_mainfirst_verify_actor_to_actor_message
125a|attack_fwd_store_relay_two_hop
125b|attack_fwd_store_relay_global_slot
125c|attack_fwd_store_relay_method
125d|attack_fwd_builder_relay_returns_container
125e|attack_fwd_fill_relay_struct_field_after_main
125f|attack_fwd_fill_relay_struct_field_before_main
126a|attack_deny_in_select_arm_callee
126b|attack_deny_in_select_arm_main
126c|attack_deny_in_select_timeout_arm
127a|attack_shape_named_closure_forwards_captured_via_helper
127b|attack_shape_named_closure_two_captured_params
128|attack_shape_named_chain_applies_returned_closure
129a|attack_shape_loop_carried_rebound_callee
129b|attack_shape_loop_carried_alias
129c|attack_shape_lambda_let_replaces_earlier_binding
130f|attack_shape_local_hidden_return_match_stmt
130g|attack_named_fn_hidden_return_direct
130h|attack_select_arm_return_plain_deny
130m|attack_accessor_hidden_return
131a|attack_shape_local_returns_struct_field_in_place
131b|attack_shape_local_struct_passthrough_in_place
131c|attack_shape_local_result_bound_then_field
131d|attack_shape_local_result_method_in_place
131e|attack_shape_local_result_nested_field_in_place
131f|attack_shape_local_result_via_two_closures
132a|attack_shape_container_param_shadows_array
132b|attack_shape_container_inner_let_shadows_array
132c|attack_shape_container_registry_holds_privileged
132d|attack_shape_container_lookup_over_privileged
132e|attack_shape_container_param_element_via_id
132f|attack_shape_container_grown_by_callee
132g|attack_shape_container_alias_grown
133a|attack_shape_name_collision_dead_if_block
133b|attack_shape_name_collision_live_if_block
133c|attack_shape_name_collision_for_body
133d|attack_shape_name_collision_reader_factory
133e|attack_shape_name_collision_nested_fn_dead_block
133f|attack_shape_name_collision_dead_closure_body
133g|attack_shape_name_collision_while_false
133h|attack_shape_name_collision_hidden_return_fn
133i|attack_shape_name_collision_generic_fn
133j|attack_shape_name_collision_bound_result
133k|attack_shape_name_collision_for_binder
133l|attack_shape_name_collision_match_binder
134a|attack_shape_local_returns_param
134b|attack_shape_nested_fn_returns_second_param
134c|attack_shape_local_returns_captured
134d|attack_shape_local_returns_captured_rebound
134e|attack_shape_local_returns_one_of_params
134f|attack_shape_local_two_levels
134g|attack_shape_local_two_levels_chain
134h|attack_shape_local_result_invoked_in_place
134i|attack_shape_local_result_let_init
134j|attack_shape_local_result_in_struct_field
134k|attack_shape_local_calls_other_local
134l|attack_shape_nested_fn_recursive
134m|attack_shape_local_block_alias
134n|attack_shape_local_result_as_hot_arg
134o|attack_shape_local_alias_of_local
134p|attack_shape_local_alias_of_named_fn
134q|attack_shape_local_two_bindings
134r|attack_shape_named_getter_through_local
134s|attack_shape_closure_in_closure_captured
"

printf "%-6s %-52s %-10s %-10s %s\n" ITEM FILE CHECK STRICT RUN
printf "%-6s %-52s %-10s %-10s %s\n" ---- ---- ----- ------ ---
esc=0; fixed=0; missing=0
for row in $files; do
  item="${row%%|*}"; name="${row##*|}"
  f="$ROOT/tests/security/$name.kry"
  if [ ! -f "$f" ]; then
    printf "%-6s %-52s %s\n" "$item" "$name" "MISSING"
    missing=$((missing+1)); continue
  fi
  ( cd "$ROOT" && timeout 60 "$K" check "$f" >/dev/null 2>&1 ); c1=$?
  ( cd "$ROOT" && timeout 60 "$K" check --strict-capabilities "$f" >/dev/null 2>&1 ); c2=$?
  out=$( cd "$ROOT" && timeout 60 "$K" run "$f" 2>&1 ); rr=$?
  leak="no"
  echo "$out" | grep -qiE "leak|TOPSECRET|ESCAPE" && leak="LEAK"
  verdict="fixed"
  if [ "$c1" -eq 0 ] && [ "$leak" = "LEAK" ]; then verdict="ESCAPES"; fi
  # A program the strict mode accepts but the default mode rejects (the default
  # mode's shape layer catches it, the strict mode relies on the row layer alone)
  # is an escape too: run it under --strict-capabilities and look for the leak.
  if [ "$c2" -eq 0 ] && [ "$verdict" != "ESCAPES" ]; then
    sout=$( cd "$ROOT" && timeout 60 "$K" run --strict-capabilities "$f" 2>&1 )
    if echo "$sout" | grep -qiE "leak|TOPSECRET|ESCAPE"; then leak="LEAK"; verdict="ESCAPES"; fi
  fi
  [ "$verdict" = "ESCAPES" ] && esc=$((esc+1)) || fixed=$((fixed+1))
  printf "%-6s %-52s rc=%-7s rc=%-7s %s %s\n" "$item" "$name" "$c1" "$c2" "$leak" "$verdict"
done
echo
echo "STILL ESCAPING: $esc    now-rejected: $fixed    missing: $missing"
