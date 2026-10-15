# Escape routing — measured, not reasoned

Where each open capability escape's call **actually reaches** the checker, and
which line hands back the fail-open answer.

Acceptance for graph node `escape-instrument` (`tools/loop/check-routing-complete.sh`).
Every LIVE CAPABILITY ESCAPE in the LEDGER's OPEN section needs a row here, and a
row containing `TODO` / `unknown` / `??` does not count.

## Why this file blocks the fix

Two fixes on 2026-08-10 were written from reading the code and both failed:

| Attempt | Predicted | Measured |
| --- | --- | --- |
| `Borrow`/`Deref` passthrough in `decompose_container_path` | closes item 37 | **still escaping** — that shape never reaches the decomposer |
| `TupleLiteral` in `literal_field_exists` | closes items 32, 38 | **still escaping** — blocked earlier than the literal resolver |

Both were plausible. Both cost a build-measure-revert cycle. The lesson is not
"try harder", it is **measure where the call goes before editing**.

## Known fail-open sites (read from source 2026-08-10, not yet per-item attributed)

- `kryos-capabilities/src/checker.rs` `decompose_container_path` (~971) — understands
  only `Identifier` / `FieldAccess` / `IndexAccess`; everything else returns `None`,
  and every caller reads `None` as "requires no authority".
- `check_callee_capabilities` (~5271) — *has* a fail-closed direct-invoke path, but it
  is gated on `segments.len() <= 1`. `resolve_path` returns `["pair","1"]` for a field
  chain, so the fail-closed path is **skipped for every field/index chain**. The
  guard's comment asserts a multi-segment path is "always a qualified stdlib call";
  that assumption is false.
- `resolve_method_field_invoke_caps` (~3647) — returns `CapabilitySet::empty()`
  (ungated) when the object does not decompose, and again when `literal_field_exists`
  says no.

## How to measure a row

Add a temporary `eprintln!` at each candidate site printing the callee shape and the
returned capability set, rebuild (`cd compiler && cargo build --release`), run the
item's repro from `tests/security/`, and record which site answered. Remove the
instrumentation before committing. This is the same technique that localized the
bootstrap regression in one step (a call counter showed `infer_expr` flat and linear
while the resolver grew to 15.3M calls).

## Routing table

Measured 2026-08-11 with `KRYOS_CAP_TRACE=1` (temporary env-gated probes in
`check_expr`, `check_callee_capabilities`, and `resolve_method_field_invoke_caps`;
instrumentation removed after the run).

| item | repro | reaches | fail-open line | notes |
| --- | --- | --- | --- | --- |
| 32 | `attack_verify_tuple_call_general` | `check_callee_capabilities`, `callee=FieldAccess segments=["pair","1"]` | the `segments.len() <= 1` guard on the fail-closed direct-invoke block | `named=false seglen=2` so `failclosed_entered=false`. The guard's comment claims a multi-segment path "is always a qualified stdlib call" — false for any field chain. |
| 38 | `attack_r2_tuple_forloop_index_call` | `check_callee_capabilities`, `callee=FieldAccess segments=["x","0"]` | same `segments.len() <= 1` guard | Identical mechanism to item 32; the `for`-binding is irrelevant. |
| 30 | `attack_container_via_accessor_fn_call` / `_method_call` / `attack_ifexpr_receiver_field_call` / `attack_matchexpr_receiver_field_call` | `resolve_method_field_invoke_caps` | `decompose_container_path` returns `None` | Measured receivers: `object=FnCall`, `object=MethodCall`, `object=IfExpr`, `object=MatchExpr`. The `_method_call` shape ALSO hits `literal_field_exists(false) lit=StructLiteral` on a second path. |
| 33 | `attack_verify_actor_to_actor_message` | `resolve_method_field_invoke_caps`, `root=target method=receive` | non-literal fallback with `has_lit=false` returns empty | Root is an actor/param, not a tracked container literal, so the literal branch is skipped and the type-based fallback yields nothing. |
| 34 | `attack_verify_double_alias` | `resolve_method_field_invoke_caps`, `root=y method=f` | non-literal fallback with `has_lit=false` returns empty | Two `let` hops from the actor-state field; the alias chain is not tracked, so `y` has no literal. |
| 37 | `attack_deref_borrow_param_defeats_field_resolver` | `check_expr` MethodCall arm, `method=f object=Deref`; `resolve_method_field_invoke_caps root=b method=f` | non-literal fallback with `has_lit=false` returns empty | **This is why the `Borrow`/`Deref` passthrough failed to fix it**: decomposition is not the blocker, the missing non-literal provenance is. |
| 35 | `attack_static_method_hotparam_offset` | `check_expr` **StaticMethodCall** arm, `method=run` | never reaches `resolve_method_field_invoke_caps` at all | The StaticMethodCall arm has no fn-value/hot-param resolution for a fn-typed argument at index 0. Distinct site from every other item. |

## Conclusion — four sites, one conflation

1. **`segments.len() <= 1`** disables the existing fail-closed path for every field/index chain → items 32, 38.
2. **`decompose_container_path` → `None`** for non-path receivers (`FnCall`, `MethodCall`, `IfExpr`, `MatchExpr`) → item 30 family.
3. **`has_lit=false` non-literal fallback returns `CapabilitySet::empty()`** for params, alias chains and actor state → items 33, 34, 37.
4. **The `StaticMethodCall` arm** performs no fn-value resolution → item 35.

All four are the same mistake: *"I could not resolve this"* returns the same value as
*"this needs no authority."* The fix is to make unresolvable provenance return an
explicit fail-closed answer (require `all`) at each of these four sites, not to teach
the decomposer more shapes.

**Expected over-rejection when flipping**, based on the pipe fix: shapes that are
*resolvable but different* must not be swept in. `ir_signatures` is the canary
(it caught `5 |> padd(10)` demanding `all`), and `strict_caps` + `examples` cover
the 91-example surface.

## Site 3 — attempt log (item 37), so the next attempt does not repeat it

Site 3's Case 2 in `resolve_method_field_invoke_caps` ALREADY fails closed
correctly: if `local_container_types.get(root)` yields a type and
`resolve_type_path(ty, path + method)` is a `TypeExpr::Function`, it returns `all`.
The gap is that the root has no type entry. Two halves were tried:

| attempt | rationale | result |
| --- | --- | --- |
| `Borrow`/`Deref` passthrough in `decompose_container_path` (shipped in `183087c`) | `(*b).f()` returned `None` and early-returned before Case 2 | necessary, **not sufficient** |
| seed function PARAMS into `current_local_container_types` | item 37's root `b` is a param, so Case 2 had no type to consult | **still escapes with both applied** |

So decomposition reaching Case 2 and a param type entry are BOTH present and item 37
is still ungated. The remaining blocker is inside Case 2 itself — either
`resolve_type_path` does not walk the param's declared type to the fn-typed field,
or the param's `ty` is not the shape Case 2 expects. **Measure that specific call
before editing again**: probe `resolve_type_path`'s input and return in Case 2 for
`root=b method=f`. Do not attempt a third blind fix on this shape; three have now
failed (Borrow/Deref alone, TupleLiteral, param seeding).

The param-seeding change was REVERTED rather than shipped: it is plausibly correct
but changes capability behaviour, and an unproven behavioural change to the security
checker is not worth carrying on the branch.

## Item 37 SOLVED, and what it costs (measured 2026-08-11)

The probe recommended above was run and answered it immediately. `resolve_type_path`
received `Reference { inner: Simple("Box") }` -- the param is declared `b: &Box` --
and returned `None`, because it has a transparent-unwrap arm for `Optional` but
**none for `Reference`**. Case 2 therefore never saw the fn-typed field.

Two changes close item 37 (verified: repro goes rc=0 -> rc=1):

1. a `Reference` unwrap arm in `resolve_type_path_inner`, next to the existing
   `Optional` one -- `&T` has exactly `T`'s fields; this is the type-side mirror of
   the `Borrow`/`Deref` passthrough already in `decompose_container_path`;
2. seeding function PARAMS into `current_local_container_types`, excluding directly
   fn-typed params (a `handler: fn(Request) -> Response` is the fn-value itself, not
   a container; its authority is already tracked by `own_params` /
   `DependsOnParam`, and seeding it made every call to such a param demand `all`).

**BUT the change is NOT shipped, because it surfaced a real design cost, not a bug.**
With `Reference` unwrapping correctly, `conf_stdlib_wave14` fails: the stdlib's own
HTTP router does `resp = route.handler(req)` (`compiler/stdlib/http.kry:682`) --
invoking a closure stored in a struct field, read out of an array. Under fail-closed
semantics that provenance genuinely IS untraceable, so requiring `all` is the
CORRECT answer, not a false positive.

So the fail-closed flip has a price, and this is it: **a dispatcher that invokes
user-supplied handlers must either declare `@capabilities(all)` or the language needs
a way to express "this call's authority flows from the handler argument".** That is a
language-design decision (and the honest reading is that a router really does carry
whatever authority its handlers carry), not something to settle by patching the
checker. It applies to every remaining site, so decide it BEFORE flipping sites 2-4.

The change was reverted to keep the branch at a proven-green state. Re-apply with:
`resolve_type_path_inner` + `Reference` arm, and param seeding with the
`TypeExpr::Function` exclusion.

## THE DESIGN QUESTION IS NOW SETTLED BY MEASUREMENT (2026-08-12)

The fail-closed flip forces a choice, and both branches were tried on real code
rather than argued about:

**Option A -- annotate dispatchers `@capabilities(all)`. MEASURABLY WRONG.**
Applied to `std::http::http_serve` (`route.handler(req)`) and
`std::agent::use_tool` (`self.tools[i].handler(input)`). Both are the same shape:
a closure registered by the caller, stored in a struct field, read back out of an
array, invoked. Annotating them did NOT fix `conf_stdlib_wave14` -- it moved the
error outward:

    error[E0507]: call to `use_tool` requires capabilities [all] not granted to caller

**`all` cascades to every consumer.** Any program that uses `std::http` or
`std::agent` at all would have to declare `@capabilities(all)`, which nullifies the
capability system for precisely the programs it exists to protect (an agent host
running third-party tools is the flagship use case). A capability system whose own
stdlib forces `all` is worse than no claim at all.

**Option B -- authority flows from the handler ARGUMENT. This is the answer.**
`use_tool` does not need `all`; it needs "whatever the tool I was handed carries".
That is row-polymorphic effect typing, and **stage 1 already exists in this repo**:
`891c406` built the `CapRow` representation and inference (`kryos-types/src/ty.rs`,
`infer.rs`), including `generic_cap_var_ids` / `own_cap_var` on every `FunctionSig`.
What is missing is stage 2: ENFORCEMENT reading those rows at the call site instead
of the shape-directed resolution in `kryos-capabilities`.

So the remaining escapes (sites 2, 3, 4) should NOT be closed by extending the
shape-matcher or by annotating stdlib. They should be closed by finishing the
capability-row work that is already half-built. That also retires the whole
`decompose_container_path` / `literal_field_exists` / `hot_params` apparatus, which
is where every one of these escapes has come from.

Item 37's mechanical fix is real and re-appliable (Reference unwrap in
`resolve_type_path_inner` + param seeding excluding `TypeExpr::Function`), but it
should land WITH stage 2, not before it -- on its own it just triggers the cascade
above.


## 2026-10-15: items 55-72, measured

Eighteen escapes routed. The table keeps the same columns as the one above
(item, repro, where the call reaches, the line that answers fail-open, notes),
and the shape layer (`kryos-capabilities`) and the row layer (`kryos-types`)
are both named where both fail: `kryos check` accepts only when both miss, so
closing either one rejects the program.

Seven of these are closed on this branch (the `select` walkers, and a fn-typed
local's authority becoming the union of its bindings). The rest need the row
work described in the LEDGER's wave entry, which was prototyped and measured
at 0 verdict changes over a 1015-program corpus but is not landed here.

| item | repro | reaches | fail-open line | notes |
|---|---|---|---|---|
| 55 | tests/security/attack_select_timeout_body_zero_cap.kry (new, committed) | shape: check_stmt Stmt::Select arm kryos-capabilities/src/checker.rs:5162-5167 (inference twin collect_caps_stmt 4318, call walkers 1231 and 1438) never reaches check_block or check_expr for SelectTimeout.body or .duration_ms; check_raw_memory_direct (pipeline.rs:406) reuses check_stmt; rows: kryos-types check.rs:3481 Stmt::Select arm is empty | checker.rs:5162 (and 1231, 1438, 4318) destructures `{ branches, .. }` and the `..` swallows the whole timeout child (kryos-ast stmt.rs:19-23); because inference loses it too, an unannotated helper whose only gated call sits in a timeout body is inferred pure | measured: scratch prototype KRYOS_X_SP=select (the four walker edits) flips body, transitive-via-helper, raw-memory (all three modes) and deny! variants to rc=1; flip control: the identical file_write in an ordinary v ch arm is E0505; executed on master with check rc=0 (inferred, strict, permissive): marker file written, byte0=104 printed |
| 56 | tests/security/attack_select_timeout_duration_zero_cap.kry (new, committed) | shape: same four Select arms as item 55 (checker.rs:5162-5167, 4318, 1231, 1438): timeout.duration_ms is an expression child that no walker visits; rows: check.rs:3481 empty arm | checker.rs:5162-5167 never calls check_expr on the duration expression | measured: flip control: the same file_write expression bound by a let before the select is E0505 (rc=1); KRYOS_X_SP=select prototype flips the probe to rc=1; executed on master with check rc=0: marker file written by the duration expression |
| 57 | tests/security/attack_select_arm_shadow_closure_deny.kry (new, committed) | shape: check_callee_capabilities checker.rs:5503-5515 -> resolve_direct_invoke_caps 3657 (path empty, 3690-3691) -> resolve_closure_caps Identifier arm 1927-1930 for f() inside the select arm; rows: check.rs:3481 skips the arm body so nothing is charged | checker.rs:1928-1930 returns the stale Known(empty) of the OUTER `let f = harmless lambda` (insert at 2417); build_local_closure_caps_block has no Select arm (catch-all 2467) so the arm's `let f = reader` is never registered; rows: Select is a no-op | measured: KRYOS_CAP_TRACE capcheck_x.exe: Identifier f -> local_caps HIT Known({}) twice and require_direct_invoke_caps required is EMPTY; flip: fresh binder name g -> E0507 [all] (triage c3); prototypes: KRYOS_X_FIX_CAPS (walk Select) rc=1 alone, KRYOS_X_RP=select rows-only rc=1 alone; executed on master: secret printed |
| 58 | tests/security/attack_spawn_lambda_shadow_closure_deny.kry (new, committed) | shape: Identifier arm 1927-1930 for f() inside spawn fn(){...}; rows: Stmt::Spawn check.rs:3478-3480 -> infer_expr Lambda arm check.rs:6317-6319 pops a fresh cap_accum_stack frame and never charges it to the enclosing deny! frame | shape: checker.rs:1928-1930 stale Known(empty) (builder has no Spawn arm, catch-all 2467); rows: check.rs:6317-6319 lambda frame dropped at Stmt::Spawn | measured: KRYOS_CAP_TRACE: Identifier f -> local_caps HIT Known({}) inside the spawn; flips: the bare `spawn { }` block form is rejected E0110 (triage p3b), fresh name -> E0507 [all] (c7); prototypes: shape KRYOS_X_SP=spawn rc=1 alone, rows KRYOS_X_RP=spawn rc=1 alone; executed on master: secret printed |
| 59 | tests/security/attack_for_binder_shadow_closure_deny.kry (new, committed) | shape: Identifier arm 1927-1930 for f() where f is the for-loop binder; rows: array literal typed by its first element (check.rs:6050-6080) and closed/closed unify no-op infer.rs:867-874 | shape: checker.rs:1928-1930 stale Known(empty) from the outer `let f = harmless lambda` (the For arm 2433-2435 neither registers nor poisons the pattern binder); rows: infer.rs:867-874 drops the privileged element's row when a pure lambda is listed first, DenyBlock row is empty | measured: KRYOS_CAP_TRACE capcheck_x.exe: Identifier f -> local_caps HIT Known({}), unify Function arm (true,true) c1={} c2={fs:read} -> NO-OP, DenyBlock accumulated body row resolved = {}; flip: binder renamed g -> E0507 (shadow_for_ctl); prototypes: KRYOS_X_RP=slots rows-only rc=1, KRYOS_X_SP=poison rc=1; executed on master: prints pure then the secret |
| 60 | tests/security/assault_round2_strip_reassigned_local_deny.kry | shape: check_callee_capabilities checker.rs:5503-5515 -> resolve_direct_invoke_caps 3690-3691 -> resolve_closure_caps Identifier arm 1927-1930; rows: Stmt::Assign check.rs:3185-3212 | shape: checker.rs:1928-1930 returns the stale Known(empty) written at 2417 for the placeholder lambda; `slot = reader` is a Stmt::Assign swallowed by catch-all 2467; rows: infer.rs:867-874 closed/closed no-op at the assign unify (check.rs:3210) | measured: triage sec1 instrumented trace plus my re-run of its config matrix: KRYOS_X_FIX_CAPS alone rc=1, KRYOS_X_FIX_ROW alone rc=1, my KRYOS_X_RP=assign rows-only rc=1; flips: `let slot = reader` -> E0110 plus E0507, initializer replaced by an if-expression -> E0507 [all] |
| 61 | tests/security/attack_deny_bare_closure_reassign_escape.kry | shape: deny! sits INSIDE fn outer (scope depth 2 vs entry depth 1); check_callee_capabilities 5503-5515 -> resolve_direct_invoke_caps -> Identifier arm 1927-1930; rows: Stmt::Assign check.rs:3210 and DenyBlock check.rs:3507-3513 | shape: checker.rs:1928-1930 stale Known(empty) (2417 insert, Assign and DenyBlock swallowed by 2467); rows: two holes must both close: infer.rs:862-866 pins the param's open row var to the closed row of the harmless initializer AND check.rs:3513 concrete_bits() ignores the still-open var, so a rows-only closure needs R2 plus R3 together | measured: triage sec_closure_reassign instrumented trace; my capcheck_x matrix: KRYOS_X_FIX_CAPS alone rc=1, KRYOS_X_FIX_ROW alone rc=0; KRYOS_X_RP=assign alone 0, openvar alone 0, assign plus openvar rc=1; flip: initializer replaced by the param itself (v1) -> E0507 [all] |
| 62 | tests/security/attack_reassign_local_defeats_hotparam.kry | shape: callee side `state()` inside apply -> Identifier arm 1927-1930; caller side check_callee_capabilities 5467 -> accumulate_hot_extra_caps 3294-3296 returns early because hot_params has no entry for apply; rows: Stmt::Assign check.rs:3210 | shape: checker.rs:1928-1930 stale Known(empty) (2417/2467) AND compute_hot_params Seed A 1518-1521 registers a param only when the call target is textually the param name, so cb is never hot; rows: infer.rs:862-866 (true,false) arm pins the param's open row var to the closed row of state | measured: each flip alone: a dead `cb()` call (v1) -> E0507 [fs:read], `let state2 = ...` (v2b) -> E0507 [all], initializer cb (v3) -> E0110; my capcheck_x matrix: KRYOS_X_FIX_CAPS rc=1, KRYOS_X_FIX_ROW rc=1; KRYOS_X_RP=assign rows-only rc=1 |
| 63 | tests/security/attack_round3_trycatch_deny_narrowed_catch.kry | shape: check_stmt TryCatch arm checker.rs:5146-5158 -> catch block -> Stmt::DenyBlock 5168 -> check_callee_capabilities 5503-5515 -> Identifier arm 1927-1930; rows: Stmt::Assign check.rs:3210 | shape: checker.rs:1928-1930 stale Known(empty) (try/catch is incidental); rows: infer.rs:867-874 closed/closed no-op | measured: KRYOS_CAP_TRACE triage tcn-deny-narrowed-catch; flips: straight-line without try/catch still accepted, `let slot = f()` -> E0507 [fs:read], no-initializer or param-initializer forms -> E0110; my capcheck_x matrix: FIX_CAPS rc=1, FIX_ROW rc=1; KRYOS_X_RP=assign rows-only rc=1 |
| 64 | tests/security/attack_deny_double_indirection_wrapper_local.kry | shape: go() inside deny! in outer -> resolve_direct_invoke_caps 3690-3691 -> Identifier arm 1927-1930 returns Known(empty) for go; the entry was FROZEN at pre-pass by the Lambda arm fallthrough checker.rs:1918-1925 -> collect_caps_expr 4429-4443 -> deferred_own_param_caps 3567-3589 (scope not yet narrowed so line 3587 returns empty); rows: DenyBlock check.rs:3507-3513 | shape: checker.rs:3587 CapabilitySet::empty() lowered a DependsOnParam at pre-pass and 1918-1925 froze it; rows: check.rs:3513 concrete_bits() drops the open row var of param reader | measured: triage dbl-indirection-wrapper-local trace (deferred_own_param_caps scope_len=0 entry_depth=0 -> {}); prototypes alone: P1 (Lambda arm carries the deferral) -> E0507, P3 (open var at the deny boundary is all) -> E0110, my KRYOS_X_RP=openvar rows-only rc=1; flip: add a direct reader() call to go's body (captured-hot-param branch) -> E0507 |
| 65 | tests/security/attack_match_bound_var_field_invoke.kry | shape: check_expr MethodCall arm checker.rs:5195-5245 -> resolve_method_field_invoke_caps 3771 (decompose succeeds, no tracked literal 3784, no type entry 3820); rows: DenyBlock check.rs:3507-3513 | shape: checker.rs:3829 final CapabilitySet::empty(): root b is pattern-bound and absent from the let-only maps built at 2319, 2752, 2869; rows: check.rs:3513 drops the open var (Box.f row var is declaration-global and the factory freshens its copy) | measured: kh_inst3.exe KRYOS_CAP_TRACE (re-run): rmfic root=b path=[] method=f has_lit=false has_ty=false in_deny_scope=true ret=FALLTHROUGH (checker.rs:3829) -> empty; flip: decoy typed `let b: Box = mk(0)` -> Case 2 (3823-3826) all -> E0507; my KRYOS_X_RP=openvar rows-only rc=1 (E0110); triage P3 prototype rc=1 |
| 66 | tests/security/attack_realprogram_registry_list_wrapper.kry | shape: MethodCall 5195 -> resolve_method_field_invoke_caps 3771 for p.run() inside deny!; rows: Plugin.run slot var, StructLiteral store check.rs:6174 | shape: checker.rs:3829 (the `let p = reg.get(1)` sits inside the deny body: the three builders have no DenyBlock arm 2467/2840/2935 and a MethodCall initializer is never typed 2876-2903); rows: infer.rs:867-874 first store Plugin{run: safe} binds the per-declaration slot var to {} and the evil second store is dropped | measured: kh_inst3.exe trace (re-run): rmfic root=p ... in_deny_scope=true ret=FALLTHROUGH (checker.rs:3829); unify trace: Function arm (true,true) c1={} c2={fs:read} -> NO-OP; flips: hoist plus annotate `let p: Plugin` -> E0507 [all], evil-first order -> E0110; my KRYOS_X_RP=slots rows-only rc=1 |
| 67 | tests/security/attack_realprogram_registry_foreach.kry | shape: p.run() MethodCall whose receiver is the lambda's own param, reached through the Lambda arm checker.rs:1816-1925 (hot_own_param at 1831 sees only a direct call p()) -> resolve_method_field_invoke_caps 3771; rows: same Plugin.run slot | shape: checker.rs:3829 (lambda params get no type entry; the lambda falls through to Known(collect_caps_expr(..)) at 1918-1925); rows: infer.rs:867-874 first-store-wins on Plugin.run | measured: kh_inst3.exe trace (re-run): rmfic root=p path=[] method=run has_lit=false has_ty=false ret=FALLTHROUGH (checker.rs:3829); flips: evil plugin first or alone -> E0110, receiver replaced by an outer tracked literal -> E0507 [fs:read]; my KRYOS_X_RP=slots rows-only rc=1 |
| 68 | tests/security/attack_index_field_invoke_factory_element_deny.kry (new, committed) | shape: MethodCall 5195 -> resolve_method_field_invoke_caps 3771 for tools[0].run(..) inside deny!; rows: DenyBlock check.rs:3507-3513 | shape: checker.rs:3786 early return (literal_field_exists 3917-3939 is false because the tracked array literal's element at the Index step is a FnCall make_tool(reader), not a StructLiteral); rows: check.rs:3513 drops the open row var | measured: kh_inst3.exe trace (re-run): rmfic root=tools path=[Index] method=run ret=LIT-BRANCH-FIELD-MISSING (checker.rs:3786) -> empty; flip: element written as an inline struct literal -> rejected (triage p7); my KRYOS_X_RP=openvar rows-only rc=1; independent of KRYOS_NO_HOTPARAM_ALL (reproduces with it unset); executed on master: secret printed |
| 69 | tests/security/assault4_actor_handler_map_param_probe_v2.kry (as written: main holds fs:read so it is delegation once the sender is charged; escaping form is the deny! variant tests/security/attack_map_param_alias_invoke_deny.kry, the triage scratch) | shape: callee body f() -> resolve_direct_invoke_caps 3709 -> deferred_own_param_caps 3567-3589 (empty at 3587, no deny in the callee); send site check_callee_capabilities 5467 -> accumulate_hot_extra_caps 3294-3296 finds no hot_params entry for run_it; rows: map literal typed by its first element check.rs:6084-6109 | shape: checker.rs:3587 deferral with no taker (compute_hot_params Seed B 1531-1552 registers only an immediate param[path]() chain; the alias `let f = reg[name]` is invisible; producer 2399-2417 and 2237); rows: infer.rs:867-874 plus first-element map typing | measured on master: direct spelling reg[name]() -> E0507 [all], alias plus one dead direct call -> E0507, swapped map entries -> E0110, hot_params trace shows the index counts explicit self; prototypes: KRYOS_X_SP=taker rc=1 but newly rejects ecosystem/kryos-tool-broker (2 files); with taker plus actorself the as-written file is ACCEPTED (sender charged [fs:read]); KRYOS_X_RP=slots rows-only rc=1 on the deny form |
| 70 | tests/security/assault4_actor_handler_map_param_strict_probe.kry (as written: delegation once the sender is charged; escaping form tests/security/attack_array_param_alias_invoke_deny.kry, the triage scratch) | same two layers as item 69: callee alias read deferred (checker.rs:3587 via 3709) with no hot_params taker (3294-3296, Seed B 1531-1552); rows: array literal typed by its first element check.rs:6050-6080 | shape: checker.rs:3587 deferral with no taker; rows: infer.rs:867-874 (the privileged lambda listed second is dropped) | measured: triage a4_actor_map_param_strict flips: privileged element first -> E0110 only, direct spelling reg[1]() -> E0507 only, alias plus unreachable direct call -> E0507 [all]; my re-run: array form accepted in all modes, KRYOS_X_SP=taker rc=1, KRYOS_X_RP=slots rows-only rc=1; the as-written file stays accepted under KRYOS_X_RP=all plus KRYOS_X_SP=select (delegation) |
| 71 | tests/security/attack_actor_explicit_self_swapped_params_deny.kry (new, committed) | shape: MethodCall 5195-5245 d.run_it(name, reg) inside deny! -> compute_hot_extra_caps 5218 -> accumulate_hot_extra_caps 3294-3312; rows: map literal typed by its first element (build_registry returns pure first) check.rs:6084-6109 | shape: checker.rs:3299-3312 has_self_offset is false for actor handlers but parse_param_list (kryos-parser parser.rs:767-780) keeps an explicit self as params[0], so hot index 2 is looked up as argument index 2 of 2 and args.get(arg_idx) else continue charges nothing (same defect in the propagation loop 1573-1575); rows: first-element typing | measured: kh_inst3.exe KRYOS_CAP_TRACE (re-run): hot_params fn=run_it param_idx=2 paths={[Index]} and extra_into_enforce=[]; flip on master: deleting only `self` from the handler -> E0507 (a_direct_swapped_deny_noself); prototype KRYOS_X_SP=actorself closes it but drops matrix PRECISION to 18/75, so closure goes through rows: KRYOS_X_RP=slots rows-only rc=1 |
| 72 | tests/security/assault_r1_real_program_plugin_platform_nopool.kry (new, committed) call in main removed; the triage scratch) | shape: PluginHost.run_all and run_named deny! blocks (file lines 134-150): p.run(arg) -> resolve_method_field_invoke_caps 3771; rows: DenyBlock check.rs:3507-3513 and the Plugin.run slot | shape: checker.rs:3829 FALLTHROUGH root=p in_deny_scope=true (4 sites); rows: infer.rs:867-874 NO-OP (honest plugin stored first, compromised second) AND the DenyBlock row is {?C26}, an open var dropped at check.rs:3513 | measured: master check rc=0 and run prints EVIL-SUPPLY-CHAIN-LEAK and TOPSECRET from inside deny! on all four routes A-D; the shipped file is rejected only by an UNRELATED E0507 [all] at run_worker_pool (spawn precision over-rejection) which masks all four routes; rows-only closure needs slots plus the deferral and raw-charge mechanism (KRYOS_X_RP=slots,openvar or slots,assign rc=1 at lines 134 and 142); the shape-only spawn arm flips the shipped file to ACCEPTED, so land rows before any spawn precision fix |
