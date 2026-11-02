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
| 59 | tests/security/attack_for_binder_shadow_closure_deny.kry (new, committed) | shape: Identifier arm 1927-1930 for f() where f is the for-loop binder; rows: array literal typed by its first element (check.rs:6050-6080) and closed/closed unify no-op infer.rs:867-874 | shape: checker.rs:1928-1930 stale Known(empty) from the outer `let f = harmless lambda` (the For arm 2433-2435 neither registers nor poisons the pattern binder); rows: infer.rs:867-874 drops the privileged element's row when a pure lambda is listed first, DenyBlock row is empty | measured: KRYOS_CAP_TRACE capcheck_x.exe: Identifier f -> local_caps HIT Known({}), unify Function arm (true,true) c1={} c2={fs:read} -> NO-OP, DenyBlock accumulated body row resolved = {}; flip: binder renamed g -> E0507 (shadow_for_ctl); prototypes: KRYOS_X_RP=slots rows-only rc=1, KRYOS_X_SP=poison rc=1; executed on master: prints pure then the secret **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 60 | tests/security/assault_round2_strip_reassigned_local_deny.kry | shape: check_callee_capabilities checker.rs:5503-5515 -> resolve_direct_invoke_caps 3690-3691 -> resolve_closure_caps Identifier arm 1927-1930; rows: Stmt::Assign check.rs:3185-3212 | shape: checker.rs:1928-1930 returns the stale Known(empty) written at 2417 for the placeholder lambda; `slot = reader` is a Stmt::Assign swallowed by catch-all 2467; rows: infer.rs:867-874 closed/closed no-op at the assign unify (check.rs:3210) | measured: triage sec1 instrumented trace plus my re-run of its config matrix: KRYOS_X_FIX_CAPS alone rc=1, KRYOS_X_FIX_ROW alone rc=1, my KRYOS_X_RP=assign rows-only rc=1; flips: `let slot = reader` -> E0110 plus E0507, initializer replaced by an if-expression -> E0507 [all] |
| 61 | tests/security/attack_deny_bare_closure_reassign_escape.kry | shape: deny! sits INSIDE fn outer (scope depth 2 vs entry depth 1); check_callee_capabilities 5503-5515 -> resolve_direct_invoke_caps -> Identifier arm 1927-1930; rows: Stmt::Assign check.rs:3210 and DenyBlock check.rs:3507-3513 | shape: checker.rs:1928-1930 stale Known(empty) (2417 insert, Assign and DenyBlock swallowed by 2467); rows: two holes must both close: infer.rs:862-866 pins the param's open row var to the closed row of the harmless initializer AND check.rs:3513 concrete_bits() ignores the still-open var, so a rows-only closure needs R2 plus R3 together | measured: triage sec_closure_reassign instrumented trace; my capcheck_x matrix: KRYOS_X_FIX_CAPS alone rc=1, KRYOS_X_FIX_ROW alone rc=0; KRYOS_X_RP=assign alone 0, openvar alone 0, assign plus openvar rc=1; flip: initializer replaced by the param itself (v1) -> E0507 [all] |
| 62 | tests/security/attack_reassign_local_defeats_hotparam.kry | shape: callee side `state()` inside apply -> Identifier arm 1927-1930; caller side check_callee_capabilities 5467 -> accumulate_hot_extra_caps 3294-3296 returns early because hot_params has no entry for apply; rows: Stmt::Assign check.rs:3210 | shape: checker.rs:1928-1930 stale Known(empty) (2417/2467) AND compute_hot_params Seed A 1518-1521 registers a param only when the call target is textually the param name, so cb is never hot; rows: infer.rs:862-866 (true,false) arm pins the param's open row var to the closed row of state | measured: each flip alone: a dead `cb()` call (v1) -> E0507 [fs:read], `let state2 = ...` (v2b) -> E0507 [all], initializer cb (v3) -> E0110; my capcheck_x matrix: KRYOS_X_FIX_CAPS rc=1, KRYOS_X_FIX_ROW rc=1; KRYOS_X_RP=assign rows-only rc=1 |
| 63 | tests/security/attack_round3_trycatch_deny_narrowed_catch.kry | shape: check_stmt TryCatch arm checker.rs:5146-5158 -> catch block -> Stmt::DenyBlock 5168 -> check_callee_capabilities 5503-5515 -> Identifier arm 1927-1930; rows: Stmt::Assign check.rs:3210 | shape: checker.rs:1928-1930 stale Known(empty) (try/catch is incidental); rows: infer.rs:867-874 closed/closed no-op | measured: KRYOS_CAP_TRACE triage tcn-deny-narrowed-catch; flips: straight-line without try/catch still accepted, `let slot = f()` -> E0507 [fs:read], no-initializer or param-initializer forms -> E0110; my capcheck_x matrix: FIX_CAPS rc=1, FIX_ROW rc=1; KRYOS_X_RP=assign rows-only rc=1 |
| 64 | tests/security/attack_deny_double_indirection_wrapper_local.kry | shape: go() inside deny! in outer -> resolve_direct_invoke_caps 3690-3691 -> Identifier arm 1927-1930 returns Known(empty) for go; the entry was FROZEN at pre-pass by the Lambda arm fallthrough checker.rs:1918-1925 -> collect_caps_expr 4429-4443 -> deferred_own_param_caps 3567-3589 (scope not yet narrowed so line 3587 returns empty); rows: DenyBlock check.rs:3507-3513 | shape: checker.rs:3587 CapabilitySet::empty() lowered a DependsOnParam at pre-pass and 1918-1925 froze it; rows: check.rs:3513 concrete_bits() drops the open row var of param reader | measured: triage dbl-indirection-wrapper-local trace (deferred_own_param_caps scope_len=0 entry_depth=0 -> {}); prototypes alone: P1 (Lambda arm carries the deferral) -> E0507, P3 (open var at the deny boundary is all) -> E0110, my KRYOS_X_RP=openvar rows-only rc=1; flip: add a direct reader() call to go's body (captured-hot-param branch) -> E0507 **CLOSED 2026-11-02 (root R3): see the R3 section below.** |
| 65 | tests/security/attack_match_bound_var_field_invoke.kry | shape: check_expr MethodCall arm checker.rs:5195-5245 -> resolve_method_field_invoke_caps 3771 (decompose succeeds, no tracked literal 3784, no type entry 3820); rows: DenyBlock check.rs:3507-3513 | shape: checker.rs:3829 final CapabilitySet::empty(): root b is pattern-bound and absent from the let-only maps built at 2319, 2752, 2869; rows: check.rs:3513 drops the open var (Box.f row var is declaration-global and the factory freshens its copy) | measured: kh_inst3.exe KRYOS_CAP_TRACE (re-run): rmfic root=b path=[] method=f has_lit=false has_ty=false in_deny_scope=true ret=FALLTHROUGH (checker.rs:3829) -> empty; flip: decoy typed `let b: Box = mk(0)` -> Case 2 (3823-3826) all -> E0507; my KRYOS_X_RP=openvar rows-only rc=1 (E0110); triage P3 prototype rc=1 **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 66 | tests/security/attack_realprogram_registry_list_wrapper.kry | shape: MethodCall 5195 -> resolve_method_field_invoke_caps 3771 for p.run() inside deny!; rows: Plugin.run slot var, StructLiteral store check.rs:6174 | shape: checker.rs:3829 (the `let p = reg.get(1)` sits inside the deny body: the three builders have no DenyBlock arm 2467/2840/2935 and a MethodCall initializer is never typed 2876-2903); rows: infer.rs:867-874 first store Plugin{run: safe} binds the per-declaration slot var to {} and the evil second store is dropped | measured: kh_inst3.exe trace (re-run): rmfic root=p ... in_deny_scope=true ret=FALLTHROUGH (checker.rs:3829); unify trace: Function arm (true,true) c1={} c2={fs:read} -> NO-OP; flips: hoist plus annotate `let p: Plugin` -> E0507 [all], evil-first order -> E0110; my KRYOS_X_RP=slots rows-only rc=1 **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 67 | tests/security/attack_realprogram_registry_foreach.kry | shape: p.run() MethodCall whose receiver is the lambda's own param, reached through the Lambda arm checker.rs:1816-1925 (hot_own_param at 1831 sees only a direct call p()) -> resolve_method_field_invoke_caps 3771; rows: same Plugin.run slot | shape: checker.rs:3829 (lambda params get no type entry; the lambda falls through to Known(collect_caps_expr(..)) at 1918-1925); rows: infer.rs:867-874 first-store-wins on Plugin.run | measured: kh_inst3.exe trace (re-run): rmfic root=p path=[] method=run has_lit=false has_ty=false ret=FALLTHROUGH (checker.rs:3829); flips: evil plugin first or alone -> E0110, receiver replaced by an outer tracked literal -> E0507 [fs:read]; my KRYOS_X_RP=slots rows-only rc=1 **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 68 | tests/security/attack_index_field_invoke_factory_element_deny.kry (new, committed) | shape: MethodCall 5195 -> resolve_method_field_invoke_caps 3771 for tools[0].run(..) inside deny!; rows: DenyBlock check.rs:3507-3513 | shape: checker.rs:3786 early return (literal_field_exists 3917-3939 is false because the tracked array literal's element at the Index step is a FnCall make_tool(reader), not a StructLiteral); rows: check.rs:3513 drops the open row var | measured: kh_inst3.exe trace (re-run): rmfic root=tools path=[Index] method=run ret=LIT-BRANCH-FIELD-MISSING (checker.rs:3786) -> empty; flip: element written as an inline struct literal -> rejected (triage p7); my KRYOS_X_RP=openvar rows-only rc=1; independent of KRYOS_NO_HOTPARAM_ALL (reproduces with it unset); executed on master: secret printed **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 69 | tests/security/assault4_actor_handler_map_param_probe_v2.kry (as written: main holds fs:read so it is delegation once the sender is charged; escaping form is the deny! variant tests/security/attack_map_param_alias_invoke_deny.kry, the triage scratch) | shape: callee body f() -> resolve_direct_invoke_caps 3709 -> deferred_own_param_caps 3567-3589 (empty at 3587, no deny in the callee); send site check_callee_capabilities 5467 -> accumulate_hot_extra_caps 3294-3296 finds no hot_params entry for run_it; rows: map literal typed by its first element check.rs:6084-6109 | shape: checker.rs:3587 deferral with no taker (compute_hot_params Seed B 1531-1552 registers only an immediate param[path]() chain; the alias `let f = reg[name]` is invisible; producer 2399-2417 and 2237); rows: infer.rs:867-874 plus first-element map typing | measured on master: direct spelling reg[name]() -> E0507 [all], alias plus one dead direct call -> E0507, swapped map entries -> E0110, hot_params trace shows the index counts explicit self; prototypes: KRYOS_X_SP=taker rc=1 but newly rejects ecosystem/kryos-tool-broker (2 files); with taker plus actorself the as-written file is ACCEPTED (sender charged [fs:read]); KRYOS_X_RP=slots rows-only rc=1 on the deny form **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 70 | tests/security/assault4_actor_handler_map_param_strict_probe.kry (as written: delegation once the sender is charged; escaping form tests/security/attack_array_param_alias_invoke_deny.kry, the triage scratch) | same two layers as item 69: callee alias read deferred (checker.rs:3587 via 3709) with no hot_params taker (3294-3296, Seed B 1531-1552); rows: array literal typed by its first element check.rs:6050-6080 | shape: checker.rs:3587 deferral with no taker; rows: infer.rs:867-874 (the privileged lambda listed second is dropped) | measured: triage a4_actor_map_param_strict flips: privileged element first -> E0110 only, direct spelling reg[1]() -> E0507 only, alias plus unreachable direct call -> E0507 [all]; my re-run: array form accepted in all modes, KRYOS_X_SP=taker rc=1, KRYOS_X_RP=slots rows-only rc=1; the as-written file stays accepted under KRYOS_X_RP=all plus KRYOS_X_SP=select (delegation) **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 71 | tests/security/attack_actor_explicit_self_swapped_params_deny.kry (new, committed) | shape: MethodCall 5195-5245 d.run_it(name, reg) inside deny! -> compute_hot_extra_caps 5218 -> accumulate_hot_extra_caps 3294-3312; rows: map literal typed by its first element (build_registry returns pure first) check.rs:6084-6109 | shape: checker.rs:3299-3312 has_self_offset is false for actor handlers but parse_param_list (kryos-parser parser.rs:767-780) keeps an explicit self as params[0], so hot index 2 is looked up as argument index 2 of 2 and args.get(arg_idx) else continue charges nothing (same defect in the propagation loop 1573-1575); rows: first-element typing | measured: kh_inst3.exe KRYOS_CAP_TRACE (re-run): hot_params fn=run_it param_idx=2 paths={[Index]} and extra_into_enforce=[]; flip on master: deleting only `self` from the handler -> E0507 (a_direct_swapped_deny_noself); prototype KRYOS_X_SP=actorself closes it but drops matrix PRECISION to 18/75, so closure goes through rows: KRYOS_X_RP=slots rows-only rc=1 **CLOSED 2026-11-01 (root R2): see the R2 section below.** |
| 72 | tests/security/assault_r1_real_program_plugin_platform_nopool.kry (new, committed) call in main removed; the triage scratch) | shape: PluginHost.run_all and run_named deny! blocks (file lines 134-150): p.run(arg) -> resolve_method_field_invoke_caps 3771; rows: DenyBlock check.rs:3507-3513 and the Plugin.run slot | shape: checker.rs:3829 FALLTHROUGH root=p in_deny_scope=true (4 sites); rows: infer.rs:867-874 NO-OP (honest plugin stored first, compromised second) AND the DenyBlock row is {?C26}, an open var dropped at check.rs:3513 | measured: master check rc=0 and run prints EVIL-SUPPLY-CHAIN-LEAK and TOPSECRET from inside deny! on all four routes A-D; the shipped file is rejected only by an UNRELATED E0507 [all] at run_worker_pool (spawn precision over-rejection) which masks all four routes; rows-only closure needs slots plus the deferral and raw-charge mechanism (KRYOS_X_RP=slots,openvar or slots,assign rc=1 at lines 134 and 142); the shape-only spawn arm flips the shipped file to ACCEPTED, so land rows before any spawn precision fix **2026-11-01: NARROWED by R2, not closed: see the R2 section below.** **CLOSED 2026-11-02 (root R3): see the R3 section below.** |

## 2026-10-31: item 77, measured

Found by the second verifier of PR #18 while probing the reassignment walk
(`f = setter(reader)`, `f = pick(harmless, reader)`). Item 58 left this list
the same day: walking every expression form puts a `spawn fn() { let f = .. }`
lambda body into the local map, so its shadow of an outer `f` is unioned and the
call is rejected.

| item | repro | reaches | fail-open line | notes |
|---|---|---|---|---|
| 77 | tests/security/attack_reassign_local_via_local_closure_return_lambda.kry and tests/security/attack_reassign_local_via_local_closure_return_nested_fn.kry (escape_status rows 77a, 77b) | shape: `build_local_closure_caps_block` Assign arm -> `resolve_closure_caps` FnCall arm with an `Identifier` callee that is a LOCAL (`setter`, `pick`) | checker.rs `if let Some(r) = local_caps.get(name) { return r.clone(); }` in the FnCall arm (about line 2010) returns the local closure's OWN invocation authority as the authority of the VALUE it returns | Measured 2026-10-31: replacing that early return with the fail-closed answer that requires `all` (temporary, reverted) makes both repros `check` rc=1, so the shape layer alone decides and no row-layer change is needed to reject them. A top-level `fn pick`/`fn ident` with the same body is rejected (the `fn_return_caps` `DependsOnParam` arm charges the argument), so only a local lambda or a nested `fn` is open. Not closed here: the return-provenance of a local lambda needs its parameter list and returned expressions, which `local_caps` does not keep, and the blanket fail-closed answer rejects every call through a local closure, curried and higher-order ones included (how many legitimate programs that costs was not measured). **CLOSED 2026-11-02 (root R3): the row layer rejects both repros (E0110), see the R3 section below.** |

## 2026-11-01: root R2 (slot rows), measured

Landed in `kryos-types` (`infer.rs`, `check.rs`). The row layer failed at four
sites, each measured on master `dcf72e93` before it was edited:

1. `infer.rs` `unify`, `Type::Function` arm, `(true, true) => {}`: two closed rows
   bind nothing. `resolve_bounded` called `resolve_cap_row` on every function type,
   so a slot whose row var had been bound met a CLOSED copy of its own row. The
   first store bound the var, every later store was dropped.
2. `check.rs` `Expr::ArrayLiteral` / `Expr::MapLiteral`: typed by the first
   element, the rest unified into a closed row.
3. `infer.rs` `instantiate` (struct literal, enum constructor, generic field
   call): `instantiate_row` resolved the declaration-global slot var first, so the
   field's row was frozen at the first value stored; `Type::Var` bound to a
   closure captured that closure's closed row (`push<T>`, `List<T>`, `Some(..)`).
4. A declaration's body that stores its OWN parameter into a global slot bound the
   slot to a polymorphic var that no call site binds (each `instantiate_sig`
   freshens a copy).

What closes them: slot vars (`slot_cap_vars`, `global_slot_cap_vars`), widened by
`unify_rows`; `resolve` keeps the row as written; `literal_slot_type` and
`generalize_rows`; `resolve_cap_row_keeping_slots` in `instantiate_row`;
`begin_store_effects` / `end_store_effects` / `replay_store_effects` at the three
declaration sites and the four reference sites.

| item | repro | reaches | fail-open line (master) | what closes it |
|---|---|---|---|---|
| 59 | tests/security/attack_for_binder_shadow_closure_deny.kry | row: `Expr::ArrayLiteral` then the `deny!` verdict (check.rs `Stmt::DenyBlock`) | the literal `[pure, reader]` is typed by `elements[0]`; the second element unifies closed/closed | `literal_slot_type`: the element type is a fresh slot every element unifies into; E0110 in default and strict; run refused |
| 65 | tests/security/attack_match_bound_var_field_invoke.kry | factory `make_boxed_opt(reader)` stores its parameter in `Box.f`; `b.f()` inside `deny!` | `Box { f: r }` binds the declaration-global slot to the factory's own parameter var, which no call binds | `replay_store_effects`: the call replays the store with the caller's closure; E0110. The 2026-10-15 routing put this under R3 ("the factory freshens its copy"); the replay closes that half |
| 66 | tests/security/attack_realprogram_registry_list_wrapper.kry | `Plugin { run: .. }` stored twice (`List<Plugin>`), `p.run()` inside `deny!` | `instantiate` flattened `Plugin.run`'s slot var at the literal; the first store pinned it | field slot (`mark_slot_rows`), raw `instantiate`, `unify_rows` widening; the field call charges the slot var itself |
| 67 | tests/security/attack_realprogram_registry_foreach.kry | the same slot read through a lambda parameter typed `Plugin` inside `for_each` | same | same |
| 68 | tests/security/attack_index_field_invoke_factory_element_deny.kry | `make_tool(reader)` factory element, `tools[0].run(..)` inside `deny!` | same as 65 | replay |
| 69 | tests/security/attack_map_param_alias_invoke_deny.kry | `run(m)`: the callee reads `let f = m["s"]; f()`, the map literal is typed by its first value | map literal typed by the first value; the callee's charge sat behind a local slot | literal slot; a LOCAL slot is flattened in `instantiate_row`, so the callee's polymorphic param var is remapped to the call's copy |
| 70 | tests/security/attack_array_param_alias_invoke_deny.kry | same with an array | same | same |
| 71 | tests/security/attack_actor_explicit_self_swapped_params_deny.kry | `build_registry()` returns a map literal; handler `run_it(self, name, reg)` | map literal typed by the first value | literal slot (the shape-layer hot-index defect is NOT fixed, and does not need to be) |
| 72 | tests/security/assault_r1_real_program_plugin_platform_nopool.kry | `PluginHost.run_named` / `run_all` `deny!` blocks, judged when the actor is checked | the verdict runs before main's `Plugin { run: .. }` stores, on the unbound slot | NOT closed in the shipped declaration order (verdict timing: R3). With `main` declared above the actors (tests/security/assault_r1_real_program_plugin_platform_stores_first.kry) both blocks are rejected, E0110 at lines 200 and 208. Route C (`dispatch_all`) is not inside a `deny!` **CLOSED 2026-11-02 (root R3): see the R3 section below.** |
| 112 | tests/security/attack_slot_param_container_store.kry | callee `register(reg, g) { reg[0] = g }`, caller iterates `fns` inside `deny!` | the callee bound its own parameter's element row var to the row it stored (`unify_rows` first loop) and the call bound that var's copy to the argument (parameter <- argument), nothing flowed back | `input_storage_cap_vars`: the body never binds a handed container's storage var, it records the store (`record_param_store`); `replay_store_effects` writes it into every slot the argument's element row reaches (`store_into_input`, `deferred_input_stores` when the copy is bound later). The lambda form has no per-call copy and uses `lambda_input_stores`. CLOSED |
| 114 | tests/security/attack_slot_register_after_main.kry | `register(reg, name, f)` (or `put(reg, g)`, or a function returning `[safe, g]`) declared below the `main` that calls it; `deny!` reads the result | the callee's effects are recorded when its body is checked, in file order; the call in `main` precedes that, so `replay_store_effects` finds none and the return row is still unbound | OPEN. Take the effects of every declaration before any body that reads them is checked (measure against the corpus first: a pre-pass also binds forward-referenced rows early, and doubles the check time of a program) **CLOSED 2026-11-02 (root R3): see the R3 section below.** |
| 113 | tests/security/attack_slot_param_struct_field_store.kry | callee `swap_in(p, g) { p.run = g }`, then `q.run()` inside `deny!` | the field's slot var bound to the callee's own parameter var `g` | `replay_store_effects`; E0110. Same mechanism closes `register(reg, name, f)` and `reg.add(name, f)` (attack_slot_register_helper.kry, attack_slot_register_method.kry) |

### The R2 revise round (2026-11-01): the independent verifier refuted the first head

The first R2 build (dbfb989d) was refuted by an independent verifier: `fn build_with(g) -> [fn() -> str] { let mut a = [safe]; a = push(a, g); return a }`
called with a privileged closure and iterated inside `deny!(fs:read)` was accepted
in default and strict mode and printed the secret (`return [safe, g]` too). The row
layer widened the slot, but `unify_rows` kept the original rule "the still-unbound
vars of the SOURCE take the destination's closed row", and the source here was the
parameter's own row var `g`: bound to the empty row of `[safe]`, so every caller was
told the parameter is pure. Measured with a temporary trace in `unify_rows` (a scratch build, not
committed): `c1=[slot] c2=[g] before={}`, `rule1 bind slot := {g}`,
`rule2 bind g := {}`. The same rule was behind a registration helper declared after a
pure store, a call made after the narrowing, a lambda's annotated parameter, the
generic and `Option` / enum / struct containers, and the middleware chain.

Three more holes on the way, each measured before it was fixed: the several `return`s of
a function and the arms of an `if` / `match` were typed by the first one (the
declaration's return row was bound once and not a slot); the parameter position of a
function type was unified in the function value's direction, so a lambda with an
annotated closure parameter handed to `for_each(fns, ..)` over an array parameter was
never linked to the array's element row (`--strict-capabilities` accepted it and
printed the secret; the default mode caught it through the shape layer); and a store
into a container the body was handed bound the parameter's own element row, so a pure
store made later reads through the parameter look pure (`reg[0] = safe; reg[1]()`).

| item | repro | reaches | fail-open line (first R2 build) | what closes it |
|---|---|---|---|---|
| 117a | tests/security/attack_slot_ret_push_param.kry (and 24 more `attack_slot_ret_*`, `attack_slot_register_after_pure_store`, `attack_slot_call_after_narrow`) | `Expr::FnCall` (`push<T>`) -> `unify`, `Type::Function` arm -> `unify_rows`; then `Stmt::Return` against the declaration's return type | infer.rs `unify_rows`, second loop: `for v in c2.var_ids() { if !cap_substitutions.contains_key(v) { bind_cap_var(v, before) } }` binds the parameter's own var to the closed row the slot held | `is_frozen`: the second loop skips the signature vars of the body being checked (`effect_scope.poly`, and `frozen_cap_vars` for the parameter types of a lambda whose body is open) |
| 117b | tests/security/attack_slot_two_returns.kry, attack_slot_if_arms.kry, attack_slot_match_arms.kry | `Stmt::Return` -> `unify(sig.ret, value)`; `Expr::IfExpr`, `Expr::MatchExpr` arms | the return row var is bound by the first `return` and is not a slot, so the first loop's `contains_key` guard drops the rest; an `if` / `match` returned `then_ty` / the first arm's type | `mark_slot_rows(&sig.ret, false)` at the declaration, handler and method sites; `literal_slot_type` at the join of the arms |
| 117c | tests/security/attack_slot_lambda_param_single_call.kry, attack_slot_lambda_param_two_calls.kry | `Expr::Lambda` (annotated parameter, template var) then `unify(param_ty, arg_ty)` at the call of the local | body narrowing (117a) and the `contains_key` guard: the first call binds the monomorphic parameter row, the second is dropped | `mark_lambda_param_rows` (slots), `freeze_rows_of` for the body, and `lambda_param_cap_vars` widened by the second loop |
| 117d | tests/security/attack_slot_lambda_param_over_container.kry, ..._map.kry | `Expr::FnCall` (`for_each` / `map`) -> `unify(fn(T) -> U, lambda)`, parameter loop of the `Type::Function` arm | the parameters were unified in the function value's direction: `unify_rows(E, Lg)` widened the array's element row by the lambda's still-unbound var and bound nothing to it | the `contravariant` flag: below a parameter list `unify_rows(c2, c1)`, the found function's parameter takes what the expected one hands it |
| 117e | tests/security/attack_slot_param_store_pure_then_read.kry, attack_slot_param_index_assign.kry, ..._map_store, ..._push_inplace, ..._helper_chain, ..._list_push, ..._mut_ref, ..._method, ..._fn_value, attack_slot_lambda_container_param_store.kry, ..._push | `Stmt::Assign` (index / field target) and `push` -> `unify`, `Type::Function` arm -> `unify_rows` first loop | the first loop binds the parameter's element row var to the row stored; reads through the parameter are charged that row alone, and the call never touches the caller's slot (item 112) | see item 112 above |
| 115 | tests/security/attack_deny_in_callee_before_stores.kry | `fn run_all(fns) { deny!(fs:read) { for f in fns { f() } } }` declared above `main`; check.rs `Stmt::DenyBlock`, `let resolved = self.engine.resolve_cap_row(&body_caps)` then `resolved.concrete_bits()` | the block is judged when `run_all` is checked: the row of `fns`' elements is still the callee's own open parameter var, `concrete_bits()` drops it, and the argument only arrives at the call in `main`, later | OPEN (root R3). Record a deny obligation (denied set, body row, span) at the block and check it at each call once the arguments are unified, or judge at the call **CLOSED 2026-11-02 (root R3): see the R3 section below.** |
| 116 | tests/security/attack_deny_in_method_before_stores.kry | the same `Stmt::DenyBlock` site, in a method declared above `main` that iterates a struct field | the field's slot is empty when the block is judged; `main` pushes the privileged closure into it later in the file | OPEN (root R3, the same class as item 72 without the actor). Judge `deny!` rows after the module, or defer them **CLOSED 2026-11-02 (root R3): see the R3 section below.** |

## 2026-11-01: the R3 wave (deny! verdict timing) - findings recorded first

The independent verifier of the R2 wave reported fifteen more escapes and three variants. They are
committed as repros before any checker edit and measured on master `732714ab` (check rc=0 in default and
strict mode, `kryos run` prints `TOPSECRET-CLOSURE-9f8e7d6c5b4a` in both modes). Their roots:
R2 (items 118, 119: a store into a handed container reached through a call form the replay does not model, or
through a parameter type `storage_rows_in` does not walk), R3 (item 120: the verdict timing placements the
ledger did not list) and the row layer's own deny matching (item 121).

| item | repro | reaches | fail-open line | notes |
|---|---|---|---|---|
| 118 | tests/security/attack_slot_param_fn_value_hof_param.kry, `_struct_field`, `_factory`, `_map_of_ops`, attack_slot_lambda_wrapper_calls_put.kry, attack_slot_param_trait_bound_dispatch.kry, attack_slot_dyn_registrar_store.kry (rows 118a-118g) | `put(reg, g) { reg[0] = g }` is reached through a call the checker types as a call through a function value (a parameter, a struct field, a map element, a factory result, a lambda that calls the helper) or through a trait method (`use_reg<R: Reg>`, `dyn Registrar`) | `replay_store_effects` is called at a REFERENCE to a declaration (identifier, instance method, static method, module-qualified call); these calls reference no declaration, so the recorded store is never written into the slot of the argument | **PARTLY CLOSED 2026-11-02: 118a, 118b, 118d and 118e by the R3 wave (see the R3 section below); the other three repros are item 123.** Carry the callee's stores with the function value, or fail closed on a call through a function value that is handed a container of closures |
| 119 | tests/security/attack_slot_param_container_in_tuple.kry, `_in_result`, `_array_of_option`, `_option_of_nested_array`, `_tuple_of_two_arrays` (rows 119a-119e) | the handed container sits inside a Tuple, Option, Result or Enum parameter | `infer.rs` `storage_rows_in` walks Array / Map / Set / Struct generics / Reference / Shared only, so the element row inside the other four is not input storage and the body's store is not recorded | OPEN (root R2, left open by the R3 wave). Walk Tuple / Option / Result / Enum payloads, then measure |
| 120 | tests/security/attack_deny_loop_carried_store_after_block.kry, `attack_deny_in_callee_after_main`, `attack_deny_in_lambda_defined_before_store`, `attack_deny_global_slot_reader_before_main`, `attack_deny_show_after_add_before_main` (rows 120a-120e) | check.rs `Stmt::DenyBlock` at the block, on a row that mentions a local slot filled after the block (a loop), a callee declared below, a global slot, or the polymorphic row of a parameter | `resolve_cap_row(&body_caps)` then `concrete_bits()` drops every var that is unbound when the block is checked: same site as items 115 and 116 | **CLOSED 2026-11-02 (root R3): see the R3 section below.** Judge the block once nothing can bind any more |
| 121 | tests/security/attack_deny_coarse_family_row_blind.kry | the same `Stmt::DenyBlock` verdict, with `deny!(fs)`, `deny!(io)` or `deny!(all)` | `CapabilitySet::satisfies_required` is containment in one direction: a used `fs:read` does not satisfy a denied coarse `fs` | OPEN (left open by the R3 wave). Judge by overlap (`overlaps_capability`) |

## 2026-11-02: root R3 (the deny! verdict is taken when the module is checked), measured

Landed in `kryos-types` (`infer.rs`, `check.rs`, `ty.rs`). The row layer failed at one site, measured on
master `732714ab` with `KRYOS_ROW_TRACE=1` and a trace of the block's row: `check.rs` `Stmt::DenyBlock`
resolves the body's row and reads `concrete_bits()`, which drops every var that is still unbound when the
block is checked. Three kinds of var are, and are bound later:

1. the declaration's own parameter row (a signature var): no body binds it, each reference freshens a
   copy and binds the copy to its argument (`fn outer = {?C6}` in the trace of item 64);
2. a slot filled by a store that comes later than the block (a field or a global `main` fills after the
   method that reads it was checked, a local array a loop fills after the block);
3. a callee or helper declared below the block or the call (its own row is unbound while the caller is
   checked).

What closes them: `record_deny_obligation` files the raw row; `reference_row` records every reference to a
declaration with polymorphic row vars together with its freshening of them (and, for a reference that
precedes its declaration, a var of its own bound later to the declaration's row instantiated for that
reference); `complete_references` replays the stores of every reference and binds those copies; and
`settle_denies` judges every block on the final row, re-instantiating the blocks that still depend on an
unbound var at each reference (a worklist, so a relay or a recursion that hands its parameter on is an
obligation of the helper). A method of a trait impl is also reached through a generic bound or a `dyn`, which
reference the trait's signature, so a block there that depends on a parameter row is judged on `all`.

| item | repro | reaches | fail-open line (master) | what closes it |
|---|---|---|---|---|
| 64 | tests/security/attack_deny_double_indirection_wrapper_local.kry | the block in `outer(reader)` calls `go()`, a wrapper over a local alias of the parameter | `Stmt::DenyBlock` judges `{?C6}`, `outer`'s own parameter var; `main`'s `outer(reader)` comes after the verdict | the obligation is re-instantiated at `outer(reader)`: the copy is bound to `reader`'s row, `{fs:read}`; E0110 with a second label on the call. Relatives pinned: alias, higher-order function, actor handler, impl method, struct parameter, relay, recursion, method chain, function value, lambda |
| 72 | tests/security/assault_r1_real_program_plugin_platform_nopool.kry (and the shipped assault_r1_real_program_plugin_platform.kry) | the two `deny!` blocks of `PluginHost.run_named` / `run_all`, judged when the actor is checked (and in the actor pre-pass) | the `Plugin.run` slot is empty at that time; `main`'s `Plugin { run: .. }` literals fill it later in the file | `settle_denies`: the slot is widened by then; both blocks rejected, E0110 at lines 134 and 142, in the shipped declaration order. Route C (`dispatch_all`) is not inside a `deny!`. Pure twin: ctl_deny_plugin_platform_pure.kry accepted |
| 77a, 77b | tests/security/attack_reassign_local_via_local_closure_return_lambda.kry, `_nested_fn.kry` | `f = setter(reader)`, `f()` inside `deny!` in `outer(reader)` | the shape layer resolves the call through the local closure's own authority; the row layer judged `outer`'s parameter var | same as 64: the lambda returns its parameter, so the block's row is the parameter var; closed with no shape-layer change (the routing of 2026-10-31 said the shape layer alone decides; the row layer decides it too once the open parameter var is respected) |
| 114 | tests/security/attack_slot_register_after_main.kry | `register(reg, name, f)` (or `put(reg, g)`, or a function returning `[safe, g]`) declared below the `main` that calls it | the reference precedes the declaration: `replay_store_effects` found no stores, and the return row was still unbound | `complete_references` replays the stores and binds the copies once the body is checked; all three forms and the forward-order twins of items 69 and 70 are rejected |
| 115 | tests/security/attack_deny_in_callee_before_stores.kry | `run_all(fns)` declared above `main` | the block is judged on `fns`' element row, the callee's own parameter var | re-instantiation at `run_all(fns)`; the copy is bound to the array's element slot, `{fs:read}`. Control: ctl_deny_param_callee_pure.kry |
| 116 | tests/security/attack_deny_in_method_before_stores.kry | `Host.run` declared above `main` iterates `self.items` | the field slot is empty when the block is judged | `settle_denies` sees the slot after `h.items = push(h.items, r)`. Control: ctl_deny_late_method_field_pure.kry |
| 118a, 118b, 118d, 118e | tests/security/attack_slot_param_fn_value_hof_param.kry, `_struct_field`, `_map_of_ops`, attack_slot_lambda_wrapper_calls_put.kry | a store helper (`put`) reached through a function-typed parameter, a struct field, a map element, or a lambda | not traced; the stores are replayed when the reference is typed, and through a function value the argument reaches the copy of the storage var only later | `complete_references` replays them again once every body is checked and the arguments are unified (measured: rejected with it, accepted without it); pure twins accepted (ctl_slot_param_fn_value_*_pure.kry). The rest of item 118 is item 123 |
| 120a-e | tests/security/attack_deny_loop_carried_store_after_block.kry, `attack_deny_in_callee_after_main`, `attack_deny_in_lambda_defined_before_store`, `attack_deny_global_slot_reader_before_main`, `attack_deny_show_after_add_before_main` | a store after the block in a loop; a callee declared after `main`; a lambda defined before the store; a global slot read by a function declared before `main`; `show(fns)` before the helper that fills `fns` | the same `Stmt::DenyBlock` site | the settled verdict. Controls: ctl_deny_late_*.kry, ctl_deny_param_*.kry |
| 122 | tests/security/attack_deny_trait_default_method_unchecked.kry | the BODY of a trait default method: `check_decl_inner` has no arm that checks it, so its `deny!` block is never visited (a type error in one is accepted) | no row is computed for it at all | OPEN, found by this wave. Check the body like an impl method, then measure what it newly rejects |
| 123 | tests/security/attack_slot_param_fn_value_factory.kry, attack_slot_param_trait_bound_dispatch.kry, attack_slot_dyn_registrar_store.kry (rows 123a-123c) | a store helper reached through a factory result, a generic function with a trait bound, a `dyn` dispatch | not traced (hypothesis): the reference to `put` sits inside the factory (or the impl), so the call in `main` instantiates the factory's return var, not the copy the reference made; a trait call references the trait's signature, not the impl's. Measured: `complete_references`, which replays the stores of every reference it records, does not reach the argument | OPEN (root R2). Carry the callee's stores with the function value, or fail closed |

## 2026-11-02: the item-77 wave - findings recorded first

The independent verifier of the R3 wave (PR 39) reported three more roots, and this wave found two more while hunting the siblings of item 77. All
are committed as repros before any checker edit and measured on master `e2fbc46c` (check rc=0 in default and strict mode, `kryos run` prints
`TOPSECRET-CLOSURE-9f8e7d6c5b4a` in both modes; `KRYOS_ROW_TRACE=1` for the row-layer ones). Roots: F (forward references, items 124 and 125), R4
(item 126) and two shape-layer defects in `resolve_closure_caps` (items 127 and 128), which the row layer would catch but a `deny!` in a select arm
hides from it.

| item | repro | reaches | fail-open line | notes |
|---|---|---|---|---|
| 124 | tests/security/attack_fwd_param_chain_invoke_in_deny.kry, `_container_in_deny`, `_method_in_deny`, `_actor_in_deny`, attack_fwd_rev_hotparam_forward_via_local_alias.kry, attack_fwd_rev_slot_param_helper_chain.kry, attack_fwd_mainfirst_verify_actor_to_actor_message.kry (rows 124a-124g) | row layer: `tool(reader)` (declared above `invoke`) copies its parameter into `g` and hands it to `invoke`; `main` calls `tool(r)` inside `deny!` | `settle_denies` judges `main`'s block on `{?C6}`, `invoke`'s parameter var: `KRYOS_ROW_TRACE=1` prints `fn tool = {?C7}`, `fn invoke = {?C6}`, `fn main = {?C6}` (with the helpers swapped `fn main = {fs:read}` and E0110) | OPEN (root F, found by the R3 verifier). The verifier's reading, not re-traced: `complete_references` binds the copy of each var the declaration bound to the callee's row, but a function declared above its callee hands its row on through a copy var that `main`'s later instantiation never maps |
| 125 | tests/security/attack_fwd_store_relay_two_hop.kry, `_global_slot`, `_method`, attack_fwd_builder_relay_returns_container.kry, attack_fwd_fill_relay_struct_field_after_main.kry, `_before_main` (rows 125a-125f) | row layer: `h1(reg, name, f)` returns `h2(reg, name, f)` (h1 declared above h2), `h2` pushes a closure into `reg`; `main` reads the array inside `deny!` | `fn h1 = {?C8}`, `fn h2 = {}`, `fn main = {?C7}`: the store `h2` makes is not in `h1`'s store list, so the slot of `main`'s array never learns it (the twin with `h2` above `h1` is rejected, E0110, measured for every probe) | OPEN (root F, found by the R3 verifier). Record the stores a forward callee of a callee makes in the first callee's list |
| 126 | tests/security/attack_deny_in_select_arm_callee.kry, `attack_deny_in_select_arm_main`, `attack_deny_in_select_timeout_arm` (rows 126a-126c) | row layer: `Stmt::Select` and its timeout arm | check.rs `Stmt::Select` arm is empty: `fn host = {}` and `fn main = {}` with the block calling an fs:read closure | OPEN (root R4, found by the R3 verifier). Walk the arms (rows-only prototype of 2026-10-15 closes item 57 at 0 verdict changes over its corpus) |
| 127 | tests/security/attack_shape_named_closure_forwards_captured_via_helper.kry, `attack_shape_named_closure_two_captured_params` (rows 127a, 127b) | shape layer: `fn_return_closure_caps` of a top-level function whose returned closure literal uses a captured parameter, with the `deny!` in a select arm so the row layer is out of the picture | checker.rs `resolve_closure_caps`, `Lambda` arm, CAPTURED-HOT-PARAMETER case: `own_params.iter().find(..)` finds a captured parameter only when the literal CALLS it by name, and only ONE (a `HashSet`, so the pick varies from run to run when two are called directly); a parameter forwarded to a hot helper (`|| run(b)`) is deferred to empty | OPEN, found by this wave. Measured: `|| run(b)` accepted and leaks, `|| b()` rejected (E0507), `compose2(harmless, reader)` with `|| { a() + run(b) }` accepted and leaks, swapped rejected. Closing root: record every captured parameter the body defers (a set), not one |
| 128 | tests/security/attack_shape_named_chain_applies_returned_closure.kry (row 128) | shape layer: `make_getter(reader)()`, a chained call whose head is a top-level function, with the `deny!` in a select arm | checker.rs `resolve_closure_caps`, chained-call arm (`Expr::FnCall` callee that is itself a call): returns the value of the inner call, the flat authority of the closure `|| c`, as the value of applying it | OPEN, found by this wave. The same program through a local (`let g = make_getter(reader)`, `f = g()`) is rejected by the item-77 wave. Closing root: the value of applying a closure the checker holds no definition of fails closed (requires `all`), or a return flow is kept for the closure literals a function returns |
