# 02 -- Kryos Governed-Agent stdlib Extension

**Pitch:** Five bridge functions that wire `std.tracked`, `std.cost`, `std.probable`, and `std.llm` into a coherent governance layer -- per-value cost receipts, RAG citation merge, budget introspection, and confidence filtering -- all in ~150 lines of pure Kryos stdlib code, no compiler changes required.

---

## Why This Exists

The four standalone modules are individually useful today:

- `std.tracked` records data lineage through `Tracked<T>` and its `lineage: [LineageEntry]`.
- `std.cost` tracks spend through `ComputeCost` and enforces limits with `Budget`/`CostTracker`.
- `std.probable` propagates uncertainty through `Probable<T>` with a `confidence: f64`.
- `std.llm` calls models and exposes actual token counts in `LlmResponse`; `chat_within` already charges a `Budget` after each call.

The gap is that they do not talk to each other. An agent that calls `chat_within` gets back a `BudgetedChat { response, budget }`. It cannot ask:

1. "What did *this specific value* cost to produce?" -- `ComputeCost` is not attached to the value, only tracked globally.
2. "Which sources contributed to this answer?" -- RAG pipelines merge multiple `Tracked<str>` chunks but there is no `tracked_merge` that collapses lineages and deduplicates sources.
3. "How much budget is left right now inside this `@budget` frame?" -- the `kryos_budget_remaining_tokens` extern exists in `llm.kry` but is not exposed as a stdlib function a user script can call.
4. "Which candidates in this ensemble exceed my confidence threshold?" -- `best_of` picks the single winner; `filter_confident` does not exist.

These five functions are the missing bridges. They require no new language features, no new compiler passes, and no additional runtime hooks beyond what already exist.

---

## Novelty Rating

**PARTIAL** -- honest assessment, function by function.

| Function | Rating | Who else does it |
|---|---|---|
| `tracked_cost(t, cost, desc)` | PARTIAL | OpenTelemetry traces carry cost-like spans; LangChain callbacks capture token usage. Neither is a language-level value you can inspect in the same expression as the value it describes. The Kryos version is a first-class struct field, not a side-channel log. |
| `tracked_merge(sources, new_val)` | PARTIAL | LangChain's `Document` objects carry `metadata`; LlamaIndex nodes carry `node_id`. Both are Python objects with no type safety; merging them requires manual iteration. Kryos merges typed `Tracked<T>` in a generic function and deduplicates sources at the language level. |
| `tracked_to_citation(t)` | PARTIAL | Every RAG framework builds citation lists in application code. Kryos surfaces this as a one-liner from the lineage already on the value. |
| `budget_remaining()` | TRULY-NOVEL | There is no mainstream language where the current call-budget frame is introspectable as a first-class language call. Rust effects crates are compile-time; wasm fuel limits are not readable from inside the running function. The `kryos_budget_remaining_tokens` extern already exists in the runtime but is not user-callable. |
| `filter_confident(ensemble, threshold)` | PARTIAL | Python type-ignore, numpy boolean indexing. Kryos is typed and the threshold is applied to a `[Probable<T>]` at the language level. |

The honest wedge is not any single function -- it is that all five work on values that are *already governance-typed* by the language itself. You cannot accidentally omit them because the type system forces you to use `Tracked<T>` and `Probable<T>`.

---

## Kryos Primitives Used (all confirmed in source)

All of the following are real, confirmed from source:

**From `compiler/stdlib/tracked.kry`:**
- `struct Tracked<T>` with fields `value: T`, `lineage: [LineageEntry]`, `source: str`, `source_description: str`
- `struct LineageEntry` with `operation: str`, `description: str`, `timestamp: i64`, `metadata: str`
- `tracked_source<T>(value, source, description) -> Tracked<T>` -- constructor
- `transform<T>(t, new_value, operation, description) -> Tracked<T>` -- append lineage entry
- `annotate<T>(t, operation, description) -> Tracked<T>` -- note without value change
- `_append<T>(t, new_value, entry) -> Tracked<T>` -- internal helper; new functions can use this pattern directly (it is not `pub`-gated, just prefixed `_` by convention; stdlib files compile together)

**From `compiler/stdlib/cost.kry`:**
- `struct ComputeCost` with `wall_time_ms: f64`, `tokens_used: i64`, `api_calls: i64`, `money_usd: f64`, `energy_kwh: f64`
- `cost_zero() -> ComputeCost`
- `cost_add(a, b) -> ComputeCost`
- `struct Budget` with `remaining_tokens()`, `remaining_api_calls()`, `remaining_usd()` methods

**From `compiler/stdlib/probable.kry`:**
- `struct Probable<T>` with `value: T`, `confidence: f64`, `source: str`
- `probable<T>(value, confidence) -> Probable<T>` -- constructor (clamps to [0,1])
- `is_confident<T>(p, threshold) -> bool`

**From `compiler/stdlib/llm.kry`:**
- `struct LlmResponse` with `input_tokens: i64`, `output_tokens: i64`, `model: str`
- `struct BudgetedChat` with `response: LlmResponse`, `budget: Budget`
- `extern fn kryos_budget_remaining_tokens() -> i64` -- already wired in runtime
- `extern fn kryos_budget_remaining_calls() -> i64` -- already wired in runtime
- `extern fn kryos_budget_active() -> i64` -- checks if a `@budget` frame is active

**No language work needed first.** Generic `impl` blocks are not yet supported, but these are all free generic functions (the existing pattern in tracked.kry, probable.kry). The `_append` internal in tracked.kry can be copied rather than called (it is an internal helper); the new functions follow the same pattern.

---

## Architecture

### New file: `compiler/stdlib/agent_bridge.kry`

All five functions live in one new file imported as `std::agent_bridge`. This avoids touching existing stdlib files (lower regression risk) while making the bridges discoverable under one import.

**Why one file not five:** the functions are coupled (tracked_cost calls tracked+cost, tracked_merge uses transform, tracked_to_citation uses lineage, budget_remaining uses the llm externs, filter_confident uses probable). A single file with one `use` block is cleaner than five cross-imports.

### Data model

No new structs are needed. The bridge functions return existing types:

- `tracked_cost` returns `Tracked<T>` (the cost is recorded in the lineage metadata, not a new field)
- `tracked_merge` returns `Tracked<T>`
- `tracked_to_citation` returns `[str]`
- `budget_remaining` returns `(i64, i64)` -- `(tokens_left, calls_left)` as a tuple
- `filter_confident` returns `[Probable<T>]`

### Function signatures (with real Kryos syntax)

```kryos
use std::tracked::{Tracked, LineageEntry, transform, tracked_source}
use std::cost::{ComputeCost, cost_to_string}
use std::probable::{Probable, is_confident}

// Attach a ComputeCost receipt to a Tracked value.
// The cost is serialized into the lineage metadata field so it survives
// to_json export without a new struct.
fn tracked_cost<T>(t: Tracked<T>, cost: ComputeCost, description: str) -> Tracked<T> {
    let meta = "tokens=" + to_string(cost.tokens_used) +
               " calls=" + to_string(cost.api_calls) +
               " usd=" + to_string(cost.money_usd) +
               " ms=" + to_string(cost.wall_time_ms)
    let entry = LineageEntry {
        operation: "cost",
        description: description,
        timestamp: time_now(),
        metadata: meta
    }
    let mut new_lineage = t.lineage
    new_lineage = push(new_lineage, entry)
    return Tracked {
        value: t.value,
        lineage: new_lineage,
        source: t.source,
        source_description: t.source_description
    }
}

// Merge N Tracked<str> source chunks into one new Tracked<T>.
// The merged lineage is all input lineages concatenated; sources are
// deduplicated (first occurrence wins, stable order).
fn tracked_merge<T>(sources: [Tracked<str>], new_val: T) -> Tracked<T> {
    let mut merged_lineage: [LineageEntry] = []
    let mut seen_sources: [str] = []
    let mut primary_source = ""
    let mut primary_desc = ""
    let mut i = 0
    while i < len(sources) {
        let s = sources[i]
        // Carry all lineage entries.
        let mut j = 0
        while j < len(s.lineage) {
            merged_lineage = push(merged_lineage, s.lineage[j])
            j = j + 1
        }
        // Deduplicate sources.
        let mut already = false
        let mut k = 0
        while k < len(seen_sources) {
            if seen_sources[k] == s.source {
                already = true
            }
            k = k + 1
        }
        if not already {
            seen_sources = push(seen_sources, s.source)
            if i == 0 {
                primary_source = s.source
                primary_desc = s.source_description
            }
        }
        i = i + 1
    }
    // Record the merge itself.
    let merge_entry = LineageEntry {
        operation: "merge",
        description: "merged " + to_string(len(sources)) + " sources",
        timestamp: time_now(),
        metadata: "source_count=" + to_string(len(seen_sources))
    }
    merged_lineage = push(merged_lineage, merge_entry)
    return Tracked {
        value: new_val,
        lineage: merged_lineage,
        source: primary_source,
        source_description: primary_desc
    }
}

// Extract a deduplicated list of source identifiers from a Tracked value.
// Useful for building footnotes / audit citations from a RAG answer.
fn tracked_to_citation<T>(t: Tracked<T>) -> [str] {
    let mut sources: [str] = []
    if t.source != "" {
        sources = push(sources, t.source)
    }
    let mut i = 0
    while i < len(t.lineage) {
        let entry = t.lineage[i]
        if entry.operation == "source" {
            // metadata field is "source=<id>"; extract after "source="
            let prefix = "source="
            if contains(entry.metadata, prefix) {
                let src = substr(entry.metadata, len(prefix), len(entry.metadata))
                // Deduplicate.
                let mut found = false
                let mut j = 0
                while j < len(sources) {
                    if sources[j] == src {
                        found = true
                    }
                    j = j + 1
                }
                if not found {
                    sources = push(sources, src)
                }
            }
        }
        i = i + 1
    }
    return sources
}

// Return (tokens_remaining, calls_remaining) from the active @budget frame.
// Returns (-1, -1) when no @budget frame is active (safe to call anywhere).
// Uses the same externs that llm.chat() already calls internally.
extern {
    fn kryos_budget_active() -> i64
    fn kryos_budget_remaining_tokens() -> i64
    fn kryos_budget_remaining_calls() -> i64
}

fn budget_remaining() -> (i64, i64) {
    if kryos_budget_active() == 0 {
        return (-1, -1)
    }
    return (kryos_budget_remaining_tokens(), kryos_budget_remaining_calls())
}

// Filter an ensemble to only values meeting the confidence threshold.
// Returns an empty array (not an error) when nothing passes.
fn filter_confident<T>(ensemble: [Probable<T>], threshold: f64) -> [Probable<T>] {
    let mut out: [Probable<T>] = []
    for p in ensemble {
        if is_confident(p, threshold) {
            out = push(out, p)
        }
    }
    return out
}
```

### New test file: `tests/stdlib/test_agent_bridge.kry`

```kryos
use std::test::{expect_eq, expect_true}
use std::tracked::{tracked_source}
use std::cost::{ComputeCost}
use std::probable::{probable}
use std::agent_bridge::{
    tracked_cost,
    tracked_merge,
    tracked_to_citation,
    budget_remaining,
    filter_confident
}

fn test_tracked_cost_appends_lineage() {
    let t = tracked_source("hello", "doc-1", "user document")
    let cost = ComputeCost {
        wall_time_ms: 120.0,
        tokens_used: 500,
        api_calls: 1,
        money_usd: 0.002,
        energy_kwh: 0.0
    }
    let with_cost = tracked_cost(t, cost, "summarize call")
    // Last lineage entry must be operation="cost".
    let last = with_cost.lineage[len(with_cost.lineage) - 1]
    expect_eq(last.operation, "cost")
    expect_true(contains(last.metadata, "tokens=500"))
    expect_true(contains(last.metadata, "usd=0.002"))
}

fn test_tracked_merge_deduplicates_sources() {
    let a = tracked_source("chunk A", "doc-1", "intro")
    let b = tracked_source("chunk B", "doc-1", "body")   // same source
    let c = tracked_source("chunk C", "doc-2", "appendix")
    let merged: Tracked<str> = tracked_merge([a, b, c], "synthesized answer")
    let cites = tracked_to_citation(merged)
    // doc-1 appears once, doc-2 once.
    expect_eq(len(cites), 2)
    expect_eq(cites[0], "doc-1")
    expect_eq(cites[1], "doc-2")
}

fn test_tracked_to_citation_single_source() {
    let t = tracked_source("fact", "arxiv:2401.00001", "paper")
    let cites = tracked_to_citation(t)
    expect_eq(len(cites), 1)
    expect_eq(cites[0], "arxiv:2401.00001")
}

fn test_budget_remaining_no_frame() {
    // Outside any @budget frame, should return (-1, -1).
    let (toks, calls) = budget_remaining()
    expect_eq(toks, -1)
    expect_eq(calls, -1)
}

fn test_filter_confident_basic() {
    let ensemble = [
        probable("cat", 0.9),
        probable("dog", 0.4),
        probable("bird", 0.7),
        probable("fish", 0.2)
    ]
    let high = filter_confident(ensemble, 0.6)
    expect_eq(len(high), 2)
    expect_eq(high[0].value, "cat")
    expect_eq(high[1].value, "bird")
}

fn test_filter_confident_empty_result() {
    let ensemble = [probable("x", 0.1), probable("y", 0.2)]
    let out = filter_confident(ensemble, 0.9)
    expect_eq(len(out), 0)
}

fn main() {
    test_tracked_cost_appends_lineage()
    test_tracked_merge_deduplicates_sources()
    test_tracked_to_citation_single_source()
    test_budget_remaining_no_frame()
    test_filter_confident_basic()
    test_filter_confident_empty_result()
    println("all agent_bridge tests passed")
}
```

### Integration smoke test: `examples/agent_bridge_demo.kry`

This is the demo a fresh session can run to see all five bridges working together in a realistic RAG-plus-budget scenario.

```kryos
use std::tracked::{tracked_source, explain}
use std::cost::{ComputeCost}
use std::probable::{probable}
use std::agent_bridge::{
    tracked_cost,
    tracked_merge,
    tracked_to_citation,
    budget_remaining,
    filter_confident
}

// Simulate three RAG chunks arriving with provenance.
fn make_chunks() -> [Tracked<str>] {
    let c1 = tracked_source(
        "Kryos capabilities prevent unauthorized IO at compile time.",
        "docs/10-capabilities.md",
        "capability spec"
    )
    let c2 = tracked_source(
        "@budget(tokens=N) halts runaway agent loops at runtime.",
        "docs/11-budgets.md",
        "budget spec"
    )
    let c3 = tracked_source(
        "Tracked<T> records every transformation for audit.",
        "docs/10-capabilities.md",   // same source as c1 -- tests dedup
        "capability spec"
    )
    return [c1, c2, c3]
}

fn main() {
    // 1. Merge RAG chunks.
    let chunks = make_chunks()
    let answer_text = "Kryos enforces capability safety, budget limits, and data provenance."
    let answer: Tracked<str> = tracked_merge(chunks, answer_text)

    // 2. Attach a cost receipt (simulated -- no real LLM call needed for demo).
    let inference_cost = ComputeCost {
        wall_time_ms: 340.0,
        tokens_used: 1200,
        api_calls: 1,
        money_usd: 0.006,
        energy_kwh: 0.0
    }
    let final_answer = tracked_cost(answer, inference_cost, "summarize via claude-sonnet-4-6")

    // 3. Extract citations.
    let citations = tracked_to_citation(final_answer)
    println("Answer: " + final_answer.value)
    println("Citations (" + to_string(len(citations)) + "):")
    for cite in citations {
        println("  - " + cite)
    }

    // 4. Budget introspection (no active @budget frame in main, shows -1,-1).
    let (toks, calls) = budget_remaining()
    if toks == -1 {
        println("No active @budget frame.")
    } else {
        println("Budget remaining: " + to_string(toks) + " tokens, " + to_string(calls) + " calls")
    }

    // 5. Confidence filtering over a candidate ensemble.
    let candidates = [
        probable("Kryos", 0.95),
        probable("Rust", 0.62),
        probable("Go", 0.38),
        probable("Python", 0.20)
    ]
    let strong = filter_confident(candidates, 0.6)
    println("High-confidence candidates (" + to_string(len(strong)) + "):")
    for c in strong {
        println("  " + c.value + " @ " + to_string(c.confidence))
    }

    // 6. Full lineage.
    println("")
    println(explain(final_answer))
}
```

Expected output (no LLM key needed):

```
Answer: Kryos enforces capability safety, budget limits, and data provenance.
Citations (2):
  - docs/10-capabilities.md
  - docs/11-budgets.md
No active @budget frame.
High-confidence candidates (2):
  Kryos @ 0.95
  Rust @ 0.62

Value: Kryos enforces capability safety, budget limits, and data provenance.

Lineage:
  1. [source] capability spec
     source=docs/10-capabilities.md
  2. [source] budget spec
     source=docs/11-budgets.md
  3. [source] capability spec
     source=docs/10-capabilities.md
  4. [merge] merged 3 sources
     source_count=2
  5. [cost] summarize via claude-sonnet-4-6
     tokens=1200 calls=1 usd=0.006 ms=340.0
```

---

## MVP Scope vs Full Vision

### MVP (this project, ~150 lines of Kryos)

- `agent_bridge.kry` with all five functions as described above.
- Unit tests covering each function individually (6 tests, `tests/stdlib/test_agent_bridge.kry`).
- Integration demo (`examples/agent_bridge_demo.kry`) that runs without any API key.
- Module registered in the stdlib module resolver so `use std::agent_bridge::{...}` works.

What is explicitly out of scope for MVP:

- No changes to the compiler.
- No changes to existing stdlib files (tracked.kry, cost.kry, probable.kry, llm.kry).
- No new runtime hooks.
- No `@budget` frame creation in tests (the test for `budget_remaining` verifies the no-frame case only; an in-frame test requires running inside a `@budget`-annotated caller which would require a real LLM call or a mock).

### Full vision (future work, listed here for roadmap visibility)

- `tracked_probable<T>(t: Tracked<T>, confidence: f64) -> Tracked<Probable<T>>` -- merge provenance and uncertainty into one wrapper.
- `cost_breakdown(tracker: CostTracker) -> [str]` -- per-entry human summary.
- `agent_audit_export(agent: Agent, tracker: CostTracker) -> str` -- full JSON report combining `Agent.audit_trail` + `CostTracker.entries` + lineage from all tracked values the agent produced.
- `budget_remaining_usd() -> f64` -- USD axis; requires a new extern `kryos_budget_remaining_usd` in the runtime (not yet wired).
- Sub-capabilities on `tracked_to_citation` so the annotation surface can express `@capabilities(tracked:read)` rather than the current coarse `io` capability.

---

## Build Plan

A fresh Claude Code session (no existing context) can execute these steps in order.

### Step 1 -- Verify stdlib compiles as-is

```bash
kryos run compiler/stdlib/tracked.kry
kryos run compiler/stdlib/cost.kry
kryos run compiler/stdlib/probable.kry
```

All three should exit 0 (they are library files with no `main`; the compiler accepts them as module source without a main when run as `kryos check`). Use `kryos check` if `run` requires a main:

```bash
kryos check compiler/stdlib/tracked.kry
kryos check compiler/stdlib/cost.kry
kryos check compiler/stdlib/probable.kry
```

This confirms the base is clean before adding anything.

### Step 2 -- Create `compiler/stdlib/agent_bridge.kry`

Write the file exactly as shown in the Architecture section above. The extern block re-declares the three budget hooks already in llm.kry; this is safe because Kryos extern declarations are resolved at link time, not dedup-required at the source level (confirmed: the self-host compiler does this for builtins across multiple files).

```bash
kryos check compiler/stdlib/agent_bridge.kry
```

Fix any type errors before proceeding. Common traps:
- `let mut out: [Probable<T>] = []` -- the array literal needs the type annotation because the compiler cannot infer generic element type from an empty literal without a push yet.
- `(i64, i64)` return type -- tuple returns are supported on both backends.
- `contains(entry.metadata, prefix)` -- this is a builtin (confirmed in CLAUDE.md), no import needed.

### Step 3 -- Register in module resolver

Find where stdlib modules are registered. The self-host compiler will have a module resolution table that maps `"agent_bridge"` to the file path. Search:

```bash
kryos run tools/find-module-table.kry   # if it exists
```

Otherwise, grep the self-host source:

```bash
grep -r "agent_bridge\|\"tracked\"\|\"probable\"" compiler/self-host/
```

Add `"agent_bridge"` -> `"compiler/stdlib/agent_bridge.kry"` in the same location as the other stdlib registrations. If the resolver auto-discovers `*.kry` files in `compiler/stdlib/`, no registration step is needed (verify by checking whether `tracked` needs registration or is auto-discovered).

### Step 4 -- Write and run unit tests

Create `tests/stdlib/test_agent_bridge.kry` as shown in the Architecture section.

```bash
kryos run tests/stdlib/test_agent_bridge.kry
```

All six tests must pass. If `test_budget_remaining_no_frame` fails (returns non-(-1,-1) values), the extern is firing outside a frame -- check that `kryos_budget_active()` is returning 0 correctly in the no-frame case by temporarily adding a `println` before the return.

### Step 5 -- Write and run the integration demo

Create `examples/agent_bridge_demo.kry` as shown.

```bash
kryos run examples/agent_bridge_demo.kry
```

Verify output matches the expected output in the Architecture section:
- 2 citations (not 3 -- deduplication working)
- 5 lineage entries (3 source + 1 merge + 1 cost)
- 2 high-confidence candidates

### Step 6 -- Run full stdlib test suite

```bash
kryos test
```

All pre-existing tests must still pass. The new file adds no new test files to the count that could cause regressions -- it only adds `test_agent_bridge.kry`.

### Step 7 -- AOT verification

```bash
kryos build --release examples/agent_bridge_demo.kry
./agent_bridge_demo
```

Output must match `kryos run`. This confirms no Cranelift-vs-LLVM divergence in the new code. Known risk: tuple return `(i64, i64)` from `budget_remaining` -- if AOT miscompiles it, switch to a two-element `[i64]` array return as a workaround (see CLAUDE.md gotcha #5 -- tuple destructuring is confirmed working on both backends as of the fix).

---

## Success Criteria / Demo

The project is done when:

1. `kryos run tests/stdlib/test_agent_bridge.kry` prints `all agent_bridge tests passed` with exit 0.
2. `kryos run examples/agent_bridge_demo.kry` prints the expected output including exactly 2 citations and a 5-entry lineage.
3. `kryos build --release examples/agent_bridge_demo.kry && ./agent_bridge_demo` produces identical output to the JIT run.
4. `kryos test` exits 0 (no regressions in the existing suite).
5. `use std::agent_bridge::{tracked_cost, tracked_merge, tracked_to_citation, budget_remaining, filter_confident}` resolves without error in any new file.

The demo tells the Kryos story concisely: a RAG pipeline that produces an answer with citations baked into the value (not logged separately), with cost attached to the value, and filtered candidates that only surface high-confidence options -- all in native Kryos, no external framework.

---

## Risks and Honest Unknowns

**Risk 1: `_append` is internal to tracked.kry.**
The new `tracked_cost` and `tracked_merge` functions cannot call `_append` from agent_bridge.kry because it is prefixed `_` (module-private by convention). The spec works around this by inlining the push pattern (4 lines). If the Kryos module system enforces `_` privacy strictly, this is not a blocker -- we just inline. Verify by checking whether `_append` causes a compile error when imported; if so, the inline pattern (already shown in the code sketch) is the correct approach.

**Risk 2: Re-declaring externs that are already in llm.kry.**
The three `extern fn kryos_budget_*` declarations appear in both llm.kry and agent_bridge.kry. This is safe if the linker deduplicates extern symbols (standard behavior for FFI declarations). If the compiler errors on duplicate externs, move the budget_remaining function into llm.kry directly and re-export from agent_bridge.kry via a thin wrapper.

**Risk 3: Empty generic array literals.**
`let mut out: [Probable<T>] = []` inside a generic function. The type annotation should be sufficient to satisfy the checker, but if not, push a known-bad element then pop it as a workaround, or change the return to start from the input and filter in-place with an index. This is a known compiler limitation (noted in CLAUDE.md, gotcha section on untyped array of aggregates -- resolved as of the noted fix, but generic parametric arrays may still have edge cases).

**Risk 4: Module resolver discovery.**
If stdlib modules are not auto-discovered but must be explicitly registered, and the registration table is inside compiled self-host Kryos source (not a config file), adding `agent_bridge` requires re-stage-1 compiling the self-host. This is a 5-10 minute build, not a blocker, but a fresh session should budget for it. The `kryos` binary at `~/.local` may not auto-reload without a rebuild.

**Risk 5: `tracked_merge` generic parameter.**
`tracked_merge<T>(sources: [Tracked<str>], new_val: T) -> Tracked<T>` mixes concrete (`Tracked<str>` input) with generic (`T` output). This is legal in Kryos because the input array element type is concrete and the return type is independently parameterized. Confirm with `kryos check` at Step 2; if the checker rejects the mixed signature, specialize the function for `Tracked<str>` output only (`fn tracked_merge(sources: [Tracked<str>], new_val: str) -> Tracked<str>`) which covers the primary RAG use case with zero generic complexity.

**Unknown: `kryos_budget_remaining_tokens` behavior when frame is active but partially consumed.**
The extern exists (confirmed in llm.kry) and is called after each `chat()`. Its behavior when the frame is active but no tokens have been charged yet is not tested. The `budget_remaining` function assumes it returns the correct remaining count. A quick manual test with a `@budget(tokens=1000, calls=5)` annotated caller that calls `budget_remaining()` before any LLM call would confirm.
