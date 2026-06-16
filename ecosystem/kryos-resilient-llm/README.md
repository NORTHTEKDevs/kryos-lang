# kryos-resilient-llm

A **certified retry envelope** for LLM calls.

`resilient_chat` is an ordinary chat client wrapped in three governance axes
that no shipping language can put in one signature:

| Axis | Mechanism | What it guarantees |
| --- | --- | --- |
| `@capabilities(net)` | compile-time capability checker | the body provably touches the network and **nothing else** — a stray `file_write`/`exec` is a build error (`E0505`) |
| `@budget(tokens, calls)` | MIR-injected runtime frame | the budget **spans the whole backoff loop**; a retry series **cannot exceed** the ceiling — exhaustion *throws* |
| `Tracked<Probable<str>>` | `std::tracked` + `std::probable` | the answer carries its **provenance** (every attempt, failure, backoff, cost) and a **retry-decayed confidence** |

It fuses the resilience primitives that already existed in the stdlib but had
never been combined — `std::circuit` (a CLOSED/OPEN/HALF_OPEN breaker) and
`std::backoff` (exponential delays) — with the governance axes
(`@capabilities`, `@budget`, `std::cost`, `std::tracked`, `std::probable`).

LangChain/Tenacity-style retry wrappers express *none* of this in the type
system: the network reach is implicit, the spend ceiling is a counter the loop
can forget to check, and the answer is a bare string with no lineage. Here all
three are properties of the function's signature, checked before the program
runs.

## The signature

```kryos
@capabilities(net)
@budget(tokens = 120000, calls = 12)
fn resilient_chat(cfg: LlmConfig, msgs: [Message], breaker: [i64], max_retries: i64) -> Tracked<Probable<str>>
```

- `resilient_chat` — returns the provenance-tracked, confidence-scored answer.
- `resilient_chat_metered` — same envelope, returns the full `ResilientOutcome`
  (the `ComputeCost`, attempt/failure/gated counters, and breaker state
  *alongside* the tracked answer).

Both build the live transport as a closure `|m| chat(cfg, m)` that inherits the
`net` scope and charges the `@budget` frame on every attempt.

## How a call flows

For each slot up to `max_retries + 1`:

1. **Breaker gate** (`std::circuit::allow`) — if the breaker is OPEN and not yet
   cooled, skip the transport, back off, and advance a logical clock toward
   HALF_OPEN (counted as a `gated` slot).
2. **Attempt** the injected transport. On success: record it on the breaker,
   charge `ComputeCost` (tokens + 1 api call), seal the answer.
3. **On a retryable error**: trip the breaker (`record_failure`), decay
   confidence (`× 0.6` per retry), compute the next exponential delay
   (`std::backoff::next_delay`), accumulate it as `wall_time_ms`, and retry.
4. **On `@budget` exhaustion**: do **not** retry — the budget is the hard
   ceiling, so the throw propagates straight out of the envelope.

Every stage appends a `LineageEntry` to the `Tracked` value, and the final
accumulated `ComputeCost` is folded into the lineage as a `cost` entry, so the
cost travels *with* the answer's provenance.

## Run it

```bash
KRYOS=path/to/kryos.exe

# offline demo (flaky mock provider — no creds, no network)
$KRYOS run ecosystem/kryos-resilient-llm/demo.kry

# the test suite (offline)
$KRYOS test --path ecosystem/kryos-resilient-llm
```

Live use points the same envelope at a real provider:

```kryos
use std::llm::{anthropic_config, system, user}
use lib::{resilient_chat, resilient_breaker}

let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
let breaker = resilient_breaker(3, 2000000000)   // open after 3 fails, 2s cooldown
let answer = resilient_chat(cfg, [user("Summarize Kryos in one line.")], breaker, 4)
```

## Compile-fail fixture

`fixtures/leaky_io.kry` is the negative half of the `@capabilities(net)`
promise: a net-only function with a stray `file_write`. It **must not compile**.
It lives under `fixtures/` (not `tests/`) precisely so `kryos test` does not try
to compile a file that is supposed to fail.

```bash
$ kryos check fixtures/leaky_io.kry
error[E0505]: builtin `file_write` requires `io` capability
 --> fixtures/leaky_io.kry:34:5
   |
34 |     file_write("/tmp/leaked_answer.txt", r.text)
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ requires `io`
   = note: add `@capabilities(io)` to the enclosing function or actor
error: check failed: 1 error, 0 warnings   # exit code 1
```

No `--strict-capabilities` flag is needed: the function is already annotated, so
the checker enforces its declared ceiling. Adding `io` to the annotation would
make it pass — which is exactly the point. A net-only envelope cannot reach the
filesystem, proven before the program runs.

## Test evidence (actual output)

```
$ kryos test --path ecosystem/kryos-resilient-llm
  PASS test_budget (10.5ms)
  PASS test_resilient (9.7ms)
Tests: 2 passed, 0 failed, 0 skipped, 2 total

running 8 @test functions
  PASS test_call_budget_frame_spans_retry_loop (0.1ms)
  PASS test_token_budget_frame_is_a_ceiling (0.0ms)
  PASS test_generous_budget_completes (0.0ms)
  PASS test_happy_path_first_try (0.0ms)
  PASS test_total_failure_opens_breaker (0.0ms)
  PASS test_flaky_recovers_through_half_open (0.0ms)
  PASS test_provenance_lineage_complete (0.0ms)
  PASS test_cost_line_format (0.0ms)
Tests: 8 passed, 0 failed, 0 skipped, 8 total
```

The tests run **offline**: the transport is an injected mock that performs the
same `@budget` dance as `std::llm::chat` (reserve a call, then charge tokens),
so the budget frame is exercised for real without a provider.

## PRE-BUILD VERIFY — does the `@budget` frame really span the retry loop?

**Yes. Verified, not assumed.** This was the spec's explicit honesty check, and
it is the headline claim, so it was proven before anything was built:

- `compiler/crates/kryos-mir/src/lower.rs` injects `kryos_budget_push(N, M)` at
  function **entry** and `kryos_budget_pop_to(depth)` before **every** return.
  The frame is therefore live for the *entire dynamic extent* of the annotated
  function — including loops in functions it calls.
- `compiler/crates/kryos-rt/src/budget.rs` keeps frames in a thread-local stack;
  `kryos_budget_try_call` decrements **every** active frame (the tighter wins).
- `test_budget.kry::test_call_budget_frame_spans_retry_loop` proves it
  end-to-end: a `@budget(calls = 2)` wrapper drives a 5-retry series with an
  always-failing transport; the **3rd** attempt throws
  `llm error: @budget exhausted: no model calls left`. A plain loop counter
  could be forgotten; this throw is a language property.

The token axis is also a ceiling (`test_token_budget_frame_is_a_ceiling`): a
40-token budget shared across two envelope calls throws on the second.

## Honest limitations / unknowns

- **money & energy cost axes are `0.0` (unimplemented).** Per-token pricing
  varies by provider/model, so `ComputeCost.money_usd` / `energy_kwh` are never
  charged. Only **tokens, api_calls, and wall_time (backoff)** are tracked.
  `cost_line` prints `usd=0 energy=0` to make this explicit.
- **No real `sleep` between retries.** Sleeping needs the `time` capability,
  which would widen the strictly-`net` envelope. The backoff *schedule* is
  computed (`std::backoff::next_delay`) and accounted as `wall_time_ms`, and a
  **logical clock** is advanced by those delays so the breaker's OPEN→HALF_OPEN
  cooldown is deterministic and testable. A production deployment that wants to
  actually block can do the sleep inside its injected transport (or add `time`
  to the cap set). This is a deliberate trade to keep the headline
  `@capabilities(net)` honest.
- **Confidence is a heuristic.** `Probable<str>.confidence` starts at `1.0` and
  multiplies by `0.6` per retry (and is `0.0` on total failure). It expresses
  "how much the instability we observed should lower trust in this answer," not
  a calibrated model probability. `std::probable` ships `CalibrationTracker`/
  `ece` if you want to measure it against ground truth.
- **`std::tracked`'s `explain`/`to_json` do not render `Tracked<Probable<str>>`
  well.** They stringify the inner value via `"{...}"` interpolation, which
  renders a struct as a raw pointer. This library reads the lineage/value fields
  directly (`outcome_explain`) instead — field access and lineage walking on the
  nested generic both work correctly on the JIT.
- **Single-threaded breaker.** Per the spec MVP, the breaker state is a plain
  `[i64]`; a multi-threaded shared breaker (`Shared<T>`) is out of scope.
- **Backend: tested on the Cranelift JIT** (`kryos run` / `kryos test`). The
  nested-aggregate field in `ResilientOutcome` is declared last to keep AOT
  field offsets correct (gotcha 21), but the AOT path was not exercised here.

## Files

```
src/lib.kry            the resilience engine + the @capabilities(net) @budget envelope
demo.kry               offline demo (flaky mock): recovery, outage, budget ceiling
tests/test_resilient.kry  engine tests: happy path, breaker open, recovery, provenance, cost
tests/test_budget.kry     the @budget frame spans the retry loop (calls + tokens axes)
fixtures/leaky_io.kry  COMPILE-FAIL fixture: file_write under @capabilities(net) -> E0505
```

License: Apache-2.0.
