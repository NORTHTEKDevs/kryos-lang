# Stage 2 — capability-row enforcement

The plan for closing the remaining capability escapes. Supersedes the
"finish it in `kryos-capabilities`" note in `ESCAPE-ROUTING.md`, which was
**wrong about where the work goes** (see the dependency fact below).

## Correction: where stage 2 lives

`kryos-types` **depends on** `kryos-capabilities`, not the reverse
(`kryos-types/Cargo.toml`). So `kryos-capabilities` can never read `CapRow` —
that is a circular dependency. Enforcement cannot move into the crate that
currently does the shape-matching.

Stage 2 therefore lands in **`kryos-types/src/check.rs`**, where the rows already
live and where `891c406` already put its hooks. `kryos-capabilities` keeps only
the builtin-name → capability table, which `kryos-types` already calls across the
bridge (`accumulate_builtin_call`).

## Why this closes the escapes that shape-matching cannot

Every open escape is a callee whose *syntactic shape* the matcher does not
recognise. A type-directed check never asks what shape the call expression is —
it asks what the callee's TYPE carries. `pair.1()`, `(*b).f()`,
`get_box(h).f()`, `x.0()` inside a `for`, and an actor message parameter all have
a function type with a capability row. The row is the same regardless of how the
value was spelled.

It also retires `decompose_container_path`, `literal_field_exists`,
`resolve_container_path_caps` and `hot_params` — the four-site apparatus that
every one of these escapes came from.

## Why it does NOT recreate the `all` cascade

Measured 2026-08-12: annotating dispatchers `@capabilities(all)` fails, because
`all` propagates to every caller of `std::http`/`std::agent`.

Rows do not have that problem, and this is the whole reason to do it. A handler
parameter's row is an **open row variable**, not `Unknown`:

- `use_tool`'s row is `{own bits} ∪ ρ_handler` — it does not need `all`, it needs
  "whatever the tool I was handed carries";
- at a call site passing a concrete tool, `ρ_handler` **binds** to that tool's
  actual row (`instantiate_row` / `bind_cap_var` already do this);
- a caller passing only pure tools charges nothing.

`CapRow::Unknown` must stay reserved for genuinely unresolvable provenance (it
erases to `CapBits::ALL`). If stage 2 starts producing `Unknown` for ordinary HOF
parameters, that is the cascade returning and the design is being applied wrong.

## Primitives that already exist (stage 1, `891c406`)

| Piece | Where |
| --- | --- |
| `CapRow` (`Resolved { concrete, vars }` / `Unknown`), `union`, `is_subset_of`, `concrete_bits` | `kryos-types/src/ty.rs` |
| `cap_accum_stack`, `accumulate_caps`, `accumulate_builtin_call` | `kryos-types/src/check.rs` |
| `fresh_cap_var`, `bind_cap_var`, `resolve_cap_row`, `instantiate_row` | `kryos-types/src/infer.rs` |
| `own_cap_var`, `generic_cap_var_ids` on every `FunctionSig` | `kryos-types/src/env.rs` |
| `cap_effects_log` + `dump_fn_effects_report` (reporting only, no diagnostics) | `kryos-types/src/check.rs` |

Stage 1 computes and reports. **It never emits a diagnostic.** That is the gap.

## The work

1. **Declared-scope row.** Turn the enclosing function's `@capabilities(...)` into
   a closed `CapRow`, pushed alongside `cap_accum_stack`. Unannotated `main` under
   inferred mode is the deny-by-default boundary that already exists.
2. **Call-site check.** At every call, resolve the callee's row
   (`resolve_cap_row` after `instantiate_row`) and require
   `row.is_subset_of(declared)`. Emit `E0507` with the existing wording when it is
   not. This is the one genuinely new behaviour.
3. **Bind at argument positions.** Passing a fn-value into a fn-typed parameter
   binds that parameter's row var to the argument's row. Mostly wired already —
   verify it survives a value being stored in a struct field and read back
   (`self.tools[i].handler`), which is where shape-matching died.
4. **Retire the shape-matcher** only once every `tests/security/attack_*.kry`
   repro is rejected by the row check alone, proven by disabling the old path and
   re-running `tools/loop/escape_status.sh`.

## Guardrails, learned the hard way

- `ir_signatures` is the false-positive canary. It caught `5 |> padd(10)`
  demanding `all`, and it caught the `Reference`-unwrap cascade. Run it before any
  full gate run — it is fast and it fails first.
- `conf_stdlib_wave14` is the cascade detector: it exercises `std::agent`'s
  dispatcher. If it goes red, `all` is propagating and the row design is being
  applied wrong.
- Do not edit the checker from reasoning. Four attempts have now failed that way
  (Borrow/Deref alone, TupleLiteral, param seeding alone, dispatcher annotation).
  The one that worked took a single measurement. Probe first.
- Item 37 has a real, re-appliable mechanical fix (a `Reference` arm in
  `resolve_type_path_inner`, plus param seeding excluding `TypeExpr::Function`).
  Land it WITH stage 2 — alone it triggers the cascade.

## Progress log — stage 2 as actually built (2026-08-12)

Escapes **12 -> 2** this session. Three fixes, each the same shape: a dispatch
surface that computed a capability row and then dropped it. None were subtle logic
errors; all were wiring stage 1 left unfinished, and all were invisible because the
failure is SILENT (enforcement runs, finds an empty row, passes).

| commit | what was dropped | closed |
| --- | --- | --- |
| `0a5dbbd` | `deny!` blocks inferred a row and discarded it; `dump_fn_effects_report` had no caller anywhere | items 30a-d, 37 |
| `848a9d4` | method/handler dispatch computed `cap_var_map` and bound it to `_` | item 34 |
| `c29b15b` | impl method bodies had no accumulator frame at all, so `own_cap_var` never bound | item 35 |

**ORDER IS LOAD-BEARING AND FAILS SILENTLY.** The callee's row must be charged AFTER
argument unification. A callee that is row-polymorphic in a fn-typed parameter carries
a row mentioning that parameter's var, and the var only binds when the argument is
unified against it. Charging first resolves a still-open row and charges nothing —
the call is checked and costs zero. Both dispatch sites had this bug.

## The last two (items 32, 33) — why they are qualitatively harder

Measured minimally. A struct field in actor state **works**
(`main = {fs:read}`); a TUPLE field in actor state does not (`main = {?C10}`).

The difference is where the binding lands:

- **struct field**: `Box`'s field row var is DECLARATION-GLOBAL, so `Box { f: r }` in
  `main` binds it directly to a concrete row. Field-insensitive, but it resolves.
- **tuple in actor state**: `self.pair = (0, f)` builds a NEW tuple type from the
  handler's own parameter, so unification binds the field var to `stash`'s ORIGINAL
  param var. At the call site `instantiate_sig` FRESHENS that var, so the concrete
  argument binds the fresh copy and the original — the one the state field points at
  — is never bound to anything concrete.

**Do NOT "fix" this by also binding the original param var.** That is the cascade in
a new costume: passing one privileged closure to `std::iter::map` would bind `map`'s
declaration-global param var, and then EVERY `map` call in every program charges that
authority. This is the same failure that killed the annotate-dispatchers approach on
2026-08-12, and `conf_stdlib_wave14` would catch it.

The sound answer is the one the capabilities crate already takes for
`self.<field>()` (`resolve_actor_self_field_invoke_caps`): **a fn-bearing ACTOR STATE
field is genuinely untraceable** — any handler may write it at any prior dispatch —
so reading one should yield `CapRow::Unknown` (which erases to `ALL`), not the
declaration's var. `Type::with_caps_erased_to_unknown()` already exists and is used
for `dyn`/trait dispatch.

Blocker for implementing it: **`kryos-types/src/check.rs` has no actor context at
all** — no `current_actor`, no state-field set. That has to be added (recorded around
the `Decl::Actor` handler loop, cleared after) before the stamp can be applied at a
`self.<field>` read. Item 33 (a closure passed actor-to-actor as a message parameter)
is a DIFFERENT shape again — not a state read — and needs cross-actor parameter row
flow, so it is not covered by the same stamp.

Expect over-rejection when the stamp lands: an actor storing a PURE closure in state
would start requiring `all`. That is the fail-closed stance the design has already
chosen elsewhere, but it should be measured against `examples`/`strict_caps` before
being called done.
