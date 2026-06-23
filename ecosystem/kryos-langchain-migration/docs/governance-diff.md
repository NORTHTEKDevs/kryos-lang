# Governance Diff: LangChain Python vs Kryos

This table maps each class of Python/LangChain runtime failure to its Kryos equivalent.
"COMPILE ERROR" means the program does not build at all — the mistake is caught before any code runs.

---

## 1. Capability Violations

| Python/LangChain (runtime) | Kryos (compile) |
|---|---|
| `requests.get(url)` inside a function with no network policy → **AuthenticationError / ConnectionError at runtime** | `http_get(url)` in a function without `@capabilities(net:http)` → **E0505 compile error**: `builtin http_get requires net:http capability` |
| `open(path, "w")` inside a sandboxed tool → **PermissionError at runtime** (if the host blocks it) | `file_write(path, content)` without `@capabilities(fs:write)` → **E0505 compile error** |
| Missing `OPENAI_API_KEY` → **AuthenticationError** propagates at the first `llm.invoke()` call, potentially after 10 seconds of setup | `chat(cfg, msgs)` without `@capabilities(net)` on the caller → compile error before the binary runs |
| Capability escapes through arbitrary `eval()` in the calculator tool (see reference/langchain_rag_tools.py line 36) → **code injection** at runtime | `@capabilities(builtin)` or similar is explicit; the Kryos compiler rejects calling any net/io builtin not declared — there is no `eval` surface |

---

## 2. Silent Citation Drops

| Python/LangChain (silent runtime drop) | Kryos (type error) |
|---|---|
| `Document.metadata["source"]` is a loosely attached dict key. String formatting in `retrieve_docs` tool embeds it as plain text — if a downstream chain truncates or reformats the context string, the citation is **silently discarded**. Nothing in the type system prevents this. | `tracked_source(text, id, desc)` returns `Tracked<str>`. Passing a plain `str` where `Tracked<str>` is required → **type mismatch compile error E0100**. `tracked_to_citation()` can only be called on a `Tracked<str>`, so citation handling is opt-out-impossible. |
| `retriever.invoke(query)` returns `[Document]`. A developer can call `doc.page_content` and drop `doc.metadata` with no warning. | `retrieve(query, chunks)` returns `[Tracked<str>]`. Accessing `.value` is fine; the `.lineage` array is always present and `tracked_to_citation` traverses it — there is no `.page_content` without the attached metadata. |
| LangChain's `StrOutputParser` strips the document to a plain string — all citation metadata is gone by the time it reaches the user. | `tracked_merge(retrieved, answer, ...)` merges lineage arrays from all input `Tracked<str>` values into the output. The compiler enforces that the return type of `rag_answer` (or its stub equivalent) is `Tracked<str>`, not `str`. |

---

## 3. Runaway Loops / Unbounded Spend

| Python/LangChain (runtime / soft hint) | Kryos (compile-time frame) |
|---|---|
| `AgentExecutor(max_iterations=5)` is a soft check inside the Python loop — it can be ignored, misconfigured, or bypassed by subclasses. No LLM/API spend is bounded before the call. | `@budget(calls = 10)` on `governed_dispatch` is a compile-time annotation. The runtime inserts a counter decrement before every call that counts against the budget. Exceeding it **throws** — the budget is not advisory. |
| No compile-time guard on the number of `llm.invoke()` calls inside the agent loop. The loop can run indefinitely if `AgentFinish` is never returned (e.g. due to a tool error the model retries in a loop). | `@budget(tokens = 100000, calls = 50)` on `chat_tools_governed` (kryos-agent-loop) stacks with any caller-frame budget. The tighter limit wins — two frames active simultaneously means the first to hit zero throws. |
| LangChain streaming: if the user does not set `max_tokens` on `ChatOpenAI`, completions can consume unbounded output tokens and dollars. | `@budget(usd = 1.00)` (real money budget, used in kryos-bench-governed) throws **before** the HTTP call if the estimated spend would exceed the cap — no charge incurred. |

---

## 4. Missing Return / Unhandled None

| Python/LangChain | Kryos |
|---|---|
| A tool function that forgets a return path returns `None` implicitly, causing a `TypeError` downstream when the agent tries to concatenate the tool result into the prompt string. | A Kryos function with a declared `-> str` return type that has a control path without a `return` → **E0101 / missing return compile error**. |
| `Optional[str]` return type annotation is advisory in Python; callers can ignore it and call `.upper()` on `None` at runtime. | `Option<str>` must be matched (`Some(v)` / `None()`) before the inner `str` is used. Calling a `str` method on an `Option<str>` without unwrapping → **type mismatch E0100 compile error**. |

---

## Summary

The governance delta is: **Python surfaces all four classes of failure at runtime (after money/time is spent); Kryos surfaces them at compile time before any execution.** The `Tracked<str>` type is the citation-safety primitive; `@capabilities` is the capability-safety primitive; `@budget` is the spend-safety primitive. None of these have an equivalent in LangChain's runtime type system.
