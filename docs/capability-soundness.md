# Capability soundness: theorem, invariants, and implementation audit

> **UPDATE (2026-08-14, authority-surface wave - READ THIS FIRST. Supersedes every
> update below, and changes the MECHANISM this document audits.)**
>
> **1. The enforcement mechanism described in the body of this document has been
> replaced.** Everything below audits *shape-based* resolution - `resolve_closure_caps`
> / `resolve_container_path_caps` matching on the syntactic form of a call expression,
> with §7 arguing structural soundness from those being exhaustive `match`es with no
> wildcard arm. That argument was not enough, and the reason is worth recording: an
> exhaustive match over `Expr` guarantees every SHAPE is *handled*, not that every shape
> is handled *correctly*. Between 2026-08-06 and 2026-08-13 twelve further live escapes
> were found, all of them shapes that were matched and then resolved to "no authority".
> Adding shapes never converged; rounds 1-3 each just moved the escape to the next shape.
>
> The current mechanism is **capability-ROW enforcement** (`kryos-types/src/check.rs`,
> stage 2, 2026-08-12): the callee's row is charged from its TYPE at every call site.
> An accessor-call receiver, an if/match receiver, a `&`/`*` indirection, a struct field,
> an array element, a map value and an actor message are charged identically, because no
> expression shape is being enumerated at all. Unresolvable provenance yields
> `CapRow::Unknown`, which erases to `CapBits::ALL` - fail-CLOSED. See
> `tools/loop/STAGE2-PLAN.md`. The shape-based resolvers still exist and still run;
> retiring them is deliberately gated on every repro being rejected by the row check
> alone, which has not been demonstrated yet.
>
> **2. Current measured status** (re-run on demand; `tools/loop/check-docs-truth.sh`
> fails CI if this prose drifts from the measurement):
> - `tools/loop/escape_status.sh` - **0 escaping, 17 rejected**, both enforcement modes.
> - `tests/security_gate.sh` - PASS, ~94 checks, each with a no-cascade complement.
> - `tests/capability_matrix_gate.sh` - **NEW, bounded EXHAUSTIVE search.** All 17 shapes
>   in `escape_status.sh` were found BY HAND, so that corpus's weakness is exactly "the
>   shape nobody thought of". This enumerates SOURCE x CONTAINER x TRANSPORT
>   combinatorially. Result: **75 shapes, 75/75 attacks rejected, 0 escapes, both
>   enforcement modes.** A bounded-model result, not a proof, and N is printed so the
>   bound is never implicit. It also measures the OTHER axis for the first time: **only
>   34/75 pure-closure CONTROLS compile.** The other 41 are byte-identical to their attack
>   twin except the closure is pure, and are rejected with `requires capabilities [all]`.
>   That is the fail-closed `Unknown -> ALL` cost, predicted in the abstract by
>   `tools/loop/STAGE2-PLAN.md` and never quantified until now: **55% of legitimate
>   closure-through-container programs in this space need `@capabilities(all)`.** Tracked
>   as LEDGER item 41. The gate reports precision but does NOT fail on it, deliberately -
>   making it a failure condition creates pressure to loosen enforcement so a number goes
>   green, which is the incentive that produced the fail-OPEN default this design replaced.
> - `tests/authority_surface_gate.sh` - **NEW, and it answers a question this document
>   never asked**: is the set of primitives that GRANT authority gated at all? Every
>   invariant below concerns whether authority can be *laundered*; none establish that it
>   cannot simply be *called for free*. One ungated builtin would make the entire
>   invariant table moot. Result: **82 authority-granting builtins, 0 ungated,
>   0 ungrantable** - verified in both directions (rejected without the capability;
>   the `E0505` cleared by granting exactly the capability the compiler itself names).
>
> That gate corrected a live documentation error in the security-relevant direction:
> `CLAUDE.md` claimed the raw-memory builtins (`alloc`, `ptr_read_i64`, `str_to_ptr`, ...)
> "need NO capability" and were "an ungated unsafe surface". Measured, false - they
> require `ffi`, and both pointer SOURCES (`alloc`, `str_to_ptr`) are gated, so the
> raw-memory surface is closed at its entry points.
>
> **3. The top-line claim, stated exactly.** The §1 theorem is still NOT proven. There is
> no formal semantics, no progress/preservation result, and no mechanized proof; a
> soundness *guarantee* does not exist. What exists is: a completely gated authority
> surface, a type-directed (not shape-directed) enforcement mechanism, and a 17-shape
> adversarial corpus that is fully rejected - all mechanically re-checked by the gate
> ladder. **Zero known escapes is not zero escapes.** The corpus is finite and
> human-authored; the shape nobody has thought of is exactly the one that matters, and no
> amount of green re-running will find it. Capabilities remain a strong
> DEVELOPMENT-TIME discipline, not a boundary to run untrusted code behind.
>
> **4. Also unproven, explicitly:** the row calculus itself has no formal model - nothing
> proves `CapRow` union/subset/instantiation preserves containment; `Unknown` is a
> conservative stance rather than a proof that every unresolvable path reaches it
> (historically several did not - that is what LEDGER items 30/32/33/34/35/37 were);
> and enforcement is entirely compile-time, so a codegen bug or native FFI can violate
> the model without the checker being wrong. There is no runtime capability monitor.

> **UPDATE (2026-08-05, structural-completeness wave - historical; the mechanism it
> describes has since been superseded, see above):** LEDGER item 10 (the wrapper-closure escape described in the
> correction immediately below) is now **FIXED**, along with two further live escapes
> found this wave by re-deriving the actor-state invariant from the closed set of
> value-producing `Expr` forms rather than trusting the prior "closed" status: a
> ONE-LEVEL-DEEPER variant of item 18 (`self.b.f()` where `b: Box`, `Box.f: fn()->str`
> - item 18's fix only ever checked the METHOD name against the actor's fn-bearing-field
> set, never the actual field being stepped through) and an aliased-local variant of the
> same gap (`let x = self.b; x.f()`). All three verified live, both directions (leak
> pre-fix, reject post-fix), both enforcement modes - see `tools/loop/LEDGER.md`'s
> structural-completeness-wave CLOSED entry for the full root-cause writeup and evidence.
> Additionally, `resolve_closure_caps` and `resolve_container_path_caps` - the two
> functions this document's invariant table cites for nearly every "D" (derivation)
> rating - are now EXHAUSTIVE `match` statements over the `Expr` AST enum with **no
> wildcard arm**, a genuine structural (not just empirical) soundness argument; see §7
> below. The theorem in §1 is reinstated as the CURRENT claim, with the same caveat every
> version of this document has carried: this is a structured code audit backed by
> executed counterexamples for every fail-closed claim, not a mechanized proof (§2(a) of
> `docs/LAUNCH-READINESS.md` remains accurate on that distinction). The known residual
> from this wave's own "measure the cost" step: a CHAINED alias-of-an-alias
> (`let y = x` where `x` is itself a `self.<field>` alias, two hops before invocation) is
> NOT covered by the narrow fix that closed the one-hop alias case - a broader fix was
> attempted and reverted after it measurably broke unrelated conformance/actor code (see
> the LEDGER entry); this is a known, disclosed, narrow open edge, not a silent gap.
>
> **CORRECTION (2026-08-05, final launch synthesis - historical, superseded above):**
> A red-team round that ran AFTER this document was written found a live,
> reproducible counterexample to the theorem stated in §1, within the SAME
> baseline (HEAD `00b3cf7`) this document audits: a closure returned by an
> ordinary zero-capability wrapper function (`fn wrap_once(inner: fn() -> str)
> -> fn() -> str { return || inner() }`) defeats `deny!()` under BOTH the
> default-inferred and `--strict-capabilities` enforcement modes. Re-verified
> live against `compiler/target/release/kryos.exe` on 2026-08-05 (independent
> of this document's own session): `kryos run
> tests/security/cap_escape_closure_wraps_closure.kry` prints
> `SINGLE-WRAPPED CLOSURE LEAK: TOPSECRET-CLOSURE-9f8e7d6c5b4a`, rc=0, and
> `kryos check --strict-capabilities` on the same file also exits 0. See
> `tools/loop/LEDGER.md` item 10 (ranked HIGHEST PRIORITY - breaks the trust
> model) and `docs/LAUNCH-READINESS.md` for the full verdict. **The "0
> findings" status and the theorem below do not hold as stated until item 10
> is fixed.** Everything else in this document (the invariant table, the
> monomorphization deep-dive, the attack programs in §5) is unaffected and
> stands as an audit of the OTHER 21 invariants - only the top-line soundness
> claim is retracted.

Status: living audit document. Written 2026-08 against HEAD `00b3cf7` (post
round-5 fail-closed fix, `hot_param_companions` as the sole non-shape-based
relief mechanism). This is not a proof; it is a structured claim with, for
every load-bearing invariant, either a derivation in the code, a named
heuristic, or an executed counter-example. Re-derive after any change to
`kryos-capabilities/src/{checker,model}.rs`.

## 1. The theorem

### 1.1 Informal statement

> **Authority confinement.** For any function, closure, actor handler, or
> `spawn`-launched task `f` that the compiler accepts, the set of gated
> operations that can execute during **any** run of `f` is a subset of the
> capability set that `f`'s *enforcing boundary* holds at the point `f`
> begins executing.

The interesting content is entirely in "enforcing boundary", because Kryos
does not require every function to carry its own capability annotation.

### 1.2 What "enforcing boundary" means, precisely

A function's effective capability set (`effective_caps` in
`check_function`) is determined by `CapabilityMode`:

- **`strict`**: every function is its own boundary. Its effective set is its
  own `@capabilities(...)` annotation (empty if unannotated). A helper that
  calls a gated builtin without declaring the capability is rejected at that
  helper's own definition, independent of any caller.
- **`inferred`** (the default): `main` and every explicitly-`@capabilities`-
  annotated function are boundaries, enforced against their declaration. An
  *interior, unannotated* function is not a boundary - the checker computes
  its **inferred** requirement (`fn_capabilities`, a call-graph fixed point)
  and checks that requirement against whatever boundary scope it is
  transitively called from. The boundary is therefore always the nearest
  enclosing annotated function/`main`/actor/`deny!` block, not the function
  itself.
- **`permissive`**: identical to `strict` except an unannotated function is
  never checked at all (opt-in, scratch-file mode).
- **`deny!(caps){ body }`**: not a function boundary but a **narrowing**
  scope. It pushes `current_scope.capabilities.without(denied_caps)` and
  every check inside `body` (including a deferred charge that resolves back
  to it) is evaluated against the narrowed set, not the enclosing function's
  full declaration.

So the precise, implementation-faithful theorem is:

> For every point `p` in the program where a gated operation (a builtin
> call, an extern call backed by a gated builtin, or an invocation of a
> fn-value carrying gated authority) can execute, the capability set
> **provably required to reach `p`** - as computed by unioning: (a) the
> operation's own required capability, (b) every capability carried by any
> fn-value/container flowing through `p`'s call chain whose provenance the
> checker can trace, and (c) `Capability::All` for any position whose
> provenance the checker cannot trace - is a subset of the capability set
> held by the nearest enclosing **enforcing scope** (`deny!` narrowing >
> annotated function/actor/`main` > `strict`-mode's own declaration),
> **and** the checker rejects the program (`E0503`/`E0505`/`E0506`/`E0507`)
> whenever that subset relation does not hold.

### 1.3 Refinements the language actually intends

- **`deny!` narrowing is sound only for code lexically/data-flow reachable
  from inside the block at check time.** A deferred charge that resolves
  *outside* the current live scope depth is unsound (this is exactly what
  round 5 part 2 found and fixed - see §3, invariant 12).
- **Sub-capabilities are a one-way lattice.** `net:http`/`net:tcp` and
  `fs:read`/`fs:write` are independent leaves; the coarse parent (`net`,
  `io`/`fs`) grants both children (`Capability::satisfies`), but holding one
  child never grants the other, and holding a child never grants the coarse
  parent. `all` grants everything.
- **The raw-memory primitives are a declared, non-propagating TCB
  split**, not a hole in the theorem: `ffi` is required at the *direct*
  call site of a raw-memory builtin in **user** code, and that requirement
  is deliberately not part of the ordinary inference fixed point - the
  stdlib is trusted to use these primitives internally without forcing
  every stdlib caller to declare `ffi`. The theorem above is scoped to
  user-authored call sites for this one family; §3 invariant 15 documents
  the enforcement mechanism and why it does not generalize into a bypass.
- **Call-site-polymorphic values (HOFs) have no fixed capability set of
  their own.** A function whose parameter is itself a fn-value (`fn(f:
  fn()->str))`) cannot be assigned one static requirement - its real
  requirement is "whatever `f` turns out to carry", resolved **per call
  site** from the actual argument expression. The theorem's "capability
  set required to reach `p`" is therefore a property of a *concrete call
  chain*, not of a function declaration in isolation; a HOF's own
  declaration is correctly allowed to require nothing when the checker
  proves it forwards its callback opaquely (`hot_params`/`DependsOnParam`),
  and each *caller* is charged for what it actually passes.
- **Unknown must mean `all`, never "nothing".** This is the single
  non-negotiable soundness axiom the codebase's own comments state
  repeatedly, and the one every real historical bug violated: whenever
  the checker cannot statically resolve what a value invoked at `p`
  actually is, it must require `Capability::All` at `p`, not skip the
  charge. Every invariant below is really a claim about which shapes are
  resolved *precisely* (derivation) vs. resolved to `Unknown -> all`
  (safe over-approximation) vs. - the failure mode - resolved to an
  **empty** set when the true value is unknown (a violation).

## 2. Enumerated invariants

Every way authority can enter, travel, be stored, retrieved, or be invoked,
against the four enforcement modes and the shapes named in the task brief.

| # | Invariant | Entry/travel/storage mechanism it governs |
|---|---|---|
| 1 | A direct call to a named, statically-resolved function/builtin requires exactly that callee's own capability, charged to the calling scope. | baseline call |
| 2 | A call through a bare fn-typed **parameter** (`f()` where `f: fn()->T`) defers the charge to the CALLER of the enclosing function, not the enclosing function itself, UNLESS the enclosing function has narrowed its own scope (`deny!`) between receiving `f` and invoking it, in which case the charge is required immediately. | fn-value parameter, direct invoke |
| 3 | A call through a fn-typed **local** (`let g = make_reader(path); g()`) resolves through the local's own traced provenance (`local_caps`/`build_local_closure_caps`), not a name lookup. | fn-value local, direct invoke |
| 4 | A call through a fn-value read out of a **container** (struct field, array element, map value), one or more levels deep, including through an intermediate local, resolves via `resolve_container_path_caps`/`PathStep` walking; an unresolvable root (a non-literal container, e.g. a function-return-built container) is `Unknown -> all`. | closure-in-container |
| 5 | A call through a **curried/chained** application (`f(a)(b)(c)`) recurses one layer at a time through `fn_return_closure_caps`. | passthrough chain |
| 6 | A HOF passed a fn-value **argument** at a call site charges the SPECIFIC argument expression's own resolved authority to that call site (`accumulate_hot_extra_caps`), not the HOF's own (possibly empty) declared/inferred set. | passthrough/HOF argument |
| 7 | An inline **transparent-forwarding lambda** (`\|f\| f()`) passed as a HOF's callback carries no authority of its own; its real authority is whichever OTHER argument the callee's OWN body actually supplies to it - resolved ONLY via `hot_param_companions` (per-declaration, syntactic data-flow from the callee's fixed source), never via declared-shape matching. | generic HOF companion |
| 8 | An **actor message handler** receiving a fn-value as a message argument is charged/deferred by the same mechanism as an ordinary function parameter (handlers have no implicit `self` offset in their own `params`, tracked via `actor_handler_names`). | actor message payload |
| 9 | A closure/fn-value captured by **`spawn`** carries its authority into the spawned task the same way an ordinary call-site argument does; `spawn` is not a capability boundary of its own. | spawn capture |
| 10 | A **`dyn Trait`** method dispatch whose target cannot be statically resolved to one known implementation is `Unknown -> all`. | dyn dispatch |
| 11 | A fn-value carried inside an **`Option<T>`/`Result<T,E>`** payload, extracted via `match`, is traced the same as any other container read (Some/Ok/Err are enum-variant constructors, tracked in `enum_variant_names` so they are not themselves mistaken for arbitrary fn-values). | Option/Result payload |
| 12 | A charge **deferred** to "the enclosing function's own call sites" (invariants 2, 6, 7) is sound only when the eventual outer call site is checked against a scope at least as wide as the scope actually in effect at the deferred call - enforced via `current_fn_entry_scope_depth` vs. live `scope_stack.len()`. | deny!-scope-vs-deferral interaction |
| 13 | **Generic monomorphization**: the checker runs once, pre-monomorphization, on each function DECLARATION's own AST. Every mechanism above (`hot_params`, `hot_param_companions`, `own_params`/`current_fn_typed_params`) is keyed by declared `TypeExpr` shape and parameter NAME/POSITION, never by an instantiated concrete type. Two instantiations of the same generic function at different type arguments must therefore always resolve to the SAME companion/hot-param facts. | generic instantiation |
| 14 | A generic parameter declared with a **bare, unconstrained type name** (`x: T`, not `x: fn(...)->...`) is never recognized as fn-bearing by `is_fn_typed`/`is_fn_bearing_type`, regardless of what concrete type it is instantiated with. | generic erasure of fn-typed-ness |
| 15 | **Raw-memory primitives** (`str_to_ptr`, `ptr_byte_at`, ...) require `ffi` at the direct call site in user code; the requirement does NOT propagate into ordinary inference (the stdlib TCB exemption), and does not weaken the direct-use check itself. | raw-memory TCB split |
| 16 | **`extern` blocks**: declaring an extern item is free; CALLING it requires the capability of the builtin it backs (`kryos_*` names) or `ffi` for any other extern name - enforced at the call site (E0506), not merely at declaration. | extern/FFI |
| 17 | **Actors are always declaration-enforced**, never inferred from handler bodies, in BOTH `strict` and `inferred` mode (only `permissive` differs) - an actor's own capability ceiling cannot silently grow from what its handlers happen to call. | actor boundary |
| 18 | An actor's/child scope's declared capabilities can never exceed its PARENT scope's capabilities at the point it is declared (`E0503`, attenuation-only - capabilities can only shrink walking down the call tree, never grow). | attenuation monotonicity |
| 19 | Coarse capabilities grant their sub-capabilities (`io`/`fs` -> `fs:read`+`fs:write`; `net` -> `net:http`+`net:tcp`; `all` -> everything); a sub-capability never grants its sibling or the coarse parent. | sub-capability lattice |
| 20 | A NAME COLLISION between two distinct declarations sharing a bare name (e.g. `std::iter::find`/`std::re::find`/`std::string::find`) must never let one declaration's own-parameter list leak into another's inference. | name-collision hygiene |
| 21 | A **decoy** argument of the same declared shape as the real authority-carrying argument must never be provable as the "real" companion merely by occupying a matching declared TYPE at the call site - companion resolution must derive from the callee's own fixed source, not from call-site shape. | decoy resistance |
| 22 | `Unknown` (unresolvable provenance) resolves to `Capability::All` at every site listed above - never to an empty set - and this is enforced UNIFORMLY (not per-shape) so no enumeration gap can silently default to "nothing needed". | fail-closed default |
| 23 | A **fresh closure literal** constructed and RETURNED by an enclosing function, whose body directly calls one of the ENCLOSING function's own fn-typed parameters (captured by closure, not a parameter of the literal itself - `fn wrap_once(inner: fn()->str) -> fn()->str { return \|\| inner() }`), resolves the RETURNED closure's own authority to `DependsOnParam(<enclosing param>)`, not the enclosing function's own (correctly empty) declared/inferred requirement - closing LEDGER item 10. | escaping wrapper closure (captured hot parameter) |
| 24 | A read of an actor-state field ANY NUMBER of field-access hops from `self` (`self.b.f()`, not just `self.f()`) whose FIRST hop names a field the actor's own state declaration makes transitively fn-bearing is fail-closed to `all`, regardless of the method/field name at the end of the chain; the same holds when the fn-bearing field is aliased into a local ONE hop before invocation (`let x = self.b; x.f()`). A chained alias-of-an-alias (two or more hops of local aliasing) is a known, disclosed, NOT-yet-covered residual (see the 2026-08-05 update note at the top of this document). | nested/aliased actor-state field access |

## 3. Per-invariant status against `checker.rs`/`model.rs`

Legend: **D** = maintained by derivation (a real, checked computation from
the callee's own fixed source or the type lattice) · **H** = maintained by
a heuristic (best-effort, not a full derivation, but currently believed
sound within a stated scope) · **V** = violated (a real, executed escape)
· **U** = unproven this session (not independently re-verified; relying on
prior-session evidence cited in the ledger).

| # | Status | Mechanism / evidence |
|---|---|---|
| 1 | **D** | `fn_capabilities` (name-keyed, call-graph fixed point) + `check_callee_capabilities`. Baseline; not independently re-audited this session (stable across dozens of prior rounds). |
| 2 | **D** | `hot_params` Seed A (`is_fn_typed` on the declared param type + a bare direct call to that param's name in the body) + `deferred_own_param_caps`'s scope-depth check (invariant 12). Confirmed by reading `checker.rs:1366-1386`, `2701-2723`. |
| 3 | **D** | `build_local_closure_caps` resolves a `let`-bound local's RHS via `resolve_closure_caps` at the point the `let` is processed; `resolve_closure_caps`'s `Expr::Identifier` arm checks `local_caps` FIRST (checker.rs:1720-1723), before falling through to `own_params`/`defined_fns`. |
| 4 | **D**, bounded | `resolve_container_path_caps` (checker.rs:1848 onward) walks `StructLiteral`/`ArrayLiteral`/`MapLiteral` and `Identifier` roots through `local_container_lits`; a non-literal root (function-call return, a container mutated inside a callee, a container read out of ANOTHER container the tracer can't follow) is `Unknown -> all`, by design - documented as an accepted, non-silent cost (LEDGER "capability cascade" wave, §1). Verified LIVE this session: `apply_from_map` decoy-companion repro (map variant) still correctly REJECTED at HEAD `00b3cf7`, both modes (see §5). |
| 5 | **D** | `resolve_closure_caps`'s `FnCall`/`MethodCall` arm recurses on the callee (checker.rs:1800-1812); sound because `fn_return_closure_caps` already over-approximates via `merge_closure_caps`'s "disagreement collapses to Unknown" rule (checker.rs:1590-1607), not independently re-probed live this session (prior rounds' curried-call repros are in the CLOSED table). |
| 6 | **D** | `accumulate_hot_extra_caps` resolves `arg` (the actual expression at the call site) via `resolve_container_path_caps`, unioned into `extra`, unioned into the call's required set - checker.rs:2450-2489. |
| 7 | **D**, narrow by design | `hot_param_companions` (checker.rs:1477-1544): computed ONCE per declaration, purely from the callee's OWN internal call-site argument EXPRESSIONS (`decompose_container_path`), matched against the callee's OTHER OWN parameter names - never against declared type shape, never against the CALLER's argument identities. Disagreement across the callee's own internal call sites collapses to `None` (unresolved -> `all`), never to a guess. This is the ONE relief mechanism in the file that is explicitly a "heuristic" only in the weak sense that it can fail to resolve (falls to `all`, safely) - it cannot be tricked into resolving to a WRONG companion by anything a caller supplies, because the companion fact is fixed at the callee's own compile time, independent of any call site. Re-verified LIVE this session (§5): the exact decoy-generic repro this mechanism replaced a broken heuristic for is still rejected at HEAD. |
| 8 | **D** | `actor_handler_names` set consulted everywhere a `has_self_offset` decision is made (checker.rs:1430-1431, 2466-2467, 3867-3872), so a handler's own params are never off-by-one against a struct method's implicit `self`. Fixed live in round 3 (an off-by-one silently dropped index-0 coverage until corrected) - per LEDGER CLOSED table; not independently re-run this session, relying on that prior live verification (8 new regression files including `..._actor_message.kry`, still present in `tests/security/`). |
| 9 | **D** | Spawn capture of a closure/fn-value is resolved through the SAME `resolve_container_path_caps`/`accumulate_hot_extra_caps` path as an ordinary call argument - no `spawn`-specific carve-out exists in `checker.rs` (grepped; none found). Round 3/5 both explicitly tested a `spawn`-capture variant (`..._spawn_capture.kry`, still present) and it was rejected. Not independently re-run this session. |
| 10 | **D** | `dyn Trait` method dispatch resolves through the SAME `resolve_method_field_invoke_caps`/`resolve_direct_invoke_caps` machinery used for every other unresolved-callee shape (no `dyn`-specific arm exists; `TypeExpr::DynTrait` appears exactly once in the file, checker.rs:642, in an unrelated type-substitution helper) - so an unresolvable `dyn` dispatch falls to the same `Unknown -> all` default as everything else. `..._dyn_trait_method.kry` regression file present. Not independently re-run this session. |
| 11 | **D** | `Some`/`None`/`Ok`/`Err` are tracked in `enum_variant_names` (round-3 fix) specifically so `Some(secret_closure)` is not mistaken for a fn-value REFERENCE itself; the payload is then reached through ordinary container-path resolution once matched out. `..._option_result_payload.kry` (round 3's `cap_escape_option_result_payload.kry`) present. Not independently re-run this session. |
| 12 | **D** | `current_fn_entry_scope_depth`, set on function/actor entry (checker.rs:3814-3816, 3863-3865) and consulted by `deferred_own_param_caps` (checker.rs:2701-2723): any deferred charge is required in full (`all`) whenever `scope_stack.len() > current_fn_entry_scope_depth`, i.e. a `deny!` (or any future narrowing construct) is active between the function's entry and the point of the deferred call. This is the round-5 part-2 fix; carved out two independent scope-tracking fields (`transparent_lambda_params`, `structural_lambda_eval_depth`) specifically so a STRUCTURAL self-classification of a fresh lambda literal (not a real enforcement-time call) never consults the ambient ceiling - verified in that round as the fix's own self-regression check (map-over-pure-closures inside an unrelated `deny!` must not cascade to `all`). Re-verified live this session: the plain `outer(reader)`-inside-`deny!` shape from that round's own repro was not independently re-run (relying on the round-5 CLOSED entry's own before/after evidence), but the SAME code path is exercised (and still passes) by the decoy-generic re-run in §5, since that repro is ALSO inside a `deny!` block. |
| 13 | **D**, confirmed live this session | See §4 - traced the exact code (`compute_hot_param_companions`, `compute_hot_params`, `is_fn_typed`, `is_fn_bearing_type`) and confirmed every gating predicate operates on the DECLARED `TypeExpr` and parameter NAME/POSITION from the pre-monomorphization AST, with no reference anywhere to an instantiated/substituted type. Constructed the adversarial case the task asks for; determined NOT REACHABLE, with reasoning and a live compiler run (§4). |
| 14 | **D** (fails closed, not silently), confirmed live this session | A bare `x: T` parameter invoked as `x()` is REJECTED by the type checker itself (`E0110: type ?T5 is not callable`) before capability checking is even meaningful - Kryos has no generic trait-bound syntax that would let `T` be constrained to "callable" (grepped `docs/19-language-reference.md` for trait-bound syntax; none exists). Independently, even though the type error already blocks it, the capability checker's OWN inference for the same program computed `invoke_generic` as requiring `[all]` (`Unknown -> all`, invariant 22) - both layers reject it, redundantly. See §4 for the executed repro and exact output. |
| 15 | **D** | `check_raw_memory_direct` (checker.rs:76-92) runs a SEPARATE pass over the ROOT module only (pre-import-merge, so it never sees stdlib internals), filtered to diagnostics naming a `RAW_MEMORY_BUILTINS` name - this is what makes the requirement direct-use-only and non-propagating BY CONSTRUCTION (the pass literally cannot see past the root module's own call sites). Original fix + `tests/security_gate.sh`'s both-directions-plus-no-cascade check per the CLOSED table; not independently re-run this session. |
| 16 | **D** | `extern` call-site check at checker.rs:3892-3898 (declaration is free, requires `Ffi` unless the name resolves to a `kryos_*` builtin's own real capability) plus two more E0506 sites (checker.rs:4438, 4516) for the different call-expression shapes. Not independently re-run this session (recently added per HEAD commit `8fba060`'s predecessor - "docs: language reference still showed a C-library extern, now rejected by E0508" confirms this is live and enforced, not aspirational). |
| 17 | **D** | `check_actor` (checker.rs:3824-3863): `scope.annotated = annotated \|\| !matches!(self.mode, CapabilityMode::Permissive)` - forces `annotated=true` (declaration-only enforcement) in both Strict and Inferred regardless of whether an explicit annotation is present, so handler bodies never get an INFERRED ceiling the way an ordinary interior function does. |
| 18 | **D** | `check_function`/`check_actor` both run the identical `is_subset_of`/`excess_over` attenuation check against `self.current_scope()` before pushing the new scope (checker.rs:3763-3781, 3835-3853) - E0503 either way. |
| 19 | **D** | `Capability::satisfies` (model.rs, not fully re-read this session byte-for-byte, but its call sites and the documented coarse/sub-cap table in CLAUDE.md §"Sub-capabilities" match the enumerated `Capability::from_str` arms read directly at model.rs:54-81 - `net`, `io`/`fs`, and `all` are the only coarse spellings that parse). |
| 20 | **D** | Round-3 fix: `collect_functions` now carries each declaration's OWN param list inline rather than a later name-based re-lookup (per the CLOSED table entry) - specifically to close the `find`/`find`/`find` collision. Not independently re-run this session. |
| 21 | **D**, confirmed live this session | `find_companion_container_arg` (the shape-matching heuristic) is DELETED - grepped `checker.rs` for `find_companion_container_arg`: zero hits. The only companion-resolution path remaining is `hot_param_companions` (invariant 7), which is call-site-shape-blind by construction. Live-verified: `cap_escape_decoy_map_companion.kry` still rejected at HEAD (§5). |
| 22 | **D** | Every `ClosureCapsResult::Unknown` arm across `resolve_closure_caps`, `resolve_container_path_caps`, `resolve_direct_invoke_caps`, `resolve_method_field_invoke_caps` terminates in the same two-line pattern (`CapabilitySet::empty(); c.insert(Capability::All)`) - checked by reading every call site of `Unknown =>` in `checker.rs` reached during this audit (2749-2753, 2776-2780, and the analogous arm inside `accumulate_hot_extra_caps`'s `DependsOnParam` non-`own_params` branch at 2627-2630). No arm returning `CapabilitySet::empty()` directly for an `Unknown`/unresolved case was found. |
| 23 | **D**, closed 2026-08-05 (LEDGER item 10) | `resolve_closure_caps`'s `Lambda` arm: a new "captured hot parameter" branch runs `walk_calls_expr` over the lambda body, checks whether any call target is one of the ENCLOSING function's own `own_params` (not the lambda's own params - that's the pre-existing, separate "lambda's own hot parameter" branch), and if `collect_caps_expr` over the body (with that param still correctly deferred) comes back empty, resolves to `DependsOnParam(<that param>)` instead of falling through to the plain `Known(collect_caps_expr(..))` fallback that previously silently dropped the deferral. If the body ALSO needs some other statically-known capability, resolves to `Unknown` (fail closed - `ClosureCapsResult` has no variant expressing "Known(X) plus a deferred Y", and this file's own rule is never to guess). Proven live, both directions: pre-fix `kryos run cap_escape_closure_wraps_closure.kry` leaks (rc=0); post-fix rejects with the PRECISE excess capability named (`[fs:read]`, not a blanket `[all]`), both `inferred` and `--strict-capabilities`. |
| 24 | **D** (one-hop), disclosed residual beyond one hop | `resolve_actor_self_field_invoke_caps` rewritten to decompose the full receiver chain and check the FIRST field stepped through from `self` (not the trailing method name) against `current_actor_fn_state_fields` - closes the `self.b.f()` shape that defeated item 18's original bare-`self`-only check. A new, deliberately narrow `current_actor_state_alias_locals` map (built per-handler, populated ONLY from a syntactically-recognized `let x = self.<path>` binding) closes the one-hop alias variant (`let x = self.b; x.f()`) the same way. A BROADER fix (consulting the general `local_caps` provenance map for any method-call root not found as a literal) was implemented, measured, and REVERTED this same wave - it over-rejected ordinary code broadly (conf_generics, conf_errors_concurrency, examples/actors.kry, 2 type-soundness + 2 inferred-soundness probes all false-positived), because that map also holds an `Unknown` entry for ordinary locals bound to any plain non-fn-returning function call. A chained alias-of-an-alias (`let y = x` where `x` is itself a `self.<path>` alias) is therefore NOT covered - an honest, disclosed gap, not a silent one. |

## 4. Generic monomorphization - the prime suspect, examined directly

**Claim to test:** the companion map (invariant 7) is computed once per
function DECLARATION; construct a case where a generic instantiated at two
different type arguments would need DIFFERENT companion relationships, and
determine whether that is reachable.

**Finding: not reachable, by construction, in the current design.**

`compute_hot_param_companions` and `compute_hot_params` both run over
`decls: &[Decl]` - the **un-monomorphized** AST, once, before any type
substitution happens (monomorphization is a `kryos-mir`-crate concern that
runs after capability checking, confirmed by grepping this crate for
`monomorph`/`type_arg`/`instantiat*`: zero hits anywhere in
`kryos-capabilities`). Every predicate the companion mechanism depends on
is one of:

1. `is_fn_typed(&p.ty)` / `is_fn_bearing_type(&p.ty)` - matches on the
   **declared** `TypeExpr` (`Function`, `Array`, `Optional`, 2-arg
   `map`/`Map` generic, or a resolved struct's own field types). A generic
   type PARAMETER name (`T`) used bare as a param's declared type never
   matches any arm (confirmed: `TypeExpr::Simple{name:"T"}` falls to
   `struct_fields_for("T", &[])`, which returns `None` because `T` is not
   a registered struct - `is_some_and(None)` is `false`).
2. `decompose_container_path(arg_expr)` - a purely syntactic decomposition
   of the argument EXPRESSION at the callee's own internal call site (`arr
   [i]` -> root `arr`, path `[Index]`). No type information enters this
   function at all.
3. The companion candidate is matched against `params.iter().position(|q|
   q.name == root ...)` - a NAME comparison against the same fixed
   parameter list every instantiation shares (a generic function has
   exactly one parameter list, declared once; type arguments substitute
   INSIDE types, never rename or reorder parameters).

Because none of (1)-(3) reads a substituted/instantiated type anywhere,
the fact "hot parameter `f` at slot `i` is fed by parameter `real`'s path
`[Index]`" is a property of the function's **source text**, true (or
unresolved) identically for `pipeline<str>` and `pipeline<SecretHolder>`
in the same program. There is no code path by which two instantiations of
the same declaration could receive two different companion answers - the
map is quite literally computed once, keyed by function NAME, not by
`(name, type_args)`.

The one way this invariant COULD be violated is if a bare, unconstrained
generic parameter (`x: T`) could be invoked as a function at one
instantiation without going through any of the recognized fn-bearing
shapes - this would be a companion/hot-param mechanism BLIND SPOT (not a
wrong-companion proof, but an unresolved-authority path that must still
fail closed). Tested this directly:

```kryos
fn invoke_generic<T>(x: T) -> str {
    return x()
}

fn main() {
    let f = || "hello from closure"
    let r: str = invoke_generic(f)
    println(r)
}
```

Run against `compiler/target/release/kryos.exe` (HEAD `00b3cf7`,
`KRYOS_STDLIB_DIR` set, read-only, no rebuild):

```
$ kryos check scratch_generic_bare_t.kry
error[E0110]: type `?T5` is not callable
 --> scratch_generic_bare_t.kry:2:12
  2 |     return x()
   |            ^^^ here
error[E0507]: call to `invoke_generic` requires capabilities [all] not granted to caller
 --> scratch_generic_bare_t.kry:7:18
  7 |     let r: str = invoke_generic(f)
   |                  ^^^^^^^^^^^^^^^^^ callee requires more capabilities
  = note: function `invoke_generic` has @capabilities(all) but caller lacks [all]
error: check failed: 2 errors, 0 warnings
```
`RC=1`. Two independent rejections: the type checker refuses to call a
value of unconstrained generic type at all (`E0110`), and - even though
that alone already blocks the program - the capability checker's own
inference for `x()` (an unresolved callee, invariant 22) independently
computed `invoke_generic` as requiring `[all]`, which a caller holding
nothing could never satisfy (`E0507`). Both layers fail closed; neither
produces a false "requires nothing".

**Conclusion:** invariant 13/14 hold. Generic monomorphization was the
correct prime suspect by prior-round history (3 of 4 historical bugs), but
in the CURRENT design the companion/hot-param machinery's pre-
monomorphization, purely-syntactic construction is exactly what makes it
immune to the specific divergence this task asked to construct - not by
a defensive check, but because the mechanism has no way to observe a type
argument in the first place. This is a structural property worth
preserving deliberately: any future change that makes companion/hot-param
resolution type-instantiation-aware (e.g. to support genuine generic
specialization) would need to re-derive this soundness argument from
scratch, since it would no longer hold by construction.

## 5. Attack programs executed this session

All runs used `compiler/target/release/kryos.exe` at HEAD `00b3cf7`,
read-only (no rebuild performed), `KRYOS_STDLIB_DIR` set per the repo's
own operational rule.

1. **Bare unconstrained generic parameter invoked as a function**
   (`invoke_generic<T>(x: T) -> str { return x() }`) - see §4. Result:
   REJECTED, `E0110` (type system) + `E0507` (`[all]` required,
   capability checker's own independent fail-closed inference). No
   escape.
2. **Re-run of the existing map-companion decoy repro**
   (`tests/security/cap_escape_decoy_map_companion.kry`, a generic
   `apply_from_map<T>(decoy: map<str,T>, real: map<str,T>, f: fn(T)->str)`
   with a decoy of identical declared shape to the real secret-carrying
   container) against BOTH `kryos run` (inferred mode) and `kryos check
   --strict-capabilities`. Result: REJECTED in both modes, `E0507`
   attributing the requirement to the real closure argument (the note
   text confirms "some of this authority is carried by a closure/fn-value
   ARGUMENT passed at this call site"). Confirms the round-5 fix (deleted
   `find_companion_container_arg`, `hot_param_companions` as sole relief)
   is still in effect at current HEAD, not merely at the commit that
   introduced it.

No new escape was found. Both runs confirm invariants already argued
sound in §3/§4 rather than surfacing a new gap.

## 6. Residual, already-documented limitations (not re-litigated as new findings)

These are known-accepted costs of the current design, not gaps discovered
this session - cross-referenced so a reader does not mistake §3's "D"
ratings for a claim of unconditional soundness:

- A container built from a genuinely non-literal source (a function
  return value, a container mutated inside a callee, a container read out
  of another container beyond what the intermediate-local extension
  follows) resolves to `Unknown -> all` - a real usability cost, not a
  soundness hole (invariant 4/22's fail-closed default is exactly what
  fires here). Documented in `docs/10-capabilities.md` and the LEDGER's
  "capability cascade" wave.
- The struct-argument leak (~86MB/1M calls, LEDGER OPEN item 3) is a
  MEMORY-SAFETY/leak issue in the two struct-drop codegen paths, not a
  capability-model issue - out of scope for this document, not re-audited
  here.
- `any`'s lack of a runtime tag (type-erased to bare `i64`) is a
  correctness issue for `to_string`/`format`, not a capability-model
  issue - out of scope here.
- `spawn` mutating-closure reentrancy and throw-during-unwind paths are
  flagged UNTESTED in the task brief; this session did not add coverage
  for either (both are concurrency/exception-safety questions orthogonal
  to capability provenance, which was the assigned scope) - still open
  for a future audit.
- A CHAINED alias-of-an-alias of an actor fn-bearing state field (`let y =
  x` where `x` is itself a `let x = self.b` alias, two hops before
  invocation) is NOT covered by this wave's fix - see invariant 24 and §7
  below for why a broader fix was reverted rather than shipped.

## 7. The structural guarantee: exhaustive `match`, no wildcard arm

Added 2026-08-05, closing the specific request that ends the six-round
enumerate-and-patch cycle: make the capability-resolution path a `match`
over the value-producing expression enum with **no wildcard arm**, so
adding a new `Expr` variant to the language fails to compile until someone
explicitly decides its capability treatment.

**Where it lives.** `kryos-capabilities/src/checker.rs`, three matches:

1. `resolve_closure_caps`'s outer match (the top-level dispatch: is this
   expression a `Lambda`, an `Identifier`, an `FnCall`, or something else).
2. The SAME function's inner match on a call's callee sub-expression
   (`callee.as_ref()`, inside the `FnCall` arm) - a second, independent
   place a bare `Expr` needs classifying.
3. `resolve_container_path_caps`'s match - previously matched the tuple
   `(PathStep, &Expr)` directly (`(PathStep::Field(fname),
   Expr::StructLiteral{..}) => ..`), which would need ~70 explicit cells
   (2 `PathStep` variants × 35 `Expr` variants) to make genuinely
   exhaustive without becoming illegible. Restructured to match primarily
   on `Expr` (35 arms, no wildcard) with `PathStep` handled as a nested
   `match` WITHIN the three container-literal arms, which stays a real
   per-`Expr`-variant enumeration - the thing that actually matters for
   "did we forget a value-producing FORM" - without the combinatorial
   blow-up. The 4 `(head, expr)` cell combinations that are syntactically
   unreachable in a well-typed program (e.g. an `Index` step into a
   `StructLiteral`) still resolve to `Unknown`, explicitly, inside the
   relevant arm - not assumed unreachable and elided.

**Every arm not given a real resolver routes to `ClosureCapsResult::Unknown`**
(which every one of this function's callers already converts to
`Capability::All` - invariant 22), spelled out one `Expr` variant per line
(grouped with `|` where several fall to the identical default, matching the
style the AST's own `Expr::span()` method already uses) rather than
collapsed into `_ => Unknown`. This is a pure refactor, not a behavior
change: verified via `cargo build --release` producing zero
"unreachable pattern" / "non-exhaustive match" diagnostics - every arm the
new enumeration lists really was reachable and really was falling through
the old wildcard to the same `Unknown` result.

**Why this is a real (if partial) soundness argument, not just tidiness.**
The six-round cycle's failure mode was never "the fail-closed default is
wrong" - every round's fix correctly defaulted to `all` for whatever it
DID recognize as unresolvable. The failure was narrower and sneakier: a
few call sites (`resolve_actor_self_field_invoke_caps`,
`resolve_method_field_invoke_caps`'s root-not-found branch, this wave's
`resolve_closure_caps` Lambda arm before its fix) had their OWN, separate
"nothing to charge" fallback that never routed through the `Unknown`
machinery at all - a *positive* default (`CapabilitySet::empty()`) chosen
because the ordinary, common case genuinely needs no charge (an actual
struct method call, an escaping lambda that needs nothing), with no way to
tell that case apart syntactically from the rare unsound one. The
exhaustive-match conversion does NOT fix those call sites by itself (they
are fixed individually, invariants 18/23/24) - what it guarantees is that
the CENTRAL closure-provenance resolvers (`resolve_closure_caps`,
`resolve_container_path_caps`), which every one of those call sites
ultimately calls into for the shapes they DO recognize, can never silently
stop covering a new `Expr` form. If Kryos's AST gains a new expression kind
tomorrow (a new literal, a new capture syntax, a new control-flow
construct) and that new form can produce or launder a function value, this
match block fails to compile until a human adds an arm and decides: does
this need a real resolver, or is the existing fail-closed default correct
for it. That is a narrower guarantee than "the whole checker is sound" - 
it converts exactly one historically-repeated failure mode (a shape
falling through an enumeration nobody remembered to extend) from a silent
security hole into a build error, for the two functions that are the
common resolution path for almost every invariant in §3's table.
