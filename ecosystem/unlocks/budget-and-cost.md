# Budget and Cost -- Kryos Language-Level Spending Control

Cluster: budget-and-cost
Source verified: compiler/stdlib/llm.kry, compiler/stdlib/cost.kry,
compiler/crates/kryos-rt/src/budget.rs, compiler/crates/kryos-mir/src/lower.rs,
examples/showcase/budget_analyst.kry, tests/smoke/test_budget_attr.kry,
docs/learn/cookbook/27-llm-chat.md, CHANGELOG v4.45.0 and v4.46.0

---

## What actually exists today (source-verified)

### @budget(tokens=N, calls=M) -- compiler-injected runtime frames

Verified in lower.rs (inject_budget_frames, lines 423-487): the compiler
rewrites the MIR of any annotated function to insert kryos_budget_push(N, M)
at entry and kryos_budget_pop_to(depth) before every return terminator.
The push returns the pre-push stack depth, so pops are self-healing after
exception unwinds -- if a throw bypasses an inner pop, the next outer pop
truncates past the leaked frames. Both axes accept -1 for "unlimited". The
MIR injection is backend-agnostic: identical behavior on Cranelift JIT and
LLVM AOT.

The runtime implementation (kryos-rt/src/budget.rs) uses thread-local
Vec<Frame>. kryos_budget_try_call() pre-charges ALL active frames before the
HTTP request fires; kryos_budget_charge_tokens() charges actual usage after.
Either can throw. Nesting is compositional: an outer @budget(tokens=50000)
constrains an inner @budget(tokens=10000) -- every charge decrements all
active frames simultaneously.

Verified in llm.kry (lines 251-282): std::llm chat() checks
kryos_budget_active(), calls kryos_budget_try_call() before the HTTP send
(refused calls never go out on the wire), and calls kryos_budget_charge_tokens()
after parsing the response. The token count charged is input_tokens +
output_tokens from the actual provider response. An exhausted token budget
throws after the call completes; an exhausted call budget throws before the
call fires.

### std.cost: ComputeCost and Budget

Verified in cost.kry: ComputeCost { wall_time_ms: f64, tokens_used: i64,
api_calls: i64, money_usd: f64, energy_kwh: f64 } is a plain struct with
cost_add() for composition. Budget has explicit limits on usd/tokens/api_calls
and a charge() method that throws BudgetExceeded if any dimension is crossed.
CostTracker records a history of ComputeCost entries alongside a running Budget.

### std.llm chat_within()

Verified in llm.kry (lines 307-321): chat_within() is a higher-level guard
that wraps a chat() call. It checks budget.is_exhausted() before the call,
then charges a ComputeCost of { tokens_used: in+out, api_calls: 1, money_usd: 0.0 }
afterward. USD is explicitly left at 0.0 -- the comment acknowledges per-token
pricing varies; the user is expected to supply rates and call charge() themselves.
This is honest but is also a current limitation.

### What the showcase demonstrates

budget_analyst.kry (verified): @budget(tokens=60000, calls=25) on the analyze()
function. The agent loop has `while not turn.done and rounds < 8` -- but the
@budget attribute means exceeding 25 calls or 60k tokens throws regardless of
the loop condition. The code comment says it explicitly: "A bug that made this
loop infinite would throw on call 26." This is the definitive use case.

test_budget_attr.kry (verified): live end-to-end test with a mock HTTP server.
one_shot_agent has @budget(tokens=1000, calls=1) -- first call succeeds, second
throws before hitting the wire. tiny_token_agent has @budget(tokens=12, calls=5)
-- the response (15 tokens) crosses the 12-token limit, throws on return.
Fresh @budget invocations get fresh frames (frames pop on return).

---

## Novelty analysis

### 1. @budget as a compiler-enforced function attribute -- PARTIAL

Who else does it: LangChain has max_tokens, max_retries, and callback-based
token counters. OpenAI's API has max_tokens per request. Guardrails-ai has
validation policies. OpenTelemetry can meter token consumption. None of these
are function-attribute-level annotations that inject budget enforcement into
the calling function's ABI automatically.

What makes this partial rather than truly-novel: the MECHANISM (thread-local
frame stack, pre/post charge hooks) is not theoretically novel. Python
contextvar-based budget tracking would achieve a similar dynamic. What Kryos
adds is the SYNTACTIC INTEGRATION -- @budget is a first-class compiler
attribute, not a context manager or decorator the developer has to remember.
The injection happens in MIR (lower.rs) so it cannot be bypassed by callee
code; it is not opt-in per-call but opt-in per-function and then automatically
applied to every nested call. Nested compositional enforcement (outer frames
constrain inner frames simultaneously) is not present in any of the
tool/framework approaches above.

The honest distinction: LangChain budgets are advisory -- a runaway loop that
ignores the callback can still continue. Kryos @budget is enforcement -- the
throw is injected by the compiler at the MIR level, not by code the author
wrote, so it cannot be accidentally omitted.

Needs-language-work caveat: currently only tokens and calls are supported axes.
USD spend (requires per-model pricing tables) and energy_kwh are in the
ComputeCost struct but not in @budget. That is a real gap for multi-model
deployments where the "right" ceiling is monetary, not token-count.

### 2. std.cost as a composable first-class value -- PARTIAL

Who else does it: OpenTelemetry instruments cost via spans+metrics but they
are observability side-channels, not values you compute with. Anthropic's own
usage response includes token counts. Cost-per-call tracking is available in
every LLM framework as a logging concern.

What makes this partial: ComputeCost is a value type that participates in
normal Kryos computation -- you can add two costs with cost_add(), track a
history of costs in CostTracker, and return a cost from a function. In Python
or TypeScript you would build a dataclass/object to hold this; Kryos provides
it in stdlib. The difference is integration depth, not conceptual novelty.
Energy tracking (energy_kwh) is notably absent from most frameworks -- this
is the most genuinely under-covered dimension.

### 3. Pre-call refusal (call never fires when budget exhausted) -- TRULY-NOVEL in context

Who else does it: Every other system charges retrospectively or enforces
server-side. LangChain's max_retries blocks retries after failure, not initial
calls. Token budgets in provider APIs set a ceiling on the output, not on
whether the request should be made.

What Kryos does: kryos_budget_try_call() runs BEFORE the HTTP request fires.
Verified in llm.kry line 251-255: if the call budget is exhausted, the throw
happens without network I/O. This is the correct safety behavior for an agent
that has already spent its call budget -- you do not want it to fire another
request at all. This is a small but practically significant behavior: at the
language runtime level, the decision "do not make this call" is made before
any bytes leave the machine.

In the context of agent safety, this is meaningful: an agent that has exhausted
its call budget cannot make one more "accidental" call while the exception
handler is being constructed.

### 4. Nested compositional budget frames -- PARTIAL

Who else does it: Python's contextvars could implement this; some resource
accounting systems (e.g. JVM thread CPU accounting) work similarly at the OS
level. No mainstream LLM framework exposes nested budget composition.

What Kryos adds: @budget(tokens=1000) calling a sub-function with its own
@budget(tokens=200) results in BOTH frames being charged simultaneously. The
outer budget constrains the inner. This is the correct model for a caller that
wants a hard cap regardless of what sub-agents do. Verified in budget.rs
nested_outer_constrains_inner test (line 127-133): charge of 60 against an
outer frame of 50 returns exceeded=1 even when the inner frame is 1000.

### 5. Energy tracking in ComputeCost -- PARTIAL / FORWARD-LOOKING

energy_kwh is in the struct (verified in cost.kry line 13). It is currently
always 0.0 in chat_within() (llm.kry line 316). No charging mechanism exists.
This is a declared surface, not a working feature. Rating as PARTIAL because
the field exists and the intent is clear, but calling it an unlock would be
premature. It becomes truly novel if Kryos ships per-provider energy estimates
-- no mainstream framework tracks energy at this level.

---

## Current limitations (honest)

1. @budget supports only tokens and calls. USD spend is tracked in
   ComputeCost.money_usd but there is no @budget(usd=0.50) attribute today.
   The cookbook doc explicitly says USD accounting is left at 0.0 (llm.kry
   line 302, 27-llm-chat.md line 50-52).

2. Per-model pricing is not in stdlib. To use money_usd meaningfully, the
   developer must supply per-token rates. This is honest for a language
   stdlib (rates change constantly) but means the money axis requires manual
   wiring.

3. @budget is per-function, per-call-of-that-function. There is no
   session-level or process-level budget enforcer -- a program that calls
   an @budget function 1000 times would run each invocation fresh. For
   process-level spend caps, the developer uses CostTracker manually or wraps
   multiple calls inside a single @budget-annotated outer function.

4. energy_kwh is unimplemented in any charge path.

5. Capability system is opt-in today (annotated fns checked; unannotated
   unconstrained). Deny-by-default needs language work. Budget enforcement
   has the same shape: it works when the annotation is applied; unannotated
   agent loops are unconstrained.

---

## Proposed additions

### @budget(usd=0.50, tokens=50000, calls=25)

Add USD as a first-class budget axis. Would require a pricing table in stdlib
-- either hardcoded (acceptable for top-5 models, updated with stdlib) or
injected at runtime via environment:

    KRYOS_COST_PER_INPUT_TOKEN=0.000003
    KRYOS_COST_PER_OUTPUT_TOKEN=0.000015

The MIR injector already handles optional axes (missing axis = -1 = unlimited);
adding usd follows the same pattern. The charge would happen in
kryos_budget_charge_tokens using a per-call rate set at push time.

Novelty if shipped: TRULY-NOVEL. No mainstream language has @budget(usd=0.50)
as a compile-enforced annotation that halts a function when money is spent.

### Cost-typed function signatures

    fn expensive_analysis(data: str) -> (str, ComputeCost)

Make it idiomatic to return cost alongside results. A budget_map() HOF that
accumulates cost across an array of items:

    fn budget_map<T, U>(items: [T], budget: Budget, f: fn(T) -> (U, ComputeCost)) -> ([U], Budget)

This enables pipeline-level cost tracking without threading a tracker through
every call. The types exist today; the HOF does not.

### Budget composition for multi-agent programs

    fn delegate_with_budget(budget: Budget, f: fn() -> str) -> (str, Budget)

A wrapper that shares the remaining budget with a sub-function -- if the
sub-function has its own @budget annotation, the effective limit is min(outer
remaining, inner declared). Today you achieve this by making the outer function
@budget-annotated and calling the inner; but for dynamic budget allocation
(e.g. "give this sub-agent 30% of remaining budget"), there is no first-class
mechanism.

### Energy estimation in chat_within

Add per-provider energy estimates to LlmConfig:

    energy_per_token_kwh: f64   // 0.0 = unknown, provider can set

Then charge energy_kwh in chat_within alongside tokens. Even rough estimates
(0.0003 kWh per 1k tokens is a published ballpark for inference) make
energy tracking useful for sustainability reporting, which has emerging
regulatory relevance (EU AI Act energy disclosure provisions).

---

## Example use cases unlocked today

### Runaway-agent halting (buildable today)

    @budget(tokens = 50000, calls = 20)
    fn research_agent(question: str) -> str {
        let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
        let mut notes = ""
        while true {
            // This loop is infinite by design; @budget makes it safe.
            // The 21st call throws "llm error: @budget exhausted: no model calls left".
            let r = chat(cfg, [user(question + notes)])
            notes = notes + r.text
        }
        return notes
    }

No equivalent in Python/LangChain without explicit check_budget() calls in the loop.
The Kryos version is safe by default for anyone who adds @budget; the throw propagates
out of the loop naturally.

### Per-call billing (buildable today with manual USD)

    fn bill_user(user_id: str, question: str, rate_per_token: f64) -> (str, f64) {
        let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
        let mut budget = budget_new(10.0, 100000, 50)
        let out = chat_within(cfg, [user(question)], budget)
        let tokens = out.response.input_tokens + out.response.output_tokens
        let charge = float(tokens) * rate_per_token
        return (out.response.text, charge)
    }

The charge value is a plain Kryos f64 you can pass to a billing system.
ComputeCost records it. This works today but USD must be computed manually.

### Cost-aware pipeline (buildable today)

    fn summarize_batch(docs: [str]) -> (str, ComputeCost) {
        let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
        let mut total = cost_zero()
        let mut summaries: [str] = []
        for doc in docs {
            let out = chat_within(cfg, [user("Summarize: " + doc)], budget_new(5.0, 10000, 1))
            let c = ComputeCost { wall_time_ms: 0.0, tokens_used: out.response.input_tokens + out.response.output_tokens, api_calls: 1, money_usd: 0.0, energy_kwh: 0.0 }
            total = cost_add(total, c)
            summaries = push(summaries, out.response.text)
        }
        // total is a value: log it, return it, gate on it.
        return (summaries[0], total)
    }

---

## Positioning summary

The @budget attribute + runtime hooks is the most honest answer to "what if
agent loops could not run away?" in a production language. The closest analogs
are:

- LangChain: advisory callbacks, not enforced at call site
- Guardrails-ai: schema/output validators, not call-count/token enforcers
- OpenTelemetry: observability side-channel, not in-process enforcement
- Wasm sandbox + metering: can count instructions but has no LLM-aware token
  concept; requires wasm target and host cooperation
- Custom Python context managers: can work but must be manually applied;
  no compiler injection

Kryos's @budget is partial-novel (the mechanism has analogs) but uniquely
integrated at the compiler-attribute level. The claim "compiler-enforced
spending ceilings" is accurate and verifiable in lower.rs. The claim
"cannot exceed N calls no matter what the loop logic says" is accurate and
demonstrated in the test file.

The money and energy axes are the gap. Shipping @budget(usd=0.50) would
move this cluster from "partial novel" to "truly novel" for the AI agent
governance use case.
