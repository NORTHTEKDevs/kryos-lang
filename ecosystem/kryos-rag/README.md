# kryos-rag

RAG pipeline where **every answer carries its source citations as a first-class
`Tracked<str>` value** -- not post-processed metadata, not a parallel dict, not a
log line you might forget.

In LangChain you ask for `return_source_documents=True` and hope the framework
wires it through. In Kryos, `answer.lineage` **is** the citation list -- it
travels with the value by type. A function that takes `Tracked<str>` and returns
a bare `str` has visibly dropped the citations in its signature.

## Pipeline

```
rag_retrieve(query, chunks)   -- [Tracked<str>]  (each chunk stamped via tracked_source)
        |
rag_answer(cfg, chunks, q)    --  Tracked<str>   (tracked_merge unions lineages -> chat() -> inference)
        |
rag_citations(answer)         --  [str]          (dedup of the source entries in answer.lineage)
```

## Files

- `src/rag.kry` -- `RagChunk`, `rag_retrieve` (`@capabilities(io)`), `rag_citations`
- `src/rag_llm.kry` -- `rag_answer` (`@capabilities(net)` `@budget(tokens=4000, calls=1)`)
- `src/lineage_utils.kry` -- `tracked_merge`, `tracked_to_citation`
- `src/main.kry` -- live demo
- `tests/test_rag.kry` -- 3 assertions proving the lineage chain without a live LLM

## Test (no API key, no network)

```bash
kryos run tests/test_rag.kry     # runs all 3 assertions; exits 1 on any failure
```

> Note: run the tests with `kryos run`, **not** `kryos test`. On this toolchain a
> file with no `@test` functions makes `kryos test` report a vacuous "1 passed"
> (exit 0) even when an assertion fails, and `@test` itself can't be used here
> because the JIT path can't resolve `kryos_string_retain` for `std::tracked`.
> `kryos run tests/test_rag.kry` genuinely executes the assertions.

## Live demo

```bash
NVIDIA_API_KEY=nvapi-...   ./run-demo.sh     # NVIDIA NIM (meta/llama-3.1-8b-instruct)
ANTHROPIC_API_KEY=sk-ant-... ./run-demo.sh   # Claude (claude-haiku-4-5)
```

Example output (NVIDIA):

```
Retrieved 2 chunks for query: capability
Answer: @capabilities declares the capability set a Kryos function is allowed to use...
Citations (2):
  - source=doc:001
  - source=doc:002
Full lineage:
  1. [source]    retrieved chunk for query: capability   source=doc:001
  2. [source]    retrieved chunk for query: capability   source=doc:002
  3. [merge]     RAG context from 2 chunks               merged=2 sources
  4. [inference] Model: meta/llama-3.1-8b-instruct       confidence=0
```

The printed citations match the `RagChunk.id` values of the retrieved chunks, and
`explain(answer)` shows the full source -> merge -> inference chain.

## Scope

MVP: keyword retrieval, no vector DB / embeddings / HTTP server. The retrieval
mechanism is deliberately trivial -- the point is the lineage chain, which is
identical for cosine-similarity retrieval. See
`../projects/04-kryos-rag-rag-pipeline-with-built-in-citation-lineage.md` for the
full vision (file-based corpus, cost tracking, `Probable<str>` confidence, HTTP
server).
