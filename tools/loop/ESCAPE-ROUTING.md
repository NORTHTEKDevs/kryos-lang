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
