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
  [ "$verdict" = "ESCAPES" ] && esc=$((esc+1)) || fixed=$((fixed+1))
  printf "%-6s %-52s rc=%-7s rc=%-7s %s %s\n" "$item" "$name" "$c1" "$c2" "$leak" "$verdict"
done
echo
echo "STILL ESCAPING: $esc    now-rejected: $fixed    missing: $missing"
