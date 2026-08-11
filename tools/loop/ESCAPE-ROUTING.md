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

| item | repro | reaches | fail-open line | notes |
| --- | --- | --- | --- | --- |

_Empty on purpose._ Nothing has been measured per-item yet, so `escape-instrument`
is red and `escape-root` stays blocked behind it. That is the system working: the
fix is gated on the measurement, not on confidence.
