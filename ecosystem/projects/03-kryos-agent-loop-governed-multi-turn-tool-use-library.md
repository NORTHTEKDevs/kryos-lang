# kryos-agent-loop: Governed Multi-Turn Tool-Use Library

**One-line pitch:** `chat_tools_governed()` -- every LLM call and tool dispatch is logged with cost and latency as the zero-effort default, turning the ad-hoc tool loop from `examples/showcase/budget_analyst.kry` into a single reusable function that any caller can audit.

---

## Why This Exists

### The gap in the current stdlib

`std::llm` ships `chat_tools` and `continue_with_tool_results`. Together they implement the mechanical wire protocol of a multi-turn tool loop. What they do NOT do:

- Populate `agent.audit_trail` -- `AuditEntry` in `std::agent` is defined and the field is on every `Agent` struct, but the stdlib never writes to it. The only way to get an audit trail today is for the application to call `agent.use_tool()` manually, which is a different API path than the `chat_tools` / `continue_with_tool_results` flow.
- Record latency per tool call.
- Enforce alignment before dispatching a tool.
- Accumulate cost across the full loop into a `ComputeCost` value the caller can inspect or record.
- Checkpoint the agent state for persistence.

The result: every app that uses `chat_tools` re-implements the audit/cost/alignment boilerplate. The `budget_analyst.kry` showcase does this inline with ~15 lines of `annotate()` calls woven through the loop. That is precisely the code this library eliminates.

### Who else does this / novelty rating: PARTIAL

The pattern of "wrap your LLM tool loop in an instrumented harness" is not novel. LangChain has `AgentExecutor` with callbacks; LlamaIndex has `ReActAgent` with step tracers; OpenAI's Assistants API logs tool calls server-side. What those approaches have in common: the instrumentation is a framework convention, not a language guarantee.

Kryos is different in three specific ways that are language-level, not framework-level:

1. `@budget(tokens=N, calls=M)` on `chat_tools_governed` means the compiler inserts `kryos_budget_try_call` / `kryos_budget_charge_tokens` calls around every `chat_tools` / `continue_with_tool_results` invocation inside the function. The ceiling is not a check the library author remembers to add -- it is emitted by the code generator. No amount of buggy loop logic in the caller can bypass it.

2. The `AuditEntry` array returned is the same `std::agent` type used everywhere else in the ecosystem. It is not a custom logging format -- it is the native audit trail of the `Agent` struct, populated by the same path `agent.use_tool()` uses.

3. Cost comes back as `ComputeCost` (a first-class stdlib value), not a print statement or a callback parameter. The caller can pass it directly to `budget.charge()`, `cost_add()`, or `tracked.annotate()`.

Honest status: the language work that would make this truly novel (deny-by-default capabilities, so a governed loop provably cannot call `file_write` unless declared) is not yet implemented. Sub-capabilities (`net:outbound-only`) are not yet implemented. What IS implemented and works today: `@budget`, `std::cost`, `std::agent.AuditEntry`, `chat_tools`, `continue_with_tool_results`.

---

## Which Kryos Primitives This Uses

All of the following are implemented and available today (verified in source):

| Primitive | Where it lives | What this library uses |
|---|---|---|
| `@budget(tokens=N, calls=M)` | `kryos-rt/src/budget.rs`; compiler attribute | Annotate `chat_tools_governed` so the model-call ceiling is compiler-enforced |
| `std::llm::chat_tools` | `compiler/stdlib/llm.kry` | First turn of the loop |
| `std::llm::continue_with_tool_results` | `compiler/stdlib/llm.kry` | Continuation turns |
| `std::llm::ToolTurn`, `ToolCall`, `ToolResult`, `ToolDef`, `LlmConfig` | `compiler/stdlib/llm.kry` | All the wire types |
| `std::agent::AuditEntry`, `Agent`, `ALIGNMENT_STRICT` etc. | `compiler/stdlib/agent.kry` | Audit trail population and alignment check |
| `std::cost::ComputeCost`, `cost_add`, `cost_zero` | `compiler/stdlib/cost.kry` | Per-loop cost accumulation |
| `std::tracked::Tracked`, `tracked_source`, `annotate`, `inference` | `compiler/stdlib/tracked.kry` | Optional: wrap the final answer with full lineage |
| `@capabilities(net)` | `kryos-capabilities` crate | Annotate the top-level entry points |

### Language work needed first

None for the MVP. The complete MVP is buildable today with existing language features.

Limitations to document honestly in the library:
- Alignment check is enforced by the library calling a Kryos function -- it is NOT a compiler gate. A caller can bypass it by calling `chat_tools` directly. True compiler-enforced alignment gating requires a language feature that does not exist yet.
- `@capabilities` annotation on `chat_tools_governed` declares `net` but sub-capabilities like `net:outbound-only` or `net:allow-list` do not exist yet. Any host with `net` can reach any URL.
- The `@budget` attribute works but requires the CALLER to annotate their function with `@budget` if they want the ceiling enforced there too. A function that calls `chat_tools_governed` without `@budget` on itself has no token ceiling.

---

## Architecture

### Component map

```
src/
  lib.kry         -- the one file that matters
  mock_server.kry -- a tiny OpenAI-wire-compatible test fixture (no network)
tests/
  governed_loop_test.kry
```

### Key types (all from existing stdlib -- this library adds none)

The library's API surface consists entirely of functions. The data types it accepts and returns are already in the stdlib:

```
LlmConfig      (std::llm)
Message        (std::llm)
ToolDef        (std::llm)
ToolTurn       (std::llm)
ToolResult     (std::llm)
Agent          (std::agent)
AuditEntry     (std::agent)
ComputeCost    (std::cost)
```

One new struct is needed to bundle the loop's return value:

```kryos
// The result of one governed multi-turn run.
struct GovernedResult {
    final_turn:  ToolTurn,    // the ToolTurn where done == true
    agent:       Agent,       // agent with audit_trail populated
    total_cost:  ComputeCost, // sum across all model calls in the loop
    steps:       i64          // number of model calls made
}
```

### Core function: `chat_tools_governed`

```kryos
use std::llm::{LlmConfig, Message, ToolDef, ToolTurn, ToolResult,
               chat_tools, continue_with_tool_results, tool_result}
use std::agent::{Agent, AuditEntry, ALIGNMENT_STRICT, ALIGNMENT_STANDARD,
                 STATE_RUNNING, STATE_COMPLETED, STATE_FAILED}
use std::cost::{ComputeCost, cost_zero, cost_add}

// dispatch_fn: the caller's tool router, same shape as budget_analyst's run_tool.
// Called once per tool call the model requests.
// Returns a JSON string result (or an error message string).

@capabilities(net)
@budget(tokens = 100000, calls = 50)
fn chat_tools_governed(
    cfg:         LlmConfig,
    messages:    [Message],
    tools:       [ToolDef],
    dispatch_fn: fn(str, str) -> str,
    agent:       Agent,
    max_steps:   i64
) -> GovernedResult {

    let mut ag = agent
    ag.state = STATE_RUNNING
    let mut total = cost_zero()
    let mut steps = 0

    // First turn
    let mut turn = chat_tools(cfg, messages, tools)
    steps = steps + 1
    total = cost_add(total, ComputeCost {
        wall_time_ms: 0.0,
        tokens_used:  turn.input_tokens + turn.output_tokens,
        api_calls:    1,
        money_usd:    0.0,
        energy_kwh:   0.0
    })

    // Record the initial model turn in the audit trail
    ag.audit_trail = push(ag.audit_trail, AuditEntry {
        id:          "turn-" + to_string(steps),
        entry_type:  "model_call",
        description: "initial turn: " + to_string(turn.output_tokens) + " output tokens",
        success:     true,
        timestamp:   time_now_secs(),
        cost_usd:    0.0,
        latency_ms:  0.0
    })

    // Tool loop
    while not turn.done and steps < max_steps {
        // Alignment check before dispatching any tool
        if ag.alignment == ALIGNMENT_STRICT {
            // In STRICT mode: only tools that are registered on the agent
            // are allowed. Others are refused and recorded as failed.
            // (sub-capability gating is a future language feature)
            for call in turn.tool_calls {
                let allowed = _tool_registered(ag, call.name)
                if not allowed {
                    ag.audit_trail = push(ag.audit_trail, AuditEntry {
                        id:          "block-" + call.id,
                        entry_type:  "alignment_block",
                        description: "STRICT: refused tool " + call.name + " (not in agent.tools)",
                        success:     false,
                        timestamp:   time_now_secs(),
                        cost_usd:    0.0,
                        latency_ms:  0.0
                    })
                    ag.state = STATE_FAILED
                    return GovernedResult { final_turn: turn, agent: ag, total_cost: total, steps: steps }
                }
            }
        }

        // Dispatch each requested tool call
        let mut results: [ToolResult] = []
        for call in turn.tool_calls {
            let t_start = time_now_secs()
            let output = dispatch_fn(call.name, call.arguments_json)
            let latency = (time_now_secs() - t_start) as f64 * 1000.0

            results = push(results, tool_result(call.id, output))

            ag.audit_trail = push(ag.audit_trail, AuditEntry {
                id:          call.id,
                entry_type:  "tool_call",
                description: call.name + "(" + call.arguments_json + ") -> " + output,
                success:     not contains(output, "error:"),
                timestamp:   time_now_secs(),
                cost_usd:    0.0,
                latency_ms:  latency
            })
        }

        // Continue the conversation with results
        turn = continue_with_tool_results(cfg, messages, tools, turn.assistant_raw, results)
        steps = steps + 1
        total = cost_add(total, ComputeCost {
            wall_time_ms: 0.0,
            tokens_used:  turn.input_tokens + turn.output_tokens,
            api_calls:    1,
            money_usd:    0.0,
            energy_kwh:   0.0
        })

        ag.audit_trail = push(ag.audit_trail, AuditEntry {
            id:          "turn-" + to_string(steps),
            entry_type:  "model_call",
            description: "continuation turn " + to_string(steps) + ": " + to_string(turn.output_tokens) + " tokens",
            success:     true,
            timestamp:   time_now_secs(),
            cost_usd:    0.0,
            latency_ms:  0.0
        })
    }

    ag.state = STATE_COMPLETED
    ag.total_cost = total.money_usd
    ag.action_count = steps

    return GovernedResult { final_turn: turn, agent: ag, total_cost: total, steps: steps }
}
```

Note: `time_now_secs()` is the `kryos_time_now_secs` builtin (used in `examples/showcase/agent.kry`). The latency computation here gives wall-seconds precision; sub-second resolution would require a millis builtin -- document this limitation and use `0.0` for latency_ms if it is not available (check the runtime before writing this line).

### Helper: `_tool_registered`

```kryos
fn _tool_registered(ag: Agent, name: str) -> bool {
    for t in ag.tools {
        if t.name == name { return true }
    }
    return false
}
```

### Checkpoint helper: `agent_checkpoint`

```kryos
// Serialize an agent's audit trail to a file for persistence.
// serialize_fn: the caller provides the serialization strategy
//   (use std::json builtins to build the JSON; this library does not
//   prescribe a schema for the audit trail, only that it is exportable).

@capabilities(io)
fn agent_checkpoint(ag: Agent, path: str) {
    let mut lines: [str] = []
    for entry in ag.audit_trail {
        let line = entry.id + "\t" + entry.entry_type + "\t"
                 + entry.description + "\t"
                 + to_string(entry.success) + "\t"
                 + to_string(entry.timestamp) + "\t"
                 + to_string(entry.cost_usd) + "\t"
                 + to_string(entry.latency_ms)
        lines = push(lines, line)
    }
    let mut out = ""
    for l in lines {
        out = out + l + "\n"
    }
    file_write(path, out)
}
```

### Caller pattern (what a user of this library writes)

```kryos
use std::llm::{anthropic_config, with_max_tokens, system, user, tool}
use std::agent::{agent_with_alignment, ALIGNMENT_STANDARD}

fn dispatch(name: str, args_json: str) -> str {
    if name == "calc" { return do_calc(args_json) }
    return "error: unknown tool " + name
}

@capabilities(net)
@budget(tokens = 40000, calls = 10)
fn main() {
    let cfg = with_max_tokens(anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6"), 1024)
    let tools = [tool("calc", "arithmetic", schema_calc())]
    let msgs = [system("You are a precise analyst. Use calc for all math."), user("What is 23% of 4400?")]
    let ag = agent_with_alignment("analyst", "answer numeric questions", ALIGNMENT_STANDARD)

    let result = chat_tools_governed(cfg, msgs, tools, dispatch, ag, 8)

    println("Answer: " + result.final_turn.text)
    println("Steps:  " + to_string(result.steps))
    println("Tokens: " + to_string(result.total_cost.tokens_used))
    println("Audit entries: " + to_string(len(result.agent.audit_trail)))
}
```

This is the complete calling pattern. The caller writes `dispatch`, sets up tools and messages, and calls one function. Everything else is handled.

---

## MVP Scope

Smallest shippable slice:

- `lib.kry` implementing `chat_tools_governed` and `agent_checkpoint` (~200 lines)
- `_tool_registered` helper
- Integration test in `governed_loop_test.kry` against a local OpenAI-wire mock (no real API key required for CI)
- The mock server (`mock_server.kry`) hardcodes two responses: one with a tool call, one with a final text answer -- enough to exercise the full two-turn loop

What the MVP does NOT include:
- USD cost accounting (leave `money_usd: 0.0`; caller can add their own rate)
- `energy_kwh` tracking (leave `0.0`)
- Streaming support (not in stdlib yet)
- Per-tool timeout enforcement
- Structured output parsing

Full vision (post-MVP):
- `chat_tools_governed_tracked` variant that wraps the final turn text in a `Tracked<str>` with full lineage (builds directly on 02, the governed-agent-stdlib-extension project)
- `chat_tools_governed_probable` variant that returns `Probable<str>` with confidence extracted from the model's own hedging language
- Per-tool capability gating once sub-capabilities land in the compiler
- A `GovernedLoopConfig` struct with tunable: alignment mode, max_steps, latency_budget_ms, USD ceiling

---

## Build Plan

Follow these steps in order. Each step is independently verifiable.

### Step 1: scaffold the project (10 min)

```
mkdir -p kryos-agent-loop/src
mkdir -p kryos-agent-loop/tests
```

Create `kryos-agent-loop/kryos.toml`:

```toml
[package]
name = "kryos-agent-loop"
version = "0.1.0"

[dependencies]
```

No external dependencies. All primitives are stdlib.

### Step 2: write `src/lib.kry` (~200 lines)

Implement in this order:
1. Import block (all the `use std::` lines at top)
2. `GovernedResult` struct
3. `_tool_registered` helper
4. `chat_tools_governed` (the main function)
5. `agent_checkpoint`

Check that `time_now_secs()` is available as a builtin: search `examples/showcase/agent.kry` for its usage, or `grep kryos_time_now_secs compiler/`. If it is not a builtin, set `latency_ms: 0.0` in all `AuditEntry` constructions and add a TODO comment.

Run `kryos check src/lib.kry` after each function to catch type errors early.

### Step 3: write `src/mock_server.kry` (~80 lines)

This is a standalone script that starts an HTTP server returning:
- On the first POST to `/v1/messages` or `/v1/chat/completions`: a response with one tool call (`calc`, arguments `{"a": 100, "b": 0.23, "op": "mul"}`)
- On the second POST: a final text response `"23% of 100 is 23.0"`

Use `std::http` or the `http_server.kry` pattern from `examples/real/http_server.kry`. The server should exit after two requests.

Run it with `kryos run src/mock_server.kry` in a background terminal to verify it responds correctly with `curl`.

### Step 4: write the integration test (`tests/governed_loop_test.kry`, ~120 lines)

```kryos
@test
fn test_two_turn_loop_populates_audit_trail() {
    // Point cfg at the local mock server started in Step 3.
    // Assert:
    //   result.steps == 2
    //   len(result.agent.audit_trail) == 3  (turn-1, tool call, turn-2)
    //   result.final_turn.done == true
    //   result.total_cost.tokens_used > 0
    //   result.total_cost.api_calls == 2
    assert(result.steps == 2, "expected 2 model calls")
    assert(len(result.agent.audit_trail) == 3, "expected 3 audit entries")
    assert(result.final_turn.done, "loop should have terminated")
}

@test
fn test_budget_exhausted_throws() {
    // Wrap in a @budget(calls=1) function. The second model call should throw.
    // Catch it with try/catch, assert the error message contains "@budget".
}

@test
fn test_strict_alignment_blocks_unregistered_tool() {
    // Create an agent with ALIGNMENT_STRICT and an empty tools list.
    // The mock server returns a tool call for "calc".
    // Assert result.agent.state == STATE_FAILED.
    // Assert the audit trail contains an "alignment_block" entry.
}

@test
fn test_agent_checkpoint_writes_file() {
    // Call chat_tools_governed against the mock.
    // Call agent_checkpoint(result.agent, "/tmp/audit.tsv").
    // Assert file_exists("/tmp/audit.tsv") == 1.
    // Read the file, assert it contains "tool_call".
}
```

Run: `kryos test tests/governed_loop_test.kry`

### Step 5: verify the full showcase

Copy `examples/showcase/budget_analyst.kry`. Replace the inline `annotate` loop with a call to `chat_tools_governed`. Verify the output is identical. This is the regression test that the library is a correct drop-in for the ad-hoc pattern.

Run: `kryos run examples/showcase/budget_analyst_v2.kry "What is 17% of 2340?"`

---

## Success Criteria / Demo

The demo is a single terminal session:

```
$ ANTHROPIC_API_KEY=sk-... kryos run demo.kry "What is 23% of 4400?"

Answer: 23% of 4400 is 1012.0
Steps:  2
Tokens: 847
Audit entries: 3

=== Audit Trail ===
turn-1  model_call  initial turn: 312 output tokens  true  1718000001  0.0  0.0
call-xyz  tool_call  calc({"a":4400,"b":0.23,"op":"mul"}) -> 1012.0  true  1718000002  0.0  84.0
turn-2  model_call  continuation turn 2: 41 tokens  true  1718000003  0.0  0.0
```

Checkpoints the audit trail:

```
$ cat /tmp/audit.tsv
turn-1	model_call	initial turn: ...	true	1718000001	0.0	0.0
...
```

All four `@test` functions pass:

```
$ kryos test tests/governed_loop_test.kry
  test_two_turn_loop_populates_audit_trail  PASS
  test_budget_exhausted_throws              PASS
  test_strict_alignment_blocks_unregistered_tool  PASS
  test_agent_checkpoint_writes_file         PASS
4 tests, 4 passed, 0 failed
```

---

## Risks and Honest Unknowns

**Risk 1: `time_now_secs()` availability**
The `budget_analyst.kry` example uses `time_now_secs()`. If it is a builtin, latency tracking works. If it is not available on the version of `kryos` installed, set `latency_ms: 0.0` and open an issue. Verify before writing the function.

**Risk 2: `@budget` on a function that calls `chat_tools` vs `_post`**
`std::llm::chat_tools` internally calls `_post` which calls `kryos_budget_try_call`. The `@budget` ceiling on `chat_tools_governed` is therefore enforced correctly (each model call decrements the frame). BUT: if the caller ALSO annotates their own function with `@budget`, BOTH frames are active simultaneously and both are decremented. This is correct semantics (outer budget constrains inner) but callers need to understand it. Document with an example.

**Risk 3: Mock server complexity**
Writing an HTTP server in Kryos that synchronizes correctly with the test (start before test, stop after) may require the test to shell out (`process` capability) or use a fixed port with a retry loop. If this proves difficult, substitute a simpler approach: pre-record the mock response strings and call `_parse_openai_turn` / `_parse_anthropic_turn` directly in the tests. These are private functions in `llm.kry`; you may need to copy them or make them package-visible by moving to a separate module. This approach still exercises all the library logic without requiring a real HTTP call.

**Risk 4: `push` on `audit_trail`**
The `Agent.audit_trail` field is `[AuditEntry]`. Mutation of an array field on a struct received as a function parameter has a known semantic divergence between Cranelift JIT and LLVM AOT (see CLAUDE.md gotcha #23). Use the portable pattern: copy the parameter into a `let mut` local (`let mut ag = agent`) and always return the modified struct, which this design already does via `GovernedResult.agent`.

**Risk 5: `state` field mutation**
`Agent.state` is `i64`. Assigning to a struct field received by value is safe after the local copy. No issue here.

**Risk 6: Latency on tool-call wall time**
`time_now_secs()` returns seconds as `i64`. Sub-second latency (normal for in-process tools like `calc`) will show as `0`. This is a documentation issue, not a functional issue. Post-MVP: add a `time_now_millis()` builtin or use a monotonic clock if available.

**Risk 7: `assert(result.steps == 2)` in tests requires the mock to be deterministic**
The mock must return exactly the same responses on every invocation. Use hardcoded JSON strings, not any randomness. Verify the mock response strings are valid wire format by checking them against the provider's schema.

**Unknown: does `@budget` propagate across function call boundaries into `chat_tools`?**
Based on reading `budget.rs`, `kryos_budget_try_call` checks ALL active frames on the thread, and frames are thread-local. If `chat_tools_governed` has a `@budget` frame active and then calls `chat_tools`, which calls `_post`, which calls `kryos_budget_try_call`, the outer frame is visible. This is the intended behavior -- verify empirically in Step 4's `test_budget_exhausted_throws` test before documenting it as guaranteed.

---

## Depends On

This project does not depend on project 02 (kryos-governed-agent-stdlib-extension). It uses `std::agent` directly as shipped. However, if 02 is built first, the `Tracked<str>` wrapping variant of `chat_tools_governed` becomes trivial to add as a second entry point in the same file.
