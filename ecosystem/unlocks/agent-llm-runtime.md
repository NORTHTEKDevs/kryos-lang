# Kryos Unlocks: agent-llm-runtime cluster

**Cluster:** agent-llm-runtime
**Source files read:** compiler/stdlib/agent.kry, compiler/stdlib/llm.kry, compiler/stdlib/cost.kry, compiler/stdlib/tracked.kry, compiler/stdlib/probable.kry, docs/10-capabilities.md, examples/showcase/agent.kry, examples/showcase/agent_runtime.kry

---

## What the source actually contains

### std.agent (agent.kry, 195 lines)

Concrete structs and functions, all in Kryos stdlib:

- `AgentMemory { working: [MemoryEntry], semantic: [MemoryEntry], episodic: [MemoryEntry] }` - three buckets, timestamped key-value entries
- `remember(key, value, memory_type)` routes to the right bucket; `recall(key)` searches working then semantic (episodic is write-only in the current impl -- recall does NOT search it)
- `Agent { name, goal, alignment: i64, state: i64, memory, tools: [AgentTool], audit_trail: [AuditEntry], action_count, total_cost, capabilities: [str] }`
- `add_tool(name, handler: fn(str)->str, description)` - first-class function field; `use_tool(name, input)` dispatches
- `spawn_child` - inherits parent alignment and tool set, fresh memory and audit trail
- `pause` / `resume` / `terminate` - state machine transitions (constants CREATED/RUNNING/PAUSED/COMPLETED/FAILED/TERMINATED)
- `AgentSwarm { agents: [Agent] }` - swarm aggregation with `terminate_all`
- Alignment constants: STRICT=0 / STANDARD=1 / MINIMAL=2 / UNRESTRICTED=3 - these are integer flags; the stdlib does NOT enforce alignment at runtime beyond storing the value
- `AuditEntry { id, entry_type, description, success, cost_usd, latency_ms, timestamp }` - audit trail entries stored on the agent struct

**What is NOT in agent.kry:**
- No persistence / serialization of episodic memory
- No `recall` from episodic bucket (write-only today)
- Alignment values are not enforced by the stdlib - they are documentation/convention, not a guard
- No capability gating on tools - `capabilities: [str]` on Agent is a string list, not enforced by the capability checker
- No inter-agent messaging; AgentSwarm only has `add` and `terminate_all`

### std.llm (llm.kry, 619 lines)

Concrete, runnable, fully wired:

- `LlmConfig` (provider, base_url, api_key, model, max_tokens, temperature, timeout_ms) - both "openai" (OpenAI-compatible wire) and "anthropic" (Claude Messages API)
- `chat(cfg, messages)` - full HTTP round-trip via native `http_request` builtin; tagged `@capabilities(net)`
- `complete(cfg, prompt)` - one-shot convenience wrapper
- Budget integration: before each call, checks `kryos_budget_active()` and `kryos_budget_try_call()` via extern hooks; after the call charges `kryos_budget_charge_tokens(in+out)`. Exceeding either limit throws a catchable "llm error: @budget..." string. This is REAL, implemented, runs today.
- `chat_within(cfg, messages, budget)` - explicit Budget struct guard (from std.cost); charges actual usage after the call; returns updated Budget alongside the response
- Tool call protocol: `ToolDef`, `ToolCall`, `ToolResult`, `ToolTurn` structs; `chat_tools` and `continue_with_tool_results` handle full multi-turn tool-use loops for both OpenAI and Anthropic wire formats. The agentic tool loop is first-class in the stdlib, not bolted on.
- `with_base_url` override - points any OpenAI-compatible server (Ollama, vLLM, OpenRouter, LM Studio)

**What is NOT in llm.kry:**
- No streaming (SSE / chunked responses) - chat() returns a complete parsed response only
- No function-schema generation helpers beyond `tool()` constructor (caller writes JSON-Schema by hand)
- No retry / backoff built into chat() - that's in std.backoff / std.circuit separately
- USD accounting in `chat_within` is deliberately 0.0 - per-token pricing must be supplied by the caller

### std.cost integration

`ComputeCost { wall_time_ms, tokens_used, api_calls, money_usd, energy_kwh }` is a composable struct. `Budget` tracks spent vs limits across all three axes (USD, tokens, API calls) and throws `BudgetExceeded` when any axis is crossed. `CostTracker` accumulates entries. `chat_within` consumes this directly. The `@budget(tokens=N, calls=M)` language attribute hooks into `kryos_budget_*` extern fns that the runtime wires up.

### Capability annotation on llm.kry

`chat`, `complete`, `chat_tools`, `continue_with_tool_results`, and `_post` are all annotated `@capabilities(net)`. This is enforced today under the opt-in model: any function that calls `chat()` and carries its own `@capabilities` annotation must include `net`, or it's a compile error (attenuation holds). Without `--strict-capabilities` (not yet implemented), unannotated callers are unconstrained.

---

## Honest novelty analysis

### 1. LLM clients in the stdlib (not a framework)

**Who else does it:** Python does not have LLM clients in its stdlib (you need httpx + openai-python or anthropic-sdk as third-party packages). Go, Rust, TypeScript are the same - every LLM integration is an ecosystem library. Elixir's Nx ecosystem has LangChain-Elixir as a separate library. There is no mainstream language where LLM calls are in the standard library.

**Novelty: PARTIAL.** Having `chat()` in the stdlib is a real differentiator for ergonomics - zero-import LLM calls, no version pinning on a client library, same behavior on every Kryos install. But it's not *unheard of* - Mojo stdlib and Julia have domain-specific builtins. The real novelty is the coupling between `chat()` and the budget runtime hooks inside the same file. That coupling is what prevents a runaway agent from being silently unbounded.

**Buildable today:** yes, the chat/complete/tool-use path works now.

### 2. Language-level token/call budget enforcement (@budget attribute + runtime hooks)

**Who else does it:** Nobody in a production language. OpenAI's Responses API has a `max_completion_tokens` field, but that is a per-request wire parameter, not a language-level budget that spans multiple calls within a function scope. LangChain has callback handlers that can observe token usage but cannot *throw and halt execution* from within a running Python function scope. DSPy has budget-like concepts in its optimizer but not at the language attribute level. The `@budget(tokens=N, calls=M)` attribute that causes `chat()` to pre-charge and post-charge, and throw when exceeded, is implemented in Kryos's runtime hooks and stdlib today.

**Novelty: TRULY-NOVEL.** The specific design - a function attribute that creates a budget frame on the current thread, with `chat()` automatically checking that frame before and after every LLM call - does not exist as a first-class language feature anywhere else. The throw-on-exceed path halts agent loops at the language level, not as an application-layer guard.

**Buildable today:** yes. The `kryos_budget_active` / `kryos_budget_try_call` / `kryos_budget_charge_tokens` extern hooks are present in llm.kry and called on every `chat()` invocation.

**Honest caveat:** `@budget` is a function attribute checked at call sites by the runtime hooks. It does not constrain *what* the LLM is called with (prompt size, model choice). It constrains cumulative usage within the annotated function's call frame. This is valuable but narrower than a full resource governor.

### 3. Agent memory trifecta in the stdlib (working / semantic / episodic)

**Who else does it:** LangChain has `ConversationBufferMemory`, `VectorStoreRetrieverMemory`, and custom memory classes. AutoGPT, CrewAI, and LlamaIndex all have memory abstractions. The cognitive science trifecta (working/semantic/episodic) appears in several research agent frameworks (MemGPT is the best-known example).

**Novelty: PARTIAL.** The concepts are not novel; the implementation in a *language stdlib* (not a Python library) is the differentiator. You get `agent_memory_new()` and `remember()` without a package install.

**Honest limitation:** `recall()` searches working then semantic but NOT episodic. Episodic is write-only today. There is no vector search, no embedding retrieval, no persistence to disk or DB. The memory is in-process RAM for the lifetime of the program. This is a scaffolding layer, not a production memory system.

**Buildable today:** yes, but the episodic recall gap means real episodic use requires application-layer workarounds (serialize to std.db or std.fs).

### 4. Alignment levels as a first-class agent property

**Who else does it:** No production language stdlib has alignment levels. Anthropic's Constitutional AI and OpenAI's system prompt conventions are model-side. Some research frameworks (CAMEL, AgentBench) have role/alignment concepts. Kryos's `ALIGNMENT_STRICT / STANDARD / MINIMAL / UNRESTRICTED` constants are stored on the `Agent` struct and inherited by `spawn_child`.

**Novelty: PARTIAL, with a critical honesty caveat.**

The values are stored but NOT enforced. Nothing in agent.kry checks `self.alignment` before calling a tool or making an LLM call. `spawn_child` propagates the parent's alignment integer to the child, which is good design, but the runtime doesn't prevent a STRICT agent from calling any tool. Enforcement is entirely application-layer today.

For the thesis to hold ("alignment-at-the-language-level"), the stdlib needs to gate tool dispatch and LLM calls on the alignment value. That is a meaningful feature gap.

**Buildable today:** the scaffolding is there; enforcement is NOT.

### 5. Tool-call protocol in the stdlib (chat_tools / continue_with_tool_results)

**Who else does it:** LangChain, LlamaIndex, and the OpenAI Python SDK all implement multi-turn tool-use loops. The Anthropic SDK does too. These are all third-party libraries.

**Novelty: PARTIAL.** Having `chat_tools()` and `continue_with_tool_results()` in the *language stdlib* means zero external dependencies for a fully functional tool-use agent loop. The wire-format abstraction (handles both OpenAI and Anthropic) is a genuine convenience. But the pattern itself is well-understood.

**Buildable today:** yes, both function bodies are complete and handle both wire formats.

### 6. Capability annotation on LLM calls (@capabilities(net) on chat())

**Who else does it:** WebAssembly component model (WASI) uses capability imports. Deno requires `--allow-net` flags. Wren and Roc have effect systems. Rust's `#[deny(unsafe_code)]` is analogous for memory safety. Python/Node: anything goes.

**Novelty: PARTIAL.** Kryos's capability system is more integrated than Deno flags (function-level, call-graph-aware, attenuation enforced) but the general idea is not new. The specific coupling of `@capabilities(net)` with LLM calls means that any function calling `chat()` that has its own capability annotation MUST declare `net` - the compiler enforces this transitively today.

**Honest caveat:** without `--strict-capabilities` (not implemented), unannotated functions are unconstrained. The deny-by-default model that would make this truly compelling is planned, not built.

**Buildable today:** opt-in enforcement works now. Deny-by-default needs language work.

### 7. Cost as a composable value (std.cost + chat_within)

**Who else does it:** OpenTelemetry tracks latency and can track custom metrics including token counts. CloudWatch, Datadog, and LangSmith all have cost dashboards. None of these are in a language stdlib and none make cost a *value you compute with* in your program logic.

**Novelty: PARTIAL.** The idea of `ComputeCost` as a first-class struct you can `cost_add()` and pass around is clean. `chat_within()` returning a `BudgetedChat { response, budget }` so the caller can see the updated remaining budget is elegant. But OpenTelemetry spans and LangSmith traces accomplish the same thing for observability purposes, just not inline in the program.

**Buildable today:** yes, `chat_within` is fully implemented. USD accounting requires the caller to set `money_usd` explicitly (the stdlib sets it to 0.0 and says "charge per-token pricing yourself").

---

## What a fully governed agent runtime would enable (combining all primitives)

This is the thesis scenario - what you get when capability + budget + tracked + agent + llm compose:

```kryos
@capabilities(net)
@budget(tokens=50000, calls=20)
fn run_governed_agent(task: str) -> Tracked<str> {
    let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
    let agent = agent_with_alignment("auditor", task, ALIGNMENT_STRICT)
    
    // LLM call: automatically budget-checked before and after
    let turn = chat_tools(cfg, [system("Be precise."), user(task)], my_tools)
    
    // Wrap the result in a lineage-tracked value
    let raw = tracked_source(turn.text, "anthropic/claude-sonnet-4-6", task)
    return inference(raw, "claude-sonnet-4-6", turn.text, 0.9)
}
```

The `@budget` attribute halts the function if cumulative tokens or calls exceed the limit. The `@capabilities(net)` annotation means any caller without `net` in its own capability set cannot call this function (attenuation). The `Tracked<str>` return carries the full provenance of the answer. Today, all three of these things work.

What does NOT work today in this scenario:
- The `agent` variable's `alignment: ALIGNMENT_STRICT` does not restrict what `chat_tools` is called with
- The `episodic` bucket in agent.kry is not recalled
- Deny-by-default would be needed to make the capability guarantee hold for unannotated callers

---

## Proposed new stdlib functions

### agent_tool_with_capability(name, handler, description, required_cap)

Stores a required capability string alongside the tool. `use_tool` checks whether the agent's declared capability list contains `required_cap` before dispatching, throwing if not. This wires alignment-and-capability together at tool dispatch time. Requires no language changes - pure stdlib.

### chat_tools_governed(cfg, messages, tools, agent)

Wraps `chat_tools` + `continue_with_tool_results` in a single call that checks `agent.alignment` and `agent.action_count` against a configurable step limit before each model call, appends to `agent.audit_trail` after each call, and returns both the `ToolTurn` and the updated `Agent`. Makes the "audit everything" pattern the default, not opt-in.

### agent_checkpoint(agent, serializer_fn)

Serializes `agent.memory.episodic` and `agent.audit_trail` to a string via the caller-supplied `serializer_fn: fn(Agent) -> str`. Bridges to `file_write` or `db_exec` for persistence. Keeps stdlib dependency-free (no fs/db import in agent.kry) while enabling the episodic persistence gap to be closed at the application layer with a one-liner.

---

## What needs language work before the full thesis holds

1. `--strict-capabilities`: deny-by-default enforcement for unannotated functions. Without this, capability annotations are documentation on annotated functions only; unannotated callers can call `chat()` freely.
2. Sub-capabilities: `agent:autonomous`, `net:llm` style restrictions so you can grant LLM-call capability without granting arbitrary raw socket access.
3. Alignment enforcement in the stdlib: agent.kry needs to actually check `self.alignment` before tool dispatch and optionally before `chat()` calls. Right now the integer is stored and inherited but never read by any stdlib function.
4. Episodic memory recall: `recall()` in agent.kry skips the episodic bucket. Either add it or document episodic-as-log-only explicitly.
5. Runtime `CapabilityEnforcer`: the docs describe it but it is not implemented. The compile-time checker covers annotated functions; the runtime layer that makes capability violations truly uncatchable does not exist yet.

---

## Summary verdict

The Kryos agent-llm-runtime cluster is a genuine, partially-implemented substrate for governed agent software. The language-level `@budget` + runtime hooks that halt runaway loops is **truly novel** - no other production language has this. The LLM client in the stdlib is a real ergonomic win. The capability annotation on `chat()` works for the opt-in case today. The agent memory trifecta and alignment levels are well-designed scaffolding, not yet enforced.

The honest positioning: Kryos is 60-70% of the way to the thesis. The buildable-today story is strong for budget enforcement and tool-use. The capability-safety story needs `--strict-capabilities` to become a real guarantee rather than a useful convention. Alignment enforcement is purely aspirational today.

Mojo is the substrate for GPU/AI compute kernels. Kryos can own AI agent governance and safe execution - but that claim needs to be hedged with the honest status of the capability system until strict mode ships.
