# Kryos Unlock Analysis: provenance-tracked
## Cluster: std.tracked -- First-class data lineage

**Source verified:** `compiler/stdlib/tracked.kry`, `docs/stdlib/tracked.md`, `docs/14-ai-runtime.md`

---

## What Actually Exists (confirmed in source)

`Tracked<T>` is a generic struct wrapping any value with a `[LineageEntry]` array and a named source:

```kryos
struct Tracked<T> {
    value: T,
    lineage: [LineageEntry],
    source: str,
    source_description: str
}
```

Operations (all free functions, not methods -- generic `impl` blocks not yet supported):

- `tracked_source<T>(value, source, description)` -- create with origin stamp
- `transform<T>(t, new_value, operation, description)` -- append a transformation step
- `inference<T>(t, model, result, confidence)` -- record an AI inference step (model name + confidence in metadata)
- `annotate<T>(t, operation, description)` -- add a note without changing the value (approval gates, reviews)
- `explain<T>(t) -> str` -- human-readable lineage chain
- `to_json<T>(t) -> str` -- JSON-serialized lineage using native json_* builtins (properly escaped)

Each `LineageEntry` records: `operation`, `description`, `timestamp` (i64 via `time_now()`), `metadata` (str, used to carry confidence score in inference entries).

### What is NOT in the current implementation

- No auto-propagation: lineage is NOT carried through standard operators (`+`, `*`, field access). You must call `transform()` manually at each step. The type system does not enforce lineage propagation.
- No taint tracking: there is no static analysis to detect when a `Tracked` value is passed into a function that discards the wrapper. The lineage is only preserved if the programmer explicitly threads it.
- No capability fusion: `Tracked<T>` and the capability system (`@capabilities`) are independent; there is no mechanism to record which capability was used at each lineage step.
- No sub-lineage merging: if two `Tracked<T>` values are combined (e.g. a JOIN of two data sources), there is no merge-lineage primitive that unions both chains.
- No schema/type tag on lineage entries: the `operation` and `description` fields are freeform strings, not typed enums; tooling cannot pattern-match on lineage operations without string comparison.

---

## Novelty Assessment

### 1. Language-integrated provenance type (PARTIAL novelty)

**Who else does it:** Python has `great_expectations`, `whylogs`, `lineapy` (a research system that patches Python's AST to auto-capture lineage), and Apache Atlas/OpenLineage for pipeline-level tracking. Rust has no mainstream lineage library. JavaScript has nothing at the type level. dbt tracks column lineage in SQL via its DAG model.

**Why a library is not the same as a language type:** In Python, you annotate a DataFrame and rely on the library patching calls. The annotation is not part of the type signature -- a function can receive a `pd.DataFrame` and return a plain `pd.DataFrame`, losing lineage silently. In Kryos, `Tracked<T>` IS the type. A function that takes `Tracked<str>` and returns `str` is VISIBLY discarding the lineage in its signature. This is a real, checkable difference: you get compile-time visibility that provenance is dropped, without needing runtime instrumentation.

**Honest caveat:** This visibility is nominal today. The compiler does not warn or error when you unwrap a `Tracked<T>` and pass the bare `T` to something. The discipline is by convention, not enforcement. Full enforcement would require effect-system-style rules: "a function touching a `Tracked<T>` must propagate it or explicitly discard it." That is not implemented.

**Rating: PARTIAL** -- cleaner than a library (type-visible), weaker than a true effect system (not enforced).

### 2. Native AI inference step in the lineage model (PARTIAL novelty)

The `inference<T>(t, model, result, confidence)` primitive is notable: it records model name and confidence alongside the lineage timestamp as a first-class operation kind. No mainstream lineage tool (OpenLineage, dbt, Marquez) models AI inference as a named lineage step. Those tools track SQL transforms and table reads, not "model X with 0.94 confidence returned value Y."

**Who else does it:** MLflow and W&B track experiments, not per-value inference provenance. They are run-level logs, not value-level lineage. Seldon's metadata tracking is API-external, not language-integrated.

**Rating: PARTIAL** -- the concept is right and differentiated, but the implementation is a string in metadata, not a typed inference record. A production-grade version would carry model version, provider, prompt hash, and link to the `@budget` frame that governed the call.

### 3. JSON export for compliance handoff (HYPE)

`to_json()` produces a JSON string of the lineage chain. This is useful but not differentiated -- every logging library does this. The Kryos version does properly use the native `json_*` builtins for escaping (the comment in source notes the old hand-built JSON did not escape, which was a bug now fixed). That is a correctness improvement, not a novelty claim.

**Rating: HYPE** (as a standalone feature).

---

## Unlocks for Real Use Cases

### EU AI Act Article 13 + 12 compliance (HIGH SIGNAL, PARTIAL buildable today)

The EU AI Act requires "high-risk AI systems" to log inputs, outputs, and decision logic in a retrievable audit trail. `Tracked<T>` gives you a per-value audit log that travels with the data. You can call `to_json()` at any decision boundary and persist it. This is more granular than run-level MLflow logs: you have the lineage of the specific piece of data that produced a specific output.

**What you can build today:** A Kryos agent that wraps user inputs as `tracked_source(user_data, "user_upload", "...")`, chains inference steps through an LLM via `std.llm.chat()`, calls `inference()` on each response, and writes `to_json()` to an audit log before acting. The audit log satisfies the "keep records" requirement.

**What needs language work:** The EU AI Act also requires traceability of WHICH data points influenced a decision. For RAG-based agents, that means tracking which retrieved chunks contributed to the response. Kryos needs a merge-lineage primitive (`tracked_merge([sources], new_value, operation, description)`) to union two `Tracked<T>` chains. That does not exist yet; you can approximate it by including source identifiers in the description string, which is workable but imprecise.

### Prompt injection forensics (PARTIAL buildable today)

When an agent processes external input and produces an action, an attacker can inject instructions in the data. With `Tracked<T>`, every piece of content that touched the prompt can be logged. You tag the raw external input at ingestion time: `tracked_source(external_content, "web_fetch", "untrusted external URL")`. When the LLM response produces an action, the lineage chain shows whether "untrusted external URL" content was in the path.

**What you can build today:** Wrap all external data sources at the trust boundary, thread `Tracked<str>` through the prompt-building function, and assert in the action-dispatch layer that the lineage of the triggering input does not contain an "untrusted" source. This is a manual discipline that the type system makes visible but does not enforce.

**What needs language work:** Proper taint tracking requires the compiler to propagate a taint flag (or capability) through operations and refuse to pass tainted data into sensitive sinks without an explicit declassification step. That is a separate compile-time analysis not currently in Kryos.

### RAG citation and grounding (BUILDABLE TODAY, PARTIAL novelty)

In a RAG pipeline, each retrieved chunk can be wrapped in `tracked_source(chunk, "vector_db", "doc_id=" + id)`. After the LLM call, `inference()` records what model ran. The output's `lineage` array is the citation list: every source document is an entry. You call `explain()` to produce a human-readable citation, or `to_json()` to build a structured citation response.

This is genuinely useful and requires no language work. The missing piece is that there is no deduplicated "which sources contributed to this answer" view -- if the same chunk appears in multiple lineage entries (because it was transformed and re-used), you get duplicate entries. A `distinct_sources(t)` utility function would help but is trivially writable in Kryos today.

**Who else does it:** LangChain has `Document` objects with `metadata` that carry source info, but the metadata is a plain dict and is NOT automatically propagated through chains. You can lose it. Kryos's approach keeps provenance in the type, which is an ergonomic and correctness improvement.

### Reproducibility for model outputs (PARTIAL buildable today)

If every data transformation and inference step is logged in `lineage`, you can replay the pipeline by re-executing the recorded operations with the same inputs. This is the core of reproducibility. The current implementation records operations as free-text descriptions, not executable nodes, so you cannot literally replay. But the logged information (model name, confidence, operation name, timestamp, source) is enough to re-run manually or to detect when re-running gives a different result.

---

## Proposed Kryos Functions

### 1. `tracked_merge` -- union two lineage chains

```kryos
fn tracked_merge<T>(
    sources: [Tracked<T>],
    new_value: T,
    operation: str,
    description: str
) -> Tracked<T>
```

Combines the lineage arrays of multiple `Tracked<T>` inputs into a single chain and records a merge step. Essential for RAG (multiple retrieved chunks -> single answer) and JOIN operations. Buildable today in Kryos as a stdlib function; no compiler work needed.

### 2. `tracked_taint_check` -- assert no untrusted source in lineage

```kryos
fn tracked_taint_check<T>(t: Tracked<T>, banned_source_prefix: str) -> bool
```

Returns `false` (or throws) if any lineage entry's metadata or source contains `banned_source_prefix`. This is the simplest usable approximation of taint detection: check before acting. Buildable today as a stdlib function.

### 3. `tracked_filter_by_op` -- extract lineage entries by operation type

```kryos
fn tracked_filter_by_op<T>(t: Tracked<T>, operation: str) -> [LineageEntry]
```

Returns only the lineage entries matching a given operation name (e.g. `"inference"`, `"source"`). Enables "show me all the models that touched this value" or "show me all untrusted sources." Buildable today.

### 4. `tracked_cost` -- fuse Tracked<T> with ComputeCost

```kryos
fn tracked_cost<T>(t: Tracked<T>, cost: ComputeCost, description: str) -> Tracked<T>
```

Appends a cost entry to the lineage, recording the `ComputeCost` (tokens, USD, latency) associated with a step. Bridges `std.tracked` and `std.cost`. Essential for billing-grade audit trails: "this output cost $0.003 and 1,500 tokens at inference step 2." Buildable today; the `metadata` field could carry `to_string(cost)`.

### 5. `tracked_to_citation` -- render lineage as a citation list

```kryos
fn tracked_to_citation<T>(t: Tracked<T>) -> [str]
```

Returns a deduplicated list of source strings from all "source" and "inference" lineage entries. Clean API for RAG citation surfaces: `let citations = tracked_to_citation(answer)`. Buildable today as a stdlib function.

---

## What Needs Language Work to Become Truly Novel

1. **Auto-propagation through operators:** The big gap is that `tracked_value.value + other_string` silently drops the lineage. A truly novel system would propagate `Tracked<T>` through `+`, `*`, and field access automatically, the way Rust's `?` propagates `Result`. This requires compiler support to overload operators on `Tracked<T>`. Without it, every programmer must remember to call `transform()` at every step, which they will forget.

2. **Enforcement at function boundaries:** A `@requires_lineage` attribute that the compiler checks -- if you pass a `Tracked<T>` into a function and it returns a bare `T`, that is a compile error unless you explicitly call `tracked_unwrap()`. This is the difference between "nice to have" and "enforcement-grade compliance."

3. **Taint tracking as a capability:** The cleanest future state: untrusted external sources (`net`, `env`) produce `Tracked<T>` values automatically when the capability system is in deny-by-default mode, and the lineage carries a taint flag that must be cleared before those values reach sensitive sinks (`io`, `process`). This fuses the capability system with provenance tracking and is genuinely novel -- no mainstream language does this. Requires deny-by-default capabilities (not yet in Kryos) plus capability-lineage fusion (not yet designed).

4. **Typed operation enum:** Replace freeform `operation: str` with a typed enum (`Source | Transform | Inference | Annotation | Merge | CostCharge`) so tooling can pattern-match on lineage without string comparison. This is a breaking API change but a small compiler addition.

---

## Honesty Summary

`Tracked<T>` is real, implemented, and works. The core insight -- provenance as a type, not a side-channel log -- is sound and differentiated from library-level approaches. The implementation is a good foundation: it covers the main operations (source, transform, inference, annotate), exports JSON correctly, and is generic over T.

The gap is enforcement. Everything works by convention today. The unlocks around EU AI Act compliance, prompt injection forensics, and RAG citation are BUILDABLE TODAY but require programmer discipline, not compiler guarantees. The truly novel capabilities (auto-propagating lineage through operators, compile-time taint enforcement, capability-provenance fusion) require language work that is on the roadmap but not implemented.

The sweet spot for near-term positioning: "Kryos is the only language where your audit trail is a first-class type, not a log statement you might forget." That claim is honest as of today. The follow-up -- "and the compiler enforces it" -- is the 6-12 month language work that turns PARTIAL into TRULY-NOVEL.
