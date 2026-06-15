# Kryos Unlock Analysis: Resilience and Fusion

**Cluster:** resilience-and-fusion
**Source read:** compiler/stdlib/{circuit,backoff,ratelimit,semaphore,cost,tracked,probable,agent,llm}.kry + docs/10-capabilities.md
**Date:** 2026-06-14

---

## What the Resilience stdlib Actually Is

Reading the source directly:

- `circuit.kry` - state machine over a `[i64; 6]` array: CLOSED/OPEN/HALF_OPEN with configurable failure threshold and cooldown. Pure functions: `new_breaker`, `allow`, `record_success`, `record_failure`. No async, no callbacks, no integration with anything else.
- `backoff.kry` - two pure math functions: `next_delay` (doubles prev, caps at max) and `total_delay` (sums a retry series). Returns the delay value; caller sleeps. No integration.
- `ratelimit.kry` - token bucket over `[i64; 4]`: capacity, current tokens, refill rate, last refill timestamp. `try_acquire` refills by elapsed time then decrements. Pure; single-threaded (comment says pair with sync for multi-thread).
- `semaphore.kry` - counting semaphore over `[i64]`. `try_acquire` / `release`. Comment: single-threaded; pair with `std::sync` atomics for multi-threaded use.

These are competent implementations. They are NOT integrated with each other or with the safety primitives at the library level - composition is manual, left to the caller. That is actually the honest picture and also where the fusion thesis gets interesting.

---

## Novelty Assessment: Resilience Primitives in Isolation

**Novelty: HYPE (in isolation)**

Circuit breaker, exponential backoff, token bucket rate limiter, and counting semaphore exist in every major language:
- Go: `sony/gobreaker` (circuit), `golang.org/x/time/rate` (bucket), `sync.WaitGroup`/channels for semaphore
- Rust: `tokio`, `tower` (circuit, rate, semaphore), `governor` (GCRA rate limiter)
- Python: `tenacity` (retry/backoff), `circuitbreaker`, `aiohttp` limiters
- TypeScript: `opossum` (circuit), `bottleneck` (rate), `p-limit` (concurrency)

None of Kryos's four resilience primitives do anything a third-party library in another language cannot do. The implementations are simple and correct. The only differentiation is that they ship with the language rather than being installed separately - but that is a packaging advantage, not a language-level novelty.

---

## The Fusion Thesis: Where It Gets Genuinely Interesting

The question is what becomes possible when you COMBINE the resilience primitives with the safety primitives in one language. I'll assess three candidate fusions honestly.

---

### Fusion 1: Capability-Bounded Resilience (PARTIAL novelty, buildable today with caveats)

**The idea:** A circuit-breaker or retry loop that is statically proven to be network-isolated. A function whose signature declares `@capabilities(net)` and whose body uses `circuit.allow()` before calling LLM APIs means the `@capabilities` annotation on that function documents, at the function signature, that ALL network use within this retry envelope is constrained to whatever the caller declared. Attenuation means a caller with `@capabilities(net)` cannot hand control to a callee that escalates to `net, io`.

**What this looks like in practice:**
```kryos
@capabilities(net)
fn resilient_llm_call(cfg: LlmConfig, msgs: [Message], breaker: [i64]) -> str {
    if not allow(breaker, time_now()) {
        throw "circuit open: upstream LLM unavailable"
    }
    try {
        let r = chat(cfg, msgs)    // @capabilities(net) - same set, no escalation
        record_success(breaker)
        return r.text
    } catch e {
        record_failure(breaker)
        throw e
    }
}
```

The `@capabilities(net)` on the signature is checked against the full call graph. If someone accidentally added a `file_write` inside the retry body, the compiler would catch it (because `file_write` requires `io`, which is not declared).

**Who else does it:** Wasm Component Model (WASI) gives capability confinement at the module boundary but not at the function level within a module. Rust's `#[target_feature]` is entirely different. Java's SecurityManager (deprecated in 17, removed in 24) was coarser-grained and runtime-only. Nothing in mainstream languages gives you function-level capability attestation that the compiler enforces through a retry/circuit wrapper.

**Honest caveat:** This works today ONLY for annotated functions. An unannotated function wrapping the same circuit breaker is unconstrained - it can call `file_write` freely. Deny-by-default (`--strict-capabilities`) is NOT implemented. So the "static proof" claim is partial: it holds if the author bothers to annotate, but the compiler does not force annotation today.

**Buildable today:** Yes, with the partial-enforcement caveat. Useful as documentation and opt-in enforcement right now.

---

### Fusion 2: Budget-Bounded Resilience with Cost Tracking (TRULY NOVEL - no mainstream language does this integrated)

**The idea:** A retry loop or circuit breaker that has a hard token/call budget enforced by the RUNTIME before each attempt, and which accumulates a `ComputeCost` struct across all retries, so the caller gets back both the result AND the exact cost of the resilience envelope.

This is the genuinely never-been-done combination. Here is the mechanism:

1. `@budget(tokens=N, calls=M)` on the outer function installs a budget frame
2. `std.llm.chat()` calls `kryos_budget_try_call()` BEFORE each HTTP request and `kryos_budget_charge_tokens()` AFTER
3. `std.cost.CostTracker.record_api_call()` accumulates the cost across retries
4. The backoff loop uses `std.backoff.next_delay()` between retries
5. If any retry causes the @budget frame to run out, the runtime throws - the loop cannot exceed its declared envelope, even across N retries

The result: a function that carries in its SIGNATURE `@budget(tokens=50000, calls=5)` where you can read "this retry envelope will make at most 5 LLM calls consuming at most 50,000 tokens total, and it will throw before exceeding that". No mainstream language has this. You cannot write this in Python without a hand-rolled decorator that the type system and runtime jointly enforce. You cannot write it in Go. You cannot write it in TypeScript. Rust effects crates (e.g. `effects-as-capabilities`) exist as research but are not shipping.

**What this looks like:**
```kryos
use std::cost::{Budget, CostTracker, ComputeCost, budget_new, cost_tracker_new, cost_zero}
use std::backoff::{next_delay}

@capabilities(net)
@budget(tokens=50000, calls=5)
fn resilient_with_budget(cfg: LlmConfig, prompt: str) -> str {
    let mut delay = 0
    let mut i = 0
    while i < 5 {
        try {
            // kryos_budget_try_call() fires inside chat() before the HTTP call
            // kryos_budget_charge_tokens() fires inside chat() after the response
            let r = chat(cfg, [user(prompt)])
            return r.text
        } catch e {
            if i >= 4 { throw e }
            delay = next_delay(delay, 100, 5000)
            // sleep(delay) -- caller's responsibility to sleep
            i = i + 1
        }
    }
    throw "unreachable"
}
```

The `@budget(tokens=50000, calls=5)` is NOT documentation - it is an enforced runtime contract. The `chat()` internals check the budget frame before and after every call. If retry 3 would push over 50,000 tokens total, the runtime throws before making the HTTP request.

**Who else does this:** Nobody shipping today. OpenTelemetry tracks costs as observability, after the fact. LangChain has `max_iterations` on agents but no token-level enforcement that throws. CrewAI, AutoGen, LangGraph - none have language-level budget enforcement that the runtime makes uncatchable. The closest prior art is AWS Lambda timeout (kills the process) and Cloudflare Workers CPU time limit (kills the isolate) - both are process-level kills, not function-level throw-with-composable-cost.

**Buildable today:** YES. `chat()` already has the `kryos_budget_active() / kryos_budget_try_call() / kryos_budget_charge_tokens()` hooks wired in. `Budget.charge()` in `std.cost` already throws on exceeded limits. The @budget attribute is described as existing with runtime hooks. This fusion works with stdlib as-is.

---

### Fusion 3: Confidence-Gated Circuit Breaker with Provenance (PARTIAL novelty)

**The idea:** A pipeline where each step returns `Probable<T>` (confidence-aware) and `Tracked<T>` (provenance-carrying), and a circuit breaker gates further calls when confidence falls below a threshold - using the circuit's HALF_OPEN state as a "probe with low confidence" mode.

**The mechanism:**
```kryos
fn confidence_gated_call(
    cfg: LlmConfig,
    prompt: str,
    breaker: [i64],
    min_confidence: f64
) -> Tracked<Probable<str>> {
    let now = time_now()
    if not allow(breaker, now) {
        // Return a tracked "circuit open" result with zero confidence
        let p = probable("", 0.0)
        return tracked_source(p, "circuit-breaker", "upstream unavailable, circuit OPEN")
    }
    try {
        let r = chat(cfg, [user(prompt)])
        // Confidence derived from model certainty signals (simplified here)
        let conf = if r.output_tokens < 10 { 0.3 } else { 0.85 }
        let p = probable(r.text, conf)
        let t = tracked_source(p, "llm:" + r.model, prompt)
        if conf < min_confidence {
            record_failure(breaker)
        } else {
            record_success(breaker)
        }
        return transform(t, p, "confidence-gate", "threshold=" + to_string(min_confidence))
    } catch e {
        record_failure(breaker)
        throw e
    }
}
```

The RESULT of this function is auditable: call `explain(result)` and you see every step including when and why the circuit tripped, what model responded, what confidence the output carries.

**Who else does this:** LangChain and LangSmith do observability/tracing. No mainstream agent framework has confidence as a first-class composable value that gates circuit behavior at the language level. HOWEVER - this is achievable by wrapping any Python circuit breaker with a custom confidence-tracker class. It is cleaner in Kryos because `Probable<T>` and `Tracked<T>` are stdlib generics not third-party, but the concept is not architecturally new.

**Honest rating:** PARTIAL. The cleanness of having `Probable<Tracked<T>>` as a stdlib type that composes with circuit.allow() without any glue code is real differentiation. But the underlying concept - gate retries on quality signals, trace the lineage - is known and implementable elsewhere.

**Buildable today:** YES. All four types (`Probable`, `Tracked`, circuit, backoff) are in stdlib. No language work needed.

---

## The Biggest Fusion Unlock: Certified Retry Envelopes

Combining all three themes gives the clearest genuinely-novel unlock:

A Kryos function can have a SIGNATURE that statically certifies:
1. It uses network capability only (not filesystem, not process spawn) -- via `@capabilities(net)`
2. It will spend at most N tokens and M calls across ALL retries (including backoff) -- via `@budget(tokens=N, calls=M)` + runtime hooks in `chat()`
3. Its output carries confidence provenance (what model, when, with what confidence) -- via `Tracked<Probable<str>>` return type

No other shipping language can express all three simultaneously in one function signature, enforced by the compiler (capabilities) and runtime (budget). This is the headline differentiator for Kryos as "the language for trustworthy AI agent software."

The key word is "CERTIFIED" - not logged-after-the-fact, not suggested-by-convention, but enforced at compile time (capabilities/attenuation) and at runtime-with-throw (budget). The function call itself is the proof.

---

## Honest Limitations to Flag

1. **Capabilities are opt-in today.** An unannotated function wrapping these primitives has no enforcement. The "static proof" claim only holds if the author annotates. `--strict-capabilities` deny-by-default is PLANNED, not shipped.

2. **`@budget` extern hooks are wired in llm.kry** (`kryos_budget_active`, `kryos_budget_try_call`, `kryos_budget_charge_tokens`) but the attribute syntax and runtime frame management are described without a separate implementation file visible in stdlib. The hooks exist; whether the attribute parser and frame stack are complete should be verified before claiming to customers.

3. **Resilience primitives are single-threaded.** `semaphore.kry` and `ratelimit.kry` both say "pair with std::sync atomics for multi-threaded use." An agent that spawns goroutine-style concurrent tasks would need extra wiring for safe shared circuit state.

4. **No sub-capabilities yet.** `@capabilities(net)` does not yet distinguish between `net:http` and `net:raw_socket`. The docs describe sub-capabilities but the compiler does not enforce them. Claiming "network-isolated" today means "uses net but not io/process/ffi" - which is meaningful but less precise than `@capabilities(net:http)` would be.

5. **`Probable<T>` confidence is caller-assigned, not model-assigned.** The confidence value in `probable(value, 0.85)` is whatever the programmer puts in. There is no integration with actual model logprobs or calibration. For the "calibrated output" pitch to be real, the LLM call would need to extract token probabilities and set confidence accordingly - which is doable but not wired in stdlib today.

---

## Proposed Kryos Functions That Would Cement This Story

These do not require new language features - they compose existing stdlib:

### `resilient_chat` (net capability, budget-bounded, cost-accumulating retry)
```kryos
@capabilities(net)
@budget(tokens=100000, calls=10)
fn resilient_chat(
    cfg: LlmConfig,
    msgs: [Message],
    breaker: [i64],
    tracker: CostTracker,
    max_retries: i64
) -> (str, CostTracker)
```
Returns the reply AND the updated CostTracker so the caller can see exactly what the resilience envelope spent across all retries. Throws if the @budget frame exhausts before success.

### `tracked_complete` (provenance-carrying one-shot)
```kryos
@capabilities(net)
fn tracked_complete(
    cfg: LlmConfig,
    prompt: str,
    breaker: [i64]
) -> Tracked<Probable<str>>
```
Returns a value where every consumer can call `explain(result)` and see: source model, timestamp of call, whether circuit was open, confidence estimate. The lineage is attached to the value, not in a separate log.

### `budget_gate` (pre-call guard composable with any operation)
```kryos
fn budget_gate(budget: Budget, estimated_tokens: i64, estimated_calls: i64) -> Budget
```
Throws BEFORE spending if the budget cannot absorb the estimated cost. Composes with any loop, not just LLM calls. Pure; no capabilities needed.
