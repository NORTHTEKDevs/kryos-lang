# 04 - kryos-rag: RAG Pipeline with Built-in Citation Lineage

**One-line pitch:** Retrieval-augmented generation where every answer carries its source citations
as a first-class `Tracked<str>` value -- not post-processed metadata, not a parallel dict, not a
log statement you might forget.

---

## Why This Is Novel

**Honest novelty rating: PARTIAL**

### What already exists

LangChain's `Document` class carries a `metadata` dict that can hold source info. Llama-Index
tracks node IDs per retrieved chunk. Both systems attach metadata as a side-car dictionary on a
plain Python object. The dictionary is not part of any type signature; a function that takes a
`Document` and returns a `str` has visibly discarded the metadata in its return type, but Python
gives you no warning. The metadata dict is not propagated through `.run()`, chain chaining, or
prompt templates -- you must manually re-attach it to the output, which LangChain users routinely
forget or re-implement via `return_source_documents=True`.

OpenLineage and Apache Atlas track pipeline-level lineage (which table fed which table) but have
no concept of per-value, per-inference-step lineage. MLflow logs run-level metadata, not
value-level provenance.

### What Kryos makes different

In Kryos, the type of a value IS the contract. A RAG answer in this library has type
`Tracked<str>`. Any function that takes `Tracked<str>` and returns a bare `str` has VISIBLY
dropped the citations in its signature -- you see it without running the code. The lineage array
is allocated once, carried through retrieval, through the LLM call (via `inference()`), and
available via `tracked_to_citation()` at any downstream call site. There is no "remember to pass
`return_source_documents=True`."

`tracked_merge` (built in project 02) is the key operation: it unions the lineage arrays of N
retrieved chunks into a single chain before the LLM call, so the answer's lineage records every
chunk that contributed. This is the primitive LangChain lacks at the type level.

### What still needs language work

- The compiler does not warn when you unwrap `t.value` and pass the bare str onward. Citation
  drop is visible in signatures but not enforced as a compile error.
- Auto-propagation through `+` (string concatenation) does not exist: if you do `a.value + b`,
  the lineage of `a` is NOT automatically merged into the result. You must call `transform()`
  explicitly.
- Sub-capabilities (e.g. `fs:read` vs `fs:write`) are not implemented, so capability annotations
  on retrieval functions are coarse (`@capabilities(io)` for local file reads).

**Bottom line:** Kryos is the only language where "every RAG answer's citations are in the type"
is a true statement, not a framework convention. Enforcement is by programmer discipline today,
not by compiler rules. The 6-12 month path to TRULY-NOVEL is auto-propagation and a
`@requires_lineage` attribute that makes citation-drop a compile error.

---

## Kryos Primitives Used

All of these are confirmed present in `compiler/stdlib/` at the current commit.

### std.tracked (compiler/stdlib/tracked.kry)

Core types and functions:

```
Tracked<T>            -- generic struct: value T + [LineageEntry] + source + source_description
LineageEntry          -- operation: str, description: str, timestamp: i64, metadata: str
tracked_source<T>()   -- create a Tracked<T> from an original source (document ID, URL, etc.)
transform<T>()        -- record a transformation step (e.g. chunk extraction, embedding lookup)
inference<T>()        -- record an AI inference step: model name + confidence in metadata
annotate<T>()         -- add a note without changing value (e.g. "confidence threshold passed")
explain<T>()          -- human-readable lineage chain as str
to_json<T>()          -- JSON-serialized lineage for audit export
```

NOT in stdlib yet -- must be written in this project (see Architecture section):

```
tracked_merge<T>()         -- union N Tracked<T> lineage chains (built in project 02 or here)
tracked_to_citation<T>()   -- deduplicated [str] of source entries (built in project 02 or here)
```

Project 02 is listed as a dependency. If project 02 has not been built yet, these two functions
must be implemented in this project as standalone utilities. Their signatures and implementations
are shown in the Architecture section below.

### std.llm (compiler/stdlib/llm.kry)

```
LlmConfig             -- provider, model, api_key, max_tokens, temperature, timeout_ms
LlmResponse           -- text: str, input_tokens: i64, output_tokens: i64, model: str
anthropic_config()    -- build config for Claude
openai_config()       -- build config for OpenAI-compatible endpoints
chat()                -- @capabilities(net); budget-hook integrated; throws on non-200
complete()            -- single-prompt convenience wrapper around chat()
chat_within()         -- chat() with explicit std.cost Budget enforcement
user() / system()     -- Message constructors
```

### std.cost (compiler/stdlib/cost.kry)

```
ComputeCost           -- wall_time_ms, tokens_used, api_calls, money_usd, energy_kwh
cost_zero()           -- zero-cost sentinel
cost_add()            -- accumulate costs
Budget                -- max/spent for usd, tokens, api_calls; .charge() throws on exceed
```

### @budget attribute + runtime hooks

The `@budget(tokens=N, calls=M)` function attribute wires into `kryos_budget_active /
kryos_budget_try_call / kryos_budget_charge_tokens` (extern hooks in llm.kry). When a budget
frame is active, `chat()` pre-charges one call and post-charges actual token usage. Exceeding
the budget throws, halting a runaway loop. This applies directly to `rag_answer()` -- the answer
function can be decorated with `@budget` to cap per-query cost.

### @capabilities

`chat()` in std.llm is annotated `@capabilities(net)`. Any function that calls it must also
declare `net`. File-based chunk retrieval needs `@capabilities(io)`. The capability system is
opt-in per annotated function (unannotated functions are unconstrained at this compiler version).

---

## Architecture

### Data model

The three types that flow through the pipeline:

```kryos
// A retrieved document chunk: text + its origin stamped in the type.
// Type: Tracked<str>
// t.source   = doc_id (e.g. "doc:001")
// t.lineage  = [LineageEntry{operation="source", description="retrieved chunk N of M", ...}]

// The merged context sent to the LLM: all N chunks' lineages unioned.
// Type: Tracked<str>
// Built by tracked_merge([chunk1, chunk2, chunk3], combined_text, "merge", "RAG context")

// The answer: LLM output with an inference entry appended.
// Type: Tracked<str>
// Built by inference(merged_context, cfg.model, reply_text, 0.0)
// t.lineage = all source entries from chunks + the inference entry
// tracked_to_citation(answer) -> ["doc:001", "doc:002", "doc:003"]
```

### Key functions to write

#### tracked_merge (if not provided by project 02)

```kryos
fn tracked_merge(sources: [Tracked<str>], new_value: str, operation: str, description: str) -> Tracked<str> {
    let mut combined_lineage: [LineageEntry] = []
    for s in sources {
        for entry in s.lineage {
            combined_lineage = push(combined_lineage, entry)
        }
    }
    let merge_entry = LineageEntry {
        operation: operation,
        description: description,
        timestamp: time_now(),
        metadata: "merged=" + to_string(len(sources)) + " sources"
    }
    combined_lineage = push(combined_lineage, merge_entry)
    let first_source = sources[0]
    return Tracked {
        value: new_value,
        lineage: combined_lineage,
        source: first_source.source,
        source_description: description
    }
}
```

Note: no generic `impl` blocks in current Kryos; this is a free function. The `Tracked<str>`
specialization is concrete (not `Tracked<T>`) to avoid the generic-struct-field gotcha on AOT.
If project 02 already provides a generic `tracked_merge<T>`, use that import instead.

#### tracked_to_citation (if not provided by project 02)

```kryos
fn tracked_to_citation(t: Tracked<str>) -> [str] {
    let mut seen: [str] = []
    let mut result: [str] = []
    for entry in t.lineage {
        if entry.operation == "source" {
            let src = entry.metadata
            let mut found = false
            for s in seen {
                if s == src {
                    found = true
                }
            }
            if not found {
                seen = push(seen, src)
                result = push(result, src)
            }
        }
    }
    return result
}
```

Deduplicates by `entry.metadata` (which `tracked_source` sets to `"source=" + source`). Returns
a `[str]` of unique source IDs in first-encounter order.

#### rag_retrieve

```kryos
@capabilities(io)
fn rag_retrieve(query: str, chunks: [RagChunk]) -> [Tracked<str>] {
    // In the MVP, chunks is a pre-built list of (id, text) pairs.
    // A real implementation would call a vector DB here.
    let mut results: [Tracked<str>] = []
    let mut i = 0
    while i < len(chunks) {
        let c = chunks[i]
        // Simple keyword match for MVP; swap in cosine similarity later.
        if contains(c.text, query) {
            let t = tracked_source(c.text, c.id, "retrieved chunk for query: " + query)
            results = push(results, t)
        }
        i = i + 1
    }
    return results
}
```

#### rag_answer

```kryos
@capabilities(net)
@budget(tokens=4000, calls=1)
fn rag_answer(cfg: LlmConfig, tracked_chunks: [Tracked<str>], question: str) -> Tracked<str> {
    if len(tracked_chunks) == 0 {
        throw "rag error: no chunks retrieved; cannot answer"
    }

    // Build context string by concatenating chunk values.
    let mut context = ""
    for tc in tracked_chunks {
        context = context + tc.value + "\n\n"
    }

    // Merge all chunk lineages into one.
    let merged = tracked_merge(tracked_chunks, context, "merge", "RAG context from " + to_string(len(tracked_chunks)) + " chunks")

    // Call the LLM.
    let prompt = "Answer the following question using ONLY the provided context.\n\nContext:\n" + context + "\n\nQuestion: " + question
    let response = chat(cfg, [user(prompt)])

    // Append inference step; this is now the citation-bearing answer.
    let answer = inference(merged, cfg.model, response.text, 0.0)
    return answer
}
```

Key point: `@budget(tokens=4000, calls=1)` caps each query. The `chat()` call's budget hooks fire
inside the active budget frame set by the attribute. Exceeding 4000 tokens throws before or after
the call.

#### rag_citations

```kryos
fn rag_citations(answer: Tracked<str>) -> [str] {
    return tracked_to_citation(answer)
}
```

Thin wrapper; the real work is in `tracked_to_citation`. Keeping it named `rag_citations` makes
the public API read like English: `rag_retrieve -> rag_answer -> rag_citations`.

### Supporting types

```kryos
struct RagChunk {
    id: str,
    text: str
}

struct RagConfig {
    llm: LlmConfig,
    max_chunks: i64
}
```

`RagConfig` is not strictly necessary for the MVP but makes it easy to extend (add embedding
model, chunk size, similarity threshold) without changing every call site.

### File layout

```
src/
  main.kry          -- demo: load 3 hardcoded chunks, answer a question, print citations
  rag.kry           -- rag_retrieve, rag_answer, rag_citations, RagChunk, RagConfig
  lineage_utils.kry -- tracked_merge, tracked_to_citation (omit if project 02 is available)
tests/
  test_rag.kry      -- smoke + assertion tests (see Success Criteria)
kryos.toml
```

---

## MVP Scope (smallest shippable slice)

The MVP is ~250 lines of Kryos across 3-4 files. It does NOT require a vector database,
embeddings, or file I/O beyond hardcoded strings.

**In scope:**
- `RagChunk` struct with `id` and `text`
- `rag_retrieve(query, chunks)`: keyword match, wraps matching chunks in `tracked_source`
- `rag_answer(cfg, tracked_chunks, question)`: `tracked_merge` + `chat()` + `inference()`
- `rag_citations(answer)`: calls `tracked_to_citation`, returns `[str]`
- `tracked_merge` and `tracked_to_citation` implemented here if project 02 is absent
- Smoke test: 3 hardcoded chunks, one question, assert `len(citations) == 3` and citation IDs
  match chunk IDs
- `@budget(tokens=4000, calls=1)` on `rag_answer`
- `@capabilities(net)` on `rag_answer`, `@capabilities(io)` on `rag_retrieve`

**Out of scope for MVP:**
- Vector embeddings or cosine similarity (keyword match is sufficient to prove the lineage chain)
- External chunk storage (file I/O, SQLite, HTTP)
- Streaming responses
- Multi-turn conversation
- Re-ranking retrieved chunks
- Cost tracking with `std.cost.ComputeCost` (add in full vision)

**Full vision (post-MVP):**
- Real embedding similarity via a local model or API (needs `std.http` + embedding endpoint)
- File-based corpus: `rag_load_corpus(path)` reads `.txt` files, chunks them, wraps each chunk
  in `tracked_source` with file path as the source ID
- `std.cost` integration: `rag_answer` returns `(Tracked<str>, ComputeCost)` so callers can
  accumulate per-query billing
- `tracked_cost()` utility (proposed in project 02 unlock doc) to append cost metadata to lineage
- Deduplication of overlapping chunks before merge: if two retrieved chunks share the same `id`,
  merge their lineage entries rather than duplicating
- `Probable<str>` wrapper on the answer with a calibrated confidence score derived from retrieval
  similarity scores (needs similarity-scored retrieval)
- A small HTTP server (`std.http`) exposing `POST /ask` -> `{answer, citations, cost}` as JSON

---

## Build Plan

A fresh session can follow these steps in order. Each step has a concrete deliverable that can be
verified before the next step starts.

### Step 1 - scaffold and imports

Create `kryos.toml` with package name `kryos-rag` version `0.1.0`. Create `src/main.kry` with a
`main()` that just prints `"kryos-rag: ok"`. Run `kryos run src/main.kry` and confirm output.

```kryos
// src/main.kry
fn main() {
    println("kryos-rag: ok")
}
```

### Step 2 - lineage_utils.kry

Write `src/lineage_utils.kry` with `tracked_merge` and `tracked_to_citation`. Write a test
function at the bottom (guarded by a `main()` in the file) that creates two `tracked_source`
calls, merges them, calls `tracked_to_citation`, and asserts the result has 2 entries.

Run `kryos run src/lineage_utils.kry` to confirm.

If project 02 is already on disk, check if it exports `tracked_merge` and `tracked_to_citation`.
If yes, skip this file and use the project 02 path via `use` import.

### Step 3 - rag.kry: RagChunk and rag_retrieve

Write `src/rag.kry`. Start with:

```kryos
use std::tracked::{Tracked, tracked_source, transform, inference, explain}

struct RagChunk {
    id: str,
    text: str
}

@capabilities(io)
fn rag_retrieve(query: str, chunks: [RagChunk]) -> [Tracked<str>] {
    let mut results: [Tracked<str>] = []
    let mut i = 0
    while i < len(chunks) {
        let c = chunks[i]
        if contains(c.text, query) {
            let t = tracked_source(c.text, c.id, "retrieved chunk for query: " + query)
            results = push(results, t)
        }
        i = i + 1
    }
    return results
}
```

Add a quick inline check: create 3 chunks, call `rag_retrieve("kryos", chunks)`, assert result
length. Run with `kryos run src/rag.kry`.

### Step 4 - rag_answer and rag_citations

Add to `src/rag.kry`:

```kryos
use std::llm::{LlmConfig, chat, user}
use std::cost::{ComputeCost}

struct RagConfig {
    llm: LlmConfig,
    max_chunks: i64
}

@capabilities(net)
@budget(tokens=4000, calls=1)
fn rag_answer(cfg: LlmConfig, tracked_chunks: [Tracked<str>], question: str) -> Tracked<str> {
    if len(tracked_chunks) == 0 {
        throw "rag error: no chunks retrieved; cannot answer"
    }
    let mut context = ""
    for tc in tracked_chunks {
        context = context + tc.value + "\n\n"
    }
    let merged = tracked_merge(tracked_chunks, context, "merge",
        "RAG context from " + to_string(len(tracked_chunks)) + " chunks")
    let prompt = "Answer using ONLY the provided context.\n\nContext:\n" + context +
                 "\n\nQuestion: " + question
    let response = chat(cfg, [user(prompt)])
    let answer = inference(merged, cfg.model, response.text, 0.0)
    return answer
}

fn rag_citations(answer: Tracked<str>) -> [str] {
    return tracked_to_citation(answer)
}
```

Do NOT run `rag_answer` yet (requires an API key and incurs cost). Proceed to Step 5 for the
test harness.

### Step 5 - tests/test_rag.kry

Write `tests/test_rag.kry`:

```kryos
use std::test::{assert_eq, assert_true}
use std::tracked::{tracked_source, explain}

fn test_retrieve_finds_matching_chunks() {
    let chunks: [RagChunk] = [
        RagChunk { id: "doc:001", text: "Kryos is a capability-safe language." },
        RagChunk { id: "doc:002", text: "Kryos uses @budget to cap token spend." },
        RagChunk { id: "doc:003", text: "Rust is a memory-safe language." }
    ]
    let results = rag_retrieve("Kryos", chunks)
    assert_eq(len(results), 2)
    assert_eq(results[0].source, "doc:001")
    assert_eq(results[1].source, "doc:002")
}

fn test_tracked_merge_unions_lineages() {
    let a = tracked_source("text A", "doc:001", "chunk 1")
    let b = tracked_source("text B", "doc:002", "chunk 2")
    let c = tracked_source("text C", "doc:003", "chunk 3")
    let merged = tracked_merge([a, b, c], "combined", "merge", "test merge")
    // 3 source entries + 1 merge entry = 4 total
    assert_eq(len(merged.lineage), 4)
}

fn test_citations_deduplicated() {
    let a = tracked_source("text A", "doc:001", "chunk 1")
    let b = tracked_source("text B", "doc:002", "chunk 2")
    let c = tracked_source("text C", "doc:003", "chunk 3")
    let merged = tracked_merge([a, b, c], "combined", "merge", "test merge")
    // Simulate inference step (no LLM needed)
    let answer = inference(merged, "test-model", "synthetic answer", 0.0)
    let citations = rag_citations(answer)
    assert_eq(len(citations), 3)
    assert_eq(citations[0], "source=doc:001")
    assert_eq(citations[1], "source=doc:002")
    assert_eq(citations[2], "source=doc:003")
}
```

Run `kryos test` and confirm all three tests pass.

Note: `test_rag.kry` does NOT call `rag_answer` against a live LLM. The citation chain is
verified by constructing the `Tracked<str>` value manually via `tracked_merge` + `inference()`,
which is how the production function builds it internally. This proves the lineage contract
without an API key.

### Step 6 - live demo in main.kry

Wire up `src/main.kry` to call the full pipeline against a real model. Requires `ANTHROPIC_API_KEY`
or `OPENAI_API_KEY` in the environment.

```kryos
use std::llm::{anthropic_config, with_max_tokens}

fn main() {
    let key = env_get("ANTHROPIC_API_KEY")
    if key == "" {
        println("Set ANTHROPIC_API_KEY to run the live demo.")
        return
    }

    let cfg = with_max_tokens(anthropic_config(key, "claude-haiku-4-5"), 512)

    let corpus: [RagChunk] = [
        RagChunk { id: "doc:001", text: "Kryos is a capability-safe programming language." },
        RagChunk { id: "doc:002", text: "Every Kryos function declares its capability set with @capabilities." },
        RagChunk { id: "doc:003", text: "The @budget attribute caps token and API call spend at compile time." }
    ]

    let query = "capabilities"
    let retrieved = rag_retrieve(query, corpus)
    println("Retrieved " + to_string(len(retrieved)) + " chunks.")

    let answer = rag_answer(cfg, retrieved, "What does @capabilities do in Kryos?")
    println("Answer: " + answer.value)

    let citations = rag_citations(answer)
    println("Citations (" + to_string(len(citations)) + "):")
    for c in citations {
        println("  - " + c)
    }

    println("Full lineage:")
    println(explain(answer))
}
```

Run `kryos run src/main.kry`. Verify that:
1. Answer is non-empty.
2. Citations list has the same number of entries as retrieved chunks.
3. Citation strings match the chunk IDs in the corpus.
4. `explain(answer)` shows source -> merge -> inference chain.

### Step 7 - @capabilities check

Run `kryos check src/rag.kry`. Confirm no capability errors. If the compiler surfaces a
capability violation (e.g. `chat()` inside a function not annotated `@capabilities(net)`), add
the annotation. The self-host compiler's capability checker is opt-in for annotated functions; if
`rag_answer` is not annotated, no error fires but the function is unconstrained. Add
`@capabilities(net)` explicitly for the demo value.

---

## Success Criteria / How to Demo

**Without a live LLM (CI-safe):**
- `kryos test` runs `tests/test_rag.kry` with all 3 assertions passing.
- `test_citations_deduplicated` proves that `answer.lineage IS the citation list` -- no separate
  metadata, no post-processing.

**With a live LLM:**
- `kryos run src/main.kry` (with API key set) produces an answer to "What does @capabilities do
  in Kryos?" with 2-3 citations printed.
- The citations printed match the `RagChunk.id` values of the retrieved chunks.
- `explain(answer)` output shows the full lineage chain from source to merge to inference.

**Demo talking point (one sentence):** "In LangChain you ask for `return_source_documents=True`
and hope the framework wires it through. In Kryos, `answer.lineage` IS the citation list --
it travels with the value by type, not by configuration."

---

## Risks and Honest Unknowns

### Risk 1: tracked_merge on [Tracked<str>] may hit generic array gotcha

The current compiler has a known gap: `let mut a = []` followed by `push(a, SomeStruct{..})` now
infers the type (resolved per CLAUDE.md note 22). However, `[Tracked<str>]` is a generic struct
array. If the JIT mishandles indexed access on `[Tracked<str>]` (e.g. `sources[0].lineage`),
fall back to writing `tracked_merge` with an explicit `let mut combined_lineage: [LineageEntry] =
[]` loop rather than a comprehension. This is already the pattern shown above; it should be safe.

### Risk 2: Generic impl blocks not supported

`Tracked<T>` functions are free functions, not methods. Do not write `impl Tracked<str> { fn
merge(...) }` -- the checker will reject generic impl blocks. All operations are free functions.
This is already reflected in the architecture above.

### Risk 3: tracked_to_citation reads entry.metadata, not entry.source

`tracked_source()` sets `metadata = "source=" + source` (confirmed in tracked.kry line 33). The
`tracked_to_citation` function above reads `entry.metadata` for deduplication. If the team
changes this in a future stdlib version, update the dedup key. The test `assert_eq(citations[0],
"source=doc:001")` will catch any format change.

### Risk 4: @budget attribute on rag_answer is opt-in

Deny-by-default capabilities and budget enforcement at the language level are PLANNED, not
implemented. `@budget(tokens=4000, calls=1)` creates a budget frame that the `chat()` hooks
respect at runtime, but there is no compile-time error if you call `rag_answer` from a context
with no budget frame. The attribute does work as documented: if you call `rag_answer` directly,
the hooks fire and enforce. The gap is that a wrapper function that strips the budget frame is not
caught by the compiler.

### Risk 5: Citation order depends on chunk array order, not relevance rank

`rag_retrieve` returns chunks in the order they were found by keyword scan. With a real
similarity-scored retriever, citation order should reflect relevance. The MVP does not implement
scoring, so citations may not be in relevance order. This is acceptable for the MVP and should be
called out in any demo.

### Risk 6: keyword match does not scale

`contains(c.text, query)` is O(N * M) over the corpus. For a demo with 3-10 chunks this is fine.
For a production RAG with thousands of chunks, replace with an embedding search. The function
signature does not change; only the internals of `rag_retrieve` change.

### Risk 7: project 02 dependency

If project 02 has not been built, `tracked_merge` and `tracked_to_citation` must be implemented
here. The implementations given above are self-contained and have no external dependencies beyond
`std::tracked`. The build plan step 2 addresses this with an explicit fallback.

---

## Dependency Notes

- **Project 02** (tracked utilities library): provides `tracked_merge`, `tracked_to_citation`,
  and potentially `tracked_cost`. If absent, implement inline in `src/lineage_utils.kry`.
- **Project 03** (LLM/budget demo): establishes that `@budget` + `chat()` work correctly on the
  local toolchain. Verify project 03's test passes before building project 04's live demo step.
- **No external packages required for MVP.** `std.tracked`, `std.llm`, `std.cost` are all in
  the self-hosted stdlib at `compiler/stdlib/`.
- **API key:** `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` for Step 6 only. Steps 1-5 run without
  any network access.
