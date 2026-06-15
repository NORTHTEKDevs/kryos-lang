# 07 -- kryos-bench-governed: Budget-Bounded Benchmark Harness

**One-line pitch:** An LLM evaluation harness where the benchmark budget is a `@budget` attribute on the runner function, not a shell timeout -- the harness provably cannot exceed the token/call ceiling because the compiler and runtime enforce it before any request fires.

---

## Why This Is Novel (Honest Novelty Rating)

### The core claim

Every existing LLM eval framework (HELM, lm-evaluation-harness, inspect-ai, LangSmith) enforces spending limits through external controls: shell timeouts, middleware rate-limiters, or manual token-counting checks scattered through Python call sites. These controls sit OUTSIDE the language. A bug in the eval loop, an off-by-one in a counter, or an unhandled exception can bypass them. The ceiling is convention, not a property of the program.

Kryos makes the ceiling a language property. `@budget(tokens=50000, calls=100)` on the benchmark runner is not a comment or a decorator that runs advisory code -- it installs a budget frame in the runtime before the function body executes. Every `chat()` call inside (no matter how deep in the call graph) pre-checks `kryos_budget_try_call()` before sending the HTTP request. If the ceiling is already hit, the call throws without ever touching the network. Token consumption is charged post-call via `kryos_budget_charge_tokens(in + out)`. If a response would cross the token ceiling, it throws immediately after the call returns. The ceiling holds even if the eval loop has a bug.

### Novelty ratings per axis

**`@budget` as a compiler/runtime language attribute -- TRULY-NOVEL**
No mainstream language or mainstream eval framework offers this as a first-class language feature. Python eval frameworks use manual accounting. TypeScript/Node has no analogous mechanism. Rust has no first-class token-budget concept. The closest analog is structured concurrency (cancel tokens, nurseries) in Python/Trio or Swift Concurrency -- but those cancel execution, they do not enforce spending ceilings on API calls. Kryos `@budget` is unique in that the budget frame is set up by the compiler (via the attribute) and enforced by runtime hooks wired into the standard library (std.llm).

**Wrapping model responses in `Probable<str>` -- PARTIAL**
Confidence-aware types exist in probabilistic programming frameworks (Pyro, Stan outputs, uncertainty-quantification libraries). What is partial-novel here is that `Probable<T>` is a first-class generic stdlib type in the language, so confidence propagates as a value through Kryos code, not through a separate framework layer. The ensemble/majority-vote logic over `[Probable<str>]` is then plain Kryos.

**`ComputeCost` as a composable value -- PARTIAL**
OpenTelemetry, LangSmith, and Weights & Biases all track token costs. What Kryos adds is that `ComputeCost` is a stdlib struct you `cost_add()` and accumulate, composable in pure Kryos without a tracing SDK or side-channel instrumentation. The `CostTracker` with `Budget.charge()` is closer to a typed ledger than a telemetry hook.

**`Tracked<str>` provenance on benchmark output -- PARTIAL**
MLflow, LangSmith, and similar tools log lineage externally. Kryos `Tracked<T>` carries lineage as a field on the value itself, making it available without a side-channel store. In a benchmark context, the final result value IS the audit trail.

**Pre-call refusal (budget enforced before the request fires) -- TRULY-NOVEL**
This specific property -- that the harness provably cannot overspend because the check runs before the HTTP request -- is not offered by any eval framework. It is a consequence of the `@budget` attribute being wired into std.llm's `chat()` implementation via `kryos_budget_try_call()`.

### Who else does it / why Kryos is the right substrate

Python eval frameworks enforce budgets through accounting done by the programmer: subtract from a counter, check before each call, hope no exception path skips the check. The ceiling is convention. Kryos makes it a proof obligation satisfied by the language runtime. For regulated-industry customers (finance, healthcare, government) who need a cost-predictability guarantee they can point to in an audit, the difference matters.

---

## Which Kryos Primitives This Uses

### `@budget(tokens=N, calls=M)` -- std.llm runtime hooks

Source: `compiler/stdlib/llm.kry` lines 32-38, 251-283.

The attribute is placed on the benchmark runner function. Every `chat()` inside:
- Pre-call: calls `kryos_budget_try_call()` -- if calls are exhausted, throws before sending HTTP.
- Post-call: calls `kryos_budget_charge_tokens(input_tokens + output_tokens)` -- if tokens are now exceeded, throws immediately after the response is parsed.

This is the budget enforcement mechanism already wired into std.llm. No extra code is required in the benchmark to get pre-call refusal.

### `std::llm` -- `chat()`, `openai_config()`, `anthropic_config()`, `with_base_url()`

Source: `compiler/stdlib/llm.kry`.

Used to send each benchmark case to the model. The harness calls `chat()` in a loop over test cases; the `@budget` frame catches runaway spending at the function level, not per-call.

### `std::probable` -- `Probable<str>`, `probable()`, `best_of()`, `majority_vote()`

Source: `compiler/stdlib/probable.kry`.

Each model response is wrapped in a `Probable<str>`. For ensemble runs (multiple calls per case), `majority_vote()` reduces `[Probable<str>]` to a single prediction weighted by confidence. `best_of()` picks the highest-confidence response for single-winner selection.

The confidence score for each reply comes from log-probability if the provider exposes it, or from a secondary scoring call, or from a fixed heuristic (e.g. 0.8 for a clean response, 0.5 for an ambiguous one). The MVP uses a fixed heuristic or a lightweight scoring prompt.

### `std::cost` -- `ComputeCost`, `CostTracker`, `budget_new()`, `cost_add()`

Source: `compiler/stdlib/cost.kry`.

Accumulates per-case cost across the benchmark run. The `CostTracker` records one `ComputeCost` per case and sums to a run total. `Budget.charge()` throws on overage (this is the std.cost enforcement path, separate from but complementary to the `@budget` attribute path).

### `std::tracked` -- `Tracked<str>`, `tracked_source()`, `inference()`, `annotate()`, `explain()`

Source: `compiler/stdlib/tracked.kry`.

The final result for each benchmark case is wrapped in `Tracked<str>` carrying: the original prompt, the model used, the reply, and any intermediate steps (tool calls, retries). `explain()` renders the lineage. `to_json()` exports it for downstream analysis.

### `@capabilities(net)` -- on any function that calls `chat()`

The `@capabilities(net)` annotation appears on `chat()` in std.llm. Any function that transitively calls `chat()` acquires the `net` capability. The benchmark runner must carry `@capabilities(net)` (or call from a context that does). This is an existing compile-time check, not new work.

### Language work needed first

None for the MVP. Every primitive listed above -- `@budget`, `std::llm`, `std::probable`, `std::cost`, `std::tracked` -- is confirmed implemented in the current repo (read from source). The generic `Probable<T>` and `Tracked<T>` are available since v4.47. `majority_vote()` is implemented in probable.kry. `CostTracker` and `Budget.charge()` are implemented in cost.kry.

**Honest limitations to keep in mind:**
- Deny-by-default capabilities and sub-capabilities (e.g. `net:https_only`) are NOT yet implemented. The `@capabilities(net)` annotation is opt-in on annotated functions; unannotated functions are unconstrained.
- `majority_vote()` uses string equality (`pj.value == pi.value`) for consensus. For free-form model replies, this means exact-string matching -- not semantic equivalence. For classification tasks this is fine. For open-ended generation you need a judge function or a clustering step (out of MVP scope).
- `float()` is used in cost.kry line 156 (`float(count)`). The current CLAUDE.md does not list `float()` as a builtin; `(count as f64)` is the idiomatic cast. The stdlib source uses it, so it works -- but prefer `(count as f64)` in new code.
- The `@budget` attribute currently caps calls and tokens globally per invocation of the annotated function. It does NOT yet support per-case sub-budgets (e.g. `@budget(tokens=500)` on an inner per-case function) with separate frames. Sub-budgets require nesting -- call a second `@budget`-annotated function from within the outer one.

---

## Architecture

### Components

```
bench.kry                  -- entry point, CLI parsing, run coordination
  BenchCase                -- struct: prompt, expected (Option<str>), case_id
  BenchResult              -- struct: case_id, answer: Probable<str>, cost: ComputeCost,
                              audit: Tracked<str>, latency_ms: i64
  run_benchmark(...)       -- @budget annotated; evaluates all cases
  evaluate_case(...)       -- evaluates one case; returns BenchResult
  ensemble_case(...)       -- evaluates one case N times; returns majority-voted result
  score_run(...)           -- computes ECE and accuracy if ground truth provided
  format_report(...)       -- renders the final cost/confidence/score table to str
  main()                   -- parses args, loads cases, calls run_benchmark, prints report
```

### Data model (Kryos structs)

```kryos
use std::probable::{Probable, probable, majority_vote, best_of, is_confident, with_source}
use std::cost::{ComputeCost, CostTracker, Budget, budget_new, cost_zero, cost_add}
use std::tracked::{Tracked, tracked_source, inference, annotate, explain}
use std::llm::{LlmConfig, openai_config, anthropic_config, with_base_url,
               with_max_tokens, system, user, chat, Message}

@copy
struct BenchCase {
    case_id: str,
    prompt: str,
    expected: str,       // "" means no ground truth
    category: str        // optional tag for grouping
}

@copy
struct BenchResult {
    case_id: str,
    answer: Probable<str>,
    cost: ComputeCost,
    latency_ms: i64,
    audit: Tracked<str>
}
```

Note: `Probable<str>` and `Tracked<str>` contain str fields which are NOT `@copy` (heap-bearing). Use these structs as move values; do not annotate `BenchResult` with `@copy`.

### Key functions

```kryos
// The budget ceiling is enforced around the entire run.
// A buggy loop that generates 10000 cases cannot exceed 50000 tokens.
@budget(tokens = 50000, calls = 100)
@capabilities(net)
fn run_benchmark(cfg: LlmConfig, cases: [BenchCase], sys_prompt: str) -> [BenchResult] {
    let mut results: [BenchResult] = []
    for c in cases {
        let result = evaluate_case(cfg, c, sys_prompt)
        results = push(results, result)
    }
    return results
}

@capabilities(net)
fn evaluate_case(cfg: LlmConfig, c: BenchCase, sys_prompt: str) -> BenchResult {
    let t0 = time_now()
    let msgs: [Message] = [system(sys_prompt), user(c.prompt)]
    let resp = chat(cfg, msgs)
    let t1 = time_now()
    let latency = t1 - t0
    let conf = infer_confidence(resp.text, c.expected)
    let answer = with_source(probable(resp.text, conf), cfg.model)
    let used = ComputeCost {
        wall_time_ms: latency as f64,
        tokens_used: resp.input_tokens + resp.output_tokens,
        api_calls: 1,
        money_usd: 0.0,
        energy_kwh: 0.0
    }
    let mut audit = tracked_source(c.prompt, "user", "benchmark case " + c.case_id)
    audit = inference(audit, cfg.model, resp.text, conf)
    return BenchResult {
        case_id: c.case_id,
        answer: answer,
        cost: used,
        latency_ms: latency,
        audit: audit
    }
}

// Heuristic confidence: if ground truth is provided, 1.0 on exact match,
// 0.5 otherwise. If no ground truth, fixed 0.8 (open-ended).
fn infer_confidence(reply: str, expected: str) -> f64 {
    if expected == "" {
        return 0.8
    }
    if reply == expected {
        return 1.0
    }
    if contains(reply, expected) {
        return 0.85
    }
    return 0.3
}
```

### Ensemble variant (N runs per case, majority vote)

```kryos
// Run each case n_runs times; majority-vote the answers.
// The @budget ceiling still holds across all n_runs * len(cases) calls.
@budget(tokens = 50000, calls = 100)
@capabilities(net)
fn run_ensemble(cfg: LlmConfig, cases: [BenchCase], sys_prompt: str, n_runs: i64) -> [BenchResult] {
    let mut results: [BenchResult] = []
    for c in cases {
        let mut predictions: [Probable<str>] = []
        let mut total_cost = cost_zero()
        let t0 = time_now()
        let mut i = 0
        while i < n_runs {
            let resp = chat(cfg, [system(sys_prompt), user(c.prompt)])
            let conf = infer_confidence(resp.text, c.expected)
            predictions = push(predictions, with_source(probable(resp.text, conf), cfg.model))
            let used = ComputeCost {
                wall_time_ms: 0.0,
                tokens_used: resp.input_tokens + resp.output_tokens,
                api_calls: 1,
                money_usd: 0.0,
                energy_kwh: 0.0
            }
            total_cost = cost_add(total_cost, used)
            i = i + 1
        }
        let winner = majority_vote(predictions)
        let latency = time_now() - t0
        let mut audit = tracked_source(c.prompt, "user", "benchmark case " + c.case_id)
        audit = inference(audit, cfg.model, winner.value, winner.confidence)
        results = push(results, BenchResult {
            case_id: c.case_id,
            answer: winner,
            cost: total_cost,
            latency_ms: latency,
            audit: audit
        })
    }
    return results
}
```

### Scoring (ECE + accuracy when ground truth is provided)

```kryos
@copy
struct RunScore {
    n_cases: i64,
    n_correct: i64,
    accuracy: f64,
    avg_confidence: f64,
    ece: f64,               // expected calibration error, lower is better
    total_tokens: i64,
    total_calls: i64,
    total_latency_ms: i64
}

fn score_run(results: [BenchResult], cases: [BenchCase]) -> RunScore {
    let n = len(results)
    if n == 0 {
        return RunScore { n_cases: 0, n_correct: 0, accuracy: 0.0, avg_confidence: 0.0,
                          ece: 0.0, total_tokens: 0, total_calls: 0, total_latency_ms: 0 }
    }
    let mut correct = 0
    let mut conf_sum = 0.0
    let mut token_sum = 0
    let mut call_sum = 0
    let mut lat_sum = 0
    // ECE: bucket predictions by confidence decile, compare to accuracy per bucket
    // Simplified: two buckets (confident >= 0.7 vs uncertain < 0.7)
    let mut hi_n = 0
    let mut hi_correct = 0
    let mut lo_n = 0
    let mut lo_correct = 0
    let mut i = 0
    while i < n {
        let r = results[i]
        let c = cases[i]
        let is_correct_val = infer_confidence(r.answer.value, c.expected) >= 0.85
        if is_correct_val { correct = correct + 1 }
        conf_sum = conf_sum + r.answer.confidence
        token_sum = token_sum + r.cost.tokens_used
        call_sum = call_sum + r.cost.api_calls
        lat_sum = lat_sum + r.latency_ms
        if r.answer.confidence >= 0.7 {
            hi_n = hi_n + 1
            if is_correct_val { hi_correct = hi_correct + 1 }
        } else {
            lo_n = lo_n + 1
            if is_correct_val { lo_correct = lo_correct + 1 }
        }
        i = i + 1
    }
    let acc = (correct as f64) / (n as f64)
    let avg_conf = conf_sum / (n as f64)
    let mut ece = 0.0
    if hi_n > 0 {
        let hi_acc = (hi_correct as f64) / (hi_n as f64)
        let hi_conf_avg = 0.85    // midpoint heuristic for hi bucket
        ece = ece + ((hi_n as f64) / (n as f64)) * abs(hi_conf_avg - hi_acc)
    }
    if lo_n > 0 {
        let lo_acc = (lo_correct as f64) / (lo_n as f64)
        let lo_conf_avg = 0.45
        ece = ece + ((lo_n as f64) / (n as f64)) * abs(lo_conf_avg - lo_acc)
    }
    return RunScore {
        n_cases: n,
        n_correct: correct,
        accuracy: acc,
        avg_confidence: avg_conf,
        ece: ece,
        total_tokens: token_sum,
        total_calls: call_sum,
        total_latency_ms: lat_sum
    }
}
```

### Report output (text table)

```kryos
fn format_report(results: [BenchResult], score: RunScore) -> str {
    let mut out = "=== kryos-bench-governed run report ===\n\n"
    out = out + "Cases:        " + to_string(score.n_cases) + "\n"
    out = out + "Correct:      " + to_string(score.n_correct) + "\n"
    out = out + "Accuracy:     " + to_string(score.accuracy) + "\n"
    out = out + "Avg conf:     " + to_string(score.avg_confidence) + "\n"
    out = out + "ECE:          " + to_string(score.ece) + "\n"
    out = out + "Total tokens: " + to_string(score.total_tokens) + "\n"
    out = out + "Total calls:  " + to_string(score.total_calls) + "\n"
    out = out + "Total ms:     " + to_string(score.total_latency_ms) + "\n"
    out = out + "\nPer-case results:\n"
    for r in results {
        let conf_str = to_string(r.answer.confidence)
        let lat_str = to_string(r.latency_ms)
        let tok_str = to_string(r.cost.tokens_used)
        out = out + "  [" + r.case_id + "] conf=" + conf_str +
              " tokens=" + tok_str + " ms=" + lat_str + "\n"
        out = out + "    -> " + r.answer.value + "\n"
    }
    return out
}
```

### CLI entrypoint sketch

```kryos
fn main() {
    let argv = args()
    let key = env_get("OPENAI_API_KEY")
    let anth = env_get("ANTHROPIC_API_KEY")
    let base = env_get("BENCH_BASE_URL")

    // Cases can be loaded from a JSON file or hardcoded for the MVP.
    // MVP: inline cases so the program runs with zero setup beyond an API key.
    let cases: [BenchCase] = [
        BenchCase { case_id: "01", prompt: "What is the capital of France?", expected: "Paris", category: "geography" },
        BenchCase { case_id: "02", prompt: "2 + 2 = ?", expected: "4", category: "math" },
        BenchCase { case_id: "03", prompt: "Name three primary colors.", expected: "", category: "knowledge" }
    ]

    let mut cfg = openai_config(key, "gpt-4o-mini")
    if anth != "" {
        cfg = anthropic_config(anth, "claude-haiku-4-5")
    }
    if base != "" {
        cfg = with_base_url(openai_config("", "llama3"), base)
    }
    cfg = with_max_tokens(cfg, 256)

    let results = run_benchmark(cfg, cases, "Answer concisely. One sentence maximum.")
    let score = score_run(results, cases)
    println(format_report(results, score))
}
```

---

## MVP Scope vs Full Vision

### MVP (smallest shippable slice, ~300 lines)

- Single `bench.kry` file; no kryos.toml dependencies needed beyond stdlib.
- `run_benchmark()` with `@budget(tokens=50000, calls=100)`.
- Inline hardcoded cases (5-10 cases covering classification, factual Q&A, math).
- `infer_confidence()` heuristic (exact-match = 1.0, contains = 0.85, else 0.3).
- `Probable<str>` wrapping on each reply.
- `ComputeCost` accumulation per case.
- `Tracked<str>` audit on each result.
- Text report with accuracy, avg confidence, token totals.
- Works against any OpenAI-compatible endpoint; also Anthropic if key is set.
- Offline/dry-run mode: no API key = prints the case list and the budget ceiling, exits cleanly.
- Total: ~300 lines.

### Full vision (post-MVP)

- JSON case loader: `bench_cases.json` with prompt/expected/category/weight fields.
- Sub-budgets via nested `@budget`-annotated function per case (lets you cap each case at e.g. 500 tokens while the outer cap protects the run).
- Semantic confidence scoring via a second lightweight judge call (GPT-3.5 or local model scores 0.0-1.0).
- Full 10-bucket ECE with proper confidence binning.
- Export: `results.json` with per-case `Tracked<str>` lineage serialized via `to_json()`.
- Multi-model comparison: accept a comma-separated model list, run the same cases on each, compare score tables.
- Temperature sweep: run same cases at temperature 0.0, 0.5, 1.0 to show confidence calibration shift.
- Registry package: publish as `kryos-bench-governed` to the Kryos registry so other Kryos projects can `kryos pkg add kryos-bench-governed` and import `run_benchmark`.

---

## Build Plan (ordered steps for a fresh session)

**Step 0: Verify toolchain**
```bash
kryos --version          # confirm >= v2.3.0
```
Confirm env: `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` set, or `BENCH_BASE_URL` pointing at a local server (Ollama, LM Studio, vLLM).

**Step 1: Create project layout**
```bash
mkdir kryos-bench-governed
cd kryos-bench-governed
# No kryos.toml needed for MVP -- single-file uses stdlib only.
```

**Step 2: Write bench.kry with offline-first structure**
Write the file in this order to avoid forward-reference issues:
1. All `use` imports at top.
2. Struct definitions: `BenchCase`, `BenchResult`, `RunScore`.
3. Helper functions: `infer_confidence`, `cost_zero_local` (if needed), `format_report`, `score_run`.
4. `evaluate_case` -- calls `chat()`, wraps result.
5. `run_benchmark` (with `@budget`) -- loops over cases, calls `evaluate_case`.
6. `run_ensemble` -- optional, adds `majority_vote`.
7. `main` -- parses env, selects cfg, calls `run_benchmark`, prints.

**Step 3: Smoke-test offline mode**
```bash
kryos run bench.kry
# Should print offline message and exit 0 (no API key set).
```

**Step 4: Type-check**
```bash
kryos check bench.kry
# Fix any E0101 (unknown type), E0102 (undefined variable), E0100 (type mismatch).
```

**Step 5: Test against a local mock or real API**
```bash
# Option A: local Ollama
BENCH_BASE_URL=http://127.0.0.1:11434/v1 kryos run bench.kry

# Option B: real OpenAI
OPENAI_API_KEY=sk-... kryos run bench.kry

# Option C: real Anthropic
ANTHROPIC_API_KEY=sk-ant-... kryos run bench.kry
```
Verify the report prints accuracy, token totals, and per-case answers.

**Step 6: Test budget enforcement**
Temporarily lower the budget to 1 call: change the attribute to `@budget(tokens=50000, calls=1)` and run with 3 cases. The second case should throw with "llm error: @budget exhausted: no model calls left". Revert after confirming.

**Step 7: Verify Tracked audit trail**
Add `println(explain(r.audit))` for the first result and confirm the lineage shows: source (user prompt), inference step (model name + confidence).

**Step 8: Build release binary (optional)**
```bash
kryos build --release bench.kry -o kryos-bench
./kryos-bench   # confirm identical behavior
```
Note: prefer `kryos run` for iteration; `build --release` for distribution.

---

## Success Criteria / How to Demo

**The core demo (2 minutes):**
1. Run with 3+ cases and a real API key. Show the report: accuracy, token totals, per-case confidence.
2. Lower `@budget(calls=1)` and re-run 3 cases. Show the second case throw: "llm error: @budget exhausted". Point out: the third case never fires a request -- no network traffic, no money spent. This is the claim: pre-call refusal, enforced by the language, not by the programmer.
3. Show `explain(audit)` for one result: the lineage from "user question" through "model inference" is embedded in the value itself, not in a separate log.

**Secondary demo (for regulated-industry pitch):**
- Export `to_json()` on a result's `Tracked<str>` audit. Show the JSON lineage: timestamp, operation, description, metadata fields. This is the compliance artifact. It was produced by the program without any external tracing SDK.

**Pass criteria:**
- [ ] `kryos run bench.kry` exits 0 with no API key (offline mode).
- [ ] With API key, produces a report with accuracy, avg confidence, total tokens.
- [ ] With `@budget(calls=1)`, the second case throws before sending a request.
- [ ] `explain(audit)` on a result shows lineage with at least 2 entries (source + inference).
- [ ] Total token spend reported equals sum of per-case `tokens_used`.

---

## Risks and Honest Unknowns

**Risk 1: `majority_vote` on free-form strings (MEDIUM)**
`majority_vote()` in probable.kry uses `pj.value == pi.value` (exact string equality). For classification or factual Q&A (MVP use case), this works well. For open-ended generation, consensus requires semantic similarity -- not implementable without another model call or embedding. The MVP must document this limitation explicitly and recommend using majority_vote only on classification/short-answer tasks.

**Risk 2: Sub-budgets not supported (LOW for MVP, MEDIUM for production)**
The `@budget` attribute caps the entire `run_benchmark` invocation. A 100-case run with `calls=100` allows exactly 1 call per case, which is correct for single-inference evaluation. But if you want ensemble runs of 3 per case over 20 cases (60 calls), you need `@budget(calls=60)`. The budget is a coarse ceiling on the function, not a per-case cap. This is adequate for MVP but limits flexibility in production benchmarks. Workaround: write a `@budget(calls=3)` inner function for per-case ensemble evaluation and call it from a non-budgeted outer loop -- the inner budget enforces per-case limits, the outer architecture enforces the overall cap through case count.

**Risk 3: `time_now()` availability (LOW)**
`tracked.kry` uses `time_now()` for timestamps. The CLAUDE.md does not list it as a builtin. It is called in the confirmed stdlib source (tracked.kry line 31, agent.kry line 46), so it exists in the runtime. Use it. If the compiler rejects it, use `0` as a placeholder timestamp.

**Risk 4: `@copy` on BenchResult is wrong (KNOWN)**
`BenchResult` contains `Probable<str>` and `Tracked<str>`, both of which have str fields. Structs with heap-bearing fields cannot be `@copy`. Do NOT annotate `BenchResult` with `@copy`. Pass it by move. This means `results = push(results, result)` works (move into array) but you cannot read a field and then use the struct again without restructuring. For the report loop, iterate the array once and extract what you need.

**Risk 5: AOT vs JIT struct field ordering (LOW)**
The CLAUDE.md (gotcha 21) warns that tuple/enum fields BEFORE other fields used to cause AOT miscompile. That bug is now RESOLVED. Still: if you add enum-typed fields to any struct, place them last as defensive practice.

**Risk 6: `cost_tracker_new().record_tokens()` uses `float()` (LOW)**
`cost.kry` line 156 uses `float(count)`. The CLAUDE.md lists `(count as f64)` as the cast syntax. The stdlib itself compiles with `float()` so it works, but avoid it in new code you write. Use `(count as f64)`.

**Risk 7: Confidence is a heuristic, not a probability (HONEST)**
The `infer_confidence()` function assigns confidence based on string matching. This is not a calibrated probability. The ECE calculation will reflect calibration of the heuristic, not of the model. To get calibrated confidence, you need either logprobs from the API (OpenAI returns them under `logprobs` if requested; this requires extending the JSON parsing), or a secondary judge call. The MVP acknowledges this in the report header. Post-MVP can add logprob-based confidence via extending `LlmResponse` and `_parse_openai`.
