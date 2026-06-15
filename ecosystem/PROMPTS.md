# Kryos ecosystem - per-project kickoff prompts (Karpathy voice)

Build order: 01 -> 02 -> (03,10) -> 04/05 -> rest; do 11 to make capability-safety honest, then 12.

## 02 - Kryos governed-agent stdlib extension kickoff

```text
Read the spec at /c/Users/Krist/projects/active/kryos-ecosystem/projects/02-kryos-governed-agent-stdlib-extension.md before doing anything else. The Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang. Run things with `kryos run f.kry`, check without running with `kryos check f.kry`.

The goal is a single new stdlib file, `compiler/stdlib/agent_bridge.kry`, containing five functions that wire together std.tracked, std.cost, std.probable, and std.llm - modules that already exist but do not talk to each other. This matters because right now an agent that calls `chat_within` cannot ask "what did this specific value cost to produce?", "which sources contributed to this answer?", or "how much budget do I have left?". Five short bridge functions fix that. No compiler changes. No changes to existing stdlib files.

The five functions: `tracked_cost` (attaches a ComputeCost receipt to a Tracked value via lineage metadata), `tracked_merge` (collapses N Tracked<str> chunks into one value with deduplicated sources), `tracked_to_citation` (extracts a citation list from a Tracked value's lineage), `budget_remaining` (surfaces the active @budget frame's remaining tokens/calls as a tuple, returns (-1,-1) when no frame is active), and `filter_confident` (filters a [Probable<T>] ensemble to only values meeting a threshold).

Start with the smallest thing: write agent_bridge.kry, run `kryos check compiler/stdlib/agent_bridge.kry`, and read the output. Fix whatever is wrong before writing a single test. Then add the test file, then the integration demo. The spec has the exact code to start from; treat it as a draft, not a guarantee.

You are done with the MVP when:
1. `kryos run tests/stdlib/test_agent_bridge.kry` prints "all agent_bridge tests passed" with exit 0
2. `kryos run examples/agent_bridge_demo.kry` prints exactly 2 citations and a 5-entry lineage
3. `kryos build --release examples/agent_bridge_demo.kry && ./agent_bridge_demo` matches the JIT output
4. `kryos test` exits 0 (no regressions)
5. `use std::agent_bridge::{...}` resolves cleanly in a fresh file

Do NOT build anything in the "full vision" section of the spec. No new structs, no JSON export, no `budget_remaining_usd`, no compiler changes.

The honest risk: module registration. Check whether stdlib modules are auto-discovered from compiler/stdlib/*.kry or need an explicit entry in a resolver table. If they need registration and the table is inside self-host Kryos source, you will need a stage-1 rebuild of the compiler (~5-10 min). Figure that out at Step 3, after you have confirmed the file checks clean. The second risk is re-declaring the three `kryos_budget_*` externs that already live in llm.kry; if the compiler rejects duplicate extern declarations, move `budget_remaining` into llm.kry and thin-wrap it from agent_bridge.kry.

Write real Kryos: no semicolons, `elif` not `else if`, `for x in arr` loops where possible, `let mut` for mutables. Do not invent stdlib functions - grep the source first if you are unsure something exists.
```

## 06 - Kryos Playground - Capability-Gated Sandbox Kickoff

```text
Read the full spec at /c/Users/Krist/projects/active/kryos-ecosystem/projects/06-kryos-playground-capability-gated-sandbox.md before touching anything. The Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang.

The goal: wire Kryos's own compile-time capability checker into the online playground runner so that programs declaring @capabilities(net/io/ffi/etc.) are rejected before a single JIT instruction runs. The rejection message is a language-level semantic error, not a generic OS "operation not permitted."

Why it matters: every other language playground (Rust, Go, Deno) uses OS-level sandboxing as the policy engine. Kryos can enforce the sandbox using the same kryos-capabilities crate that enforces capability annotations in production code. That is the story - the language's type system IS the sandbox. No separate security layer.

Here is what to build (MVP only):

1. Read the kryos-runner source first - main.rs/handler.rs and Cargo.toml. Do not guess its structure.
2. Add kryos-capabilities as a path dependency in kryos-runner/Cargo.toml. Run cargo check to verify it resolves.
3. Create capability_gate.rs: call build_fn_capability_map(source_path), filter functions whose CapabilitySet contains anything other than Compute, return Vec of violations.
4. Wire the gate into the execute endpoint before the kryos run call. On violation, return structured JSON immediately (shape is in the spec).
5. Three integration tests: compute-only passes, net capability blocked, io capability blocked. cargo test must be GREEN before moving on.
6. Update the playground frontend to render a capability-violation panel when ok === false && error === "capability_violation".

Do NOT build: capability explorer mode, share links with capability badges, registry integration, strict-capabilities mode, sub-capability enforcement. Those are all in the spec as "full vision" - ignore them tonight.

Dumbest thing that works first: get cargo check green, then cargo test green, then manually POST a net-capability program to the runner endpoint and read the raw JSON response. Only then touch the frontend.

You are done with the MVP when:
- cargo test passes all three capability gate tests
- submitting the fetch_weather program (spec has it) to the live runner returns ok: false with "net" in diagnostics
- submitting the classify_confidence program returns ok: true with stdout output
- the playground UI renders the violation panel with the function name and capability shown

The honest risk that will bite you: build_fn_capability_map may have a different signature than what the spec describes - the spec was written from a point-in-time source read. Read the actual kryos-capabilities crate source in kryos-lang/compiler/crates/kryos-capabilities/ and verify the public API before writing a single line of gate code. A second risk: kryos-runner might be a shell script, not a Rust crate. If so, the fallback is to call kryos check, parse stderr for E0502 capability errors, and gate on that output. Read the runner before choosing the approach.

Never claim it works without running cargo test and reading the output.
```

## 08 - kryos-audit-trail EU AI Act Compliance Reporter kickoff

```text
Read the full project spec first:
  /c/Users/Krist/projects/active/kryos-ecosystem/projects/08-kryos-audit-trail-eu-ai-act-compliance-reporter.md

The Kryos compiler and stdlib live at:
  /c/Users/Krist/projects/active/kryos-lang

Run files with `kryos run f.kry`, build native with `kryos build --release f.kry`, check with `kryos check f.kry`.

---

The goal: build a ~150-line pure-Kryos library that takes a `Tracked<T>` value and emits a JSON record satisfying EU AI Act Annex IV traceability requirements. One function, `audit_tracked(t, cost, caps, confidence, system_id, user_id) -> str`, produces the whole thing.

Why it matters: the EU AI Act is in force for high-risk systems by August 2026. The compliance requirement is fundamentally a provenance problem, not a logging problem. Kryos's `Tracked<T>` already solves that problem structurally - the lineage is causally attached to the value, not written to a side-channel log. This library just surfaces that lineage in the shape auditors need.

---

Build the smallest useful slice first:

1. `src/cost_summary.kry` - `cost_to_json(c: ComputeCost) -> JsonValue`, the simplest helper with no internal dependencies.
2. `src/schema.kry` - `annex_iv_fields(lineage, cost, confidence) -> JsonValue` with the heuristics for human_oversight, model_identified, traceability pass/fail.
3. `src/audit.kry` - `lineage_to_json`, `caps_to_json`, `audit_record(...)`, `audit_tracked(...)`.
4. `src/main.kry` - a loan-approval demo that builds a tracked value through transform/inference/annotate steps, then calls `audit_tracked` and prints the result.

Run `kryos run src/main.kry | python3 -m json.tool` after step 4 and actually read the output. Don't add tests until you've seen the JSON and confirmed it looks right.

Do NOT build yet: `audit_stream`, caps.json sidecar reader, schema validation mode, GDPR redaction helper, or the `audit_record_pretty` variant. Post-MVP, all of it.

---

Done when:
- `kryos run src/main.kry` produces valid JSON with no parse errors.
- `annex_iv.traceability` is "PASS" when at least one lineage step exists, "FAIL: no lineage recorded" when none.
- `annex_iv.human_oversight_recorded` is true when a lineage entry's operation contains "review", "oversight", or "human".
- `annex_iv.model_identified` is true when a lineage entry's operation contains "inference", "model", or "llm".
- `confidence` field appears only when a non-negative value is passed.
- 3 unit tests pass: empty-lineage fail, human-review detected, model-inference detected.
- A one-page `SCHEMA.md` maps each Annex IV field to its Kryos source field.

---

One honest risk: `datetime_now_unix()` may not be the actual function name in `compiler/stdlib/datetime.kry` - check before writing `audit.kry`. If it's not there, use `0` as a placeholder and note it. Don't guess; check the source. Similarly, `Tracked.value` is typed `any` internally, so serializing non-primitive values will give you a struct repr string - document that limitation, don't try to fix it in MVP scope.

Write real Kryos: no semicolons, `elif` not `else if`, `@capabilities()` annotation on the pure functions, use `std::json`, `std::tracked`, `std::cost` as confirmed in the stdlib. Never claim it works without running it and reading the actual output.
```

## 09 - kryos-mcp-governed kickoff

```text
Read the spec at /c/Users/Krist/projects/active/kryos-ecosystem/projects/09-kryos-mcp-governed-capability-verified-mcp-server-template.md before writing a single line of code. The Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang. Run things with `kryos run f.kry` or `kryos build --release f.kry`.

The goal: build an MCP server in Kryos where every tool function carries a `@capabilities` annotation, and that annotation drives (a) what the server emits in the tools/list response and (b) a startup allowlist check against a server-level policy. That is it. The interesting thing is not the MCP plumbing -- it is that the capability claim lives in the source file, the compiler already enforces it for annotated functions, and the MCP host sees it before calling anything.

There is an existing template at NORTHTEKDevs/kryos-mcp-template that handles the JSON-RPC stdio loop in ~180 lines. Start from that -- copy it verbatim into src/main.kry and confirm `kryos build --release src/main.kry` produces a binary before touching anything else.

Here is the smallest useful thing to build:

1. cap_emit.kry -- a `tool_def_governed(name, desc, schema_node, caps: [str])` helper that appends `[caps: net]` to the description string and adds a `kryos_capabilities` array to the tool JSON.
2. cap_check.kry -- a `check_tools(records, policy)` function that walks the tool registry at startup and prints WARN for any tool whose declared caps exceed the server allowlist. Add a `KRYOS_MCP_STRICT=1` env var that turns warn into a hard exit.
3. tools.kry -- three demo tools: `add` (@capabilities(compute)), `fetch` (@capabilities(net)), `summarize` (@capabilities(net) + @budget(tokens=2000, calls=1)). For local testing, stub `tool_fetch` with a hardcoded string so you do not need live outbound access yet.
4. Wire them into main.kry with a `startup_check()` call at the top of main.

Build order matters: write and run each file in isolation before integrating. Cap_emit first, then cap_check, then tools, then the integrated main.

Do NOT yet: add Tracked/lineage wrapping, implement sub-capability enforcement, write a linter, or integrate with Claude Desktop. Those are all in the spec but not the MVP.

You are done with the MVP when:
- `echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | ./kryos-mcp-governed` returns JSON that includes `"kryos_capabilities":["compute"]` on add and `"kryos_capabilities":["net"]` on fetch.
- `KRYOS_MCP_CAPS=compute ./kryos-mcp-governed` prints two WARN lines at startup (fetch and summarize exceed the allowlist) and continues.
- `KRYOS_MCP_STRICT=1 KRYOS_MCP_CAPS=compute ./kryos-mcp-governed` exits non-zero with a clear error.
- Adding `file_write("/tmp/x", "y")` inside `tool_fetch` (annotated `@capabilities(net)` only) causes `kryos build` to reject with a capability error. Run this check explicitly and show the compiler output. This is the proof the demo hinges on.

The one thing most likely to bite you: `@capabilities` is opt-in today -- an unannotated function is unconstrained. Document this honestly in the README. Also verify that stacking `@capabilities(net)` and `@budget(tokens=2000, calls=1)` on the same function compiles without a parser error; the spec flags this as an unknown.

Real Kryos syntax reminders: no semicolons, use `elif` not `else if`, string concat with `+`, `let mut`, `while i < len(arr)`. Do not invent stdlib functions -- check the source under /c/Users/Krist/projects/active/kryos-lang/compiler/stdlib/ before using anything unfamiliar. Never report done without running the proving command and showing the output.
```

## 11 - Kryos Strict-Capabilities Mode kickoff prompt

```text
Read the project spec first:
/c/Users/Krist/projects/active/kryos-ecosystem/projects/11-kryos-strict-capabilities-mode-strict-capabilities-deny-by-default-.md

Compiler + stdlib live at /c/Users/Krist/projects/active/kryos-lang. Run files with `kryos run f.kry`, build native with `kryos build --release f.kry`.

---

Goal: add a `--strict-capabilities` flag to `kryos check` and `kryos build` that makes unannotated functions a compile error when they call any capability-gated builtin. Today those calls are invisible to the checker. After this change, they fail with E0505 -- the same error that annotated functions already produce when they exceed their declared set.

Why it matters: the whole @capabilities story is documentation today. An attacker (or a careless developer) can add `http_get(url)` to any unannotated function and nothing stops them. Strict mode makes the guarantee compile-time and machine-auditable. That is the wedge for selling Kryos as the language for trustworthy AI agent code.

Here is what I want you to build -- smallest useful version only:

1. Add `strict_capabilities: bool` to `BuildConfig` in `compiler/crates/kryos-driver/src/config.rs`. Default false.
2. Add `--strict-capabilities` to `Commands::Build` and `Commands::Check` in `compiler/crates/kryos-cli/src/main.rs`.
3. Change the `check_capabilities(module)` signature to `check_capabilities(module, strict: bool)` in `kryos-capabilities/src/lib.rs`, thread through pipeline.rs.
4. In `checker.rs`, add `strict_mode: bool` to `CapabilityChecker`. Replace the `if self.has_annotated_scope()` guard at ~line 537 (and the propagation check at ~line 564) with `if self.strict_mode || self.has_annotated_scope()`. In `check_function`, push unannotated functions with `annotated: annotated || self.strict_mode`.
5. Add 4 unit tests in checker.rs: unannotated + file_write in strict -> E0505; same without strict -> no error; unannotated + no gated calls in strict -> no error; @capabilities(io) + file_write in strict -> no error.
6. Annotate any examples that break under `kryos check --strict-capabilities examples/`.

Do NOT build yet: `#![strict_capabilities]` file-level annotation (parser addition, defer to v2), per-package kryos.toml setting, auto-fixer, sub-capabilities, WASM sandbox story.

Start by reading checker.rs and model.rs to understand the actual current state -- `has_annotated_scope()`, `CapabilityScope`, `CapabilitySet`. Do not edit blind. Then make the smallest change, `cargo test -p kryos-capabilities`, and read the output before touching anything else.

You are done with the MVP when:
- `kryos check --strict-capabilities` on a file with unannotated `http_get` or `file_write` emits E0505.
- `kryos check --strict-capabilities` on a fully-annotated file emits zero errors.
- `kryos check` (no flag) on both files emits zero errors -- no regression.
- All pre-existing tests in kryos-capabilities pass.
- The four new unit tests pass.

The one thing that might bite you: pipeline.rs has multiple `check_capabilities` call sites, and some may be in `check_file` paths that do not carry a full `BuildConfig`. Thread `strict: bool` directly through those paths rather than plumbing a whole config struct -- keep it minimal. Also: do not set `strict_capabilities: true` in any default config; the self-host compiler will blow up with thousands of errors if you do.
```

## 999 - kryos-agent-loop kickoff prompt

```text
Read the full spec first: /c/Users/Krist/projects/active/kryos-ecosystem/projects/03-kryos-agent-loop-governed-multi-turn-tool-use-library.md. The Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang. Run Kryos with `kryos run f.kry`; build native with `kryos build --release f.kry`; check types with `kryos check f.kry`. Real Kryos syntax rules: no semicolons, elif not else-if, use @budget/@capabilities attributes, Shared<T> for shared state, and do not invent stdlib symbols - verify against the actual source before writing them.

The goal is a library with one function: `chat_tools_governed`. Every Kryos app that does multi-turn LLM tool use today re-implements the same audit/cost/alignment boilerplate inline. This library collapses that into a single call that returns a `GovernedResult` with the final turn, a populated `audit_trail`, total cost as a `ComputeCost`, and step count. The point is not novelty - LangChain has `AgentExecutor`. The point is that `@budget(tokens=N, calls=M)` on `chat_tools_governed` is a compiler-enforced ceiling, not a check the caller remembers to add.

MVP is three files, nothing more:
- `src/lib.kry` (~200 lines): `GovernedResult` struct, `_tool_registered` helper, `chat_tools_governed`, `agent_checkpoint`
- `src/mock_server.kry` (~80 lines): returns one tool-call response then one final-text response, exits after two requests
- `tests/governed_loop_test.kry` (~120 lines): four @test functions covering the happy path, budget exhaustion, strict alignment blocking, and checkpoint write

Do NOT build yet: USD cost rates, energy tracking, streaming, per-tool timeouts, the Tracked or Probable variants, GovernedLoopConfig.

Before writing a single line, do two things: (1) grep the kryos-lang source for `time_now_secs` to confirm it exists as a builtin - if it does not, set latency_ms to 0.0 throughout and leave a TODO; (2) check `compiler/stdlib/agent.kry` to confirm the exact field names on `Agent` and `AuditEntry` before writing any struct literal.

Build the smallest thing, run it, read the actual output. Run `kryos check src/lib.kry` after each function. Run the mock server in a background terminal and hit it with curl before writing the test that depends on it.

You are done with the MVP when:
- `kryos test tests/governed_loop_test.kry` prints "4 tests, 4 passed, 0 failed"
- A live demo run (`ANTHROPIC_API_KEY=sk-... kryos run demo.kry "What is 23% of 4400?"`) prints Steps: 2, Audit entries: 3, and a correct answer
- `cat /tmp/audit.tsv` shows the three TSV rows

The one thing most likely to bite: the mock server. Getting an HTTP server in Kryos to synchronize cleanly with the test runner (start before, stop after, fixed port) may be more friction than expected. If it is, fall back to calling the private `_parse_anthropic_turn` / `_parse_openai_turn` functions in llm.kry directly with hardcoded response strings. That still exercises all library logic without a real HTTP call. Prefer working tests over an elegant fixture.
```

## 999 - kryos-bench-governed kickoff prompt

```text
Read the full spec before writing a single line of code:
  /c/Users/Krist/projects/active/kryos-ecosystem/projects/07-kryos-bench-governed-budget-bounded-benchmark-harness.md

Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang. Run programs with `kryos run f.kry`. Check types without running with `kryos check f.kry`. The stdlib modules (std::llm, std::probable, std::cost, std::tracked) are implemented -- read the source in kryos-lang/compiler/stdlib/ before assuming any function signature.

---

The goal: build a single-file LLM benchmark harness in Kryos where the token and call ceiling is a `@budget` attribute on the runner function -- not a shell timeout, not a counter the programmer manually decrements. The budget is enforced by the runtime before each `chat()` call fires. If the ceiling is hit, the call throws without touching the network. This is the entire point: pre-call refusal as a language property, not a convention.

Why it matters: every existing eval framework (HELM, lm-evaluation-harness, LangSmith) enforces spending limits through external controls that a buggy loop can bypass. Kryos makes the ceiling a proof obligation. That is a real differentiator for regulated-industry customers who need a cost-predictability guarantee they can point to in an audit.

---

MVP is one file: bench.kry, roughly 300 lines. Build in this order:
1. Struct definitions (BenchCase, BenchResult, RunScore). Do NOT put @copy on BenchResult -- it holds Probable<str> and Tracked<str>, which are heap-bearing.
2. infer_confidence() heuristic: exact match = 1.0, contains = 0.85, else 0.3, empty expected = 0.8.
3. evaluate_case() -- calls chat(), wraps reply in Probable<str>, builds ComputeCost and Tracked<str> audit.
4. run_benchmark() with @budget(tokens=50000, calls=100) and @capabilities(net) -- loops over cases, collects results.
5. score_run() -- accuracy, avg confidence, simplified ECE (two buckets: conf >= 0.7 and conf < 0.7).
6. format_report() -- plain text table.
7. main() -- reads OPENAI_API_KEY / ANTHROPIC_API_KEY / BENCH_BASE_URL from env, inline hardcoded 3-5 cases, offline-safe (no key = print budget ceiling and exit 0).

Do NOT build yet: JSON case loader, multi-model comparison, temperature sweep, semantic judge calls, registry packaging, sub-budgets per case.

---

Start by running `kryos --version` and confirming the stdlib files exist. Then write the file, run `kryos check bench.kry`, fix any type errors, and run `kryos run bench.kry` without a key to confirm offline mode. Only then set a real key and run against an API.

After the API run works, do one more thing: temporarily set @budget(calls=1) and run with 3 cases. The second case must throw "budget exhausted" without sending a request. That is the demo. Revert the attribute after confirming.

---

Done when:
- `kryos run bench.kry` with no API key exits 0 and prints the offline message.
- With a real key, the report prints accuracy, avg confidence, total tokens, and per-case answers.
- With @budget(calls=1) and 3 cases, the second case throws before hitting the network.
- `explain(r.audit)` on one result shows at least two lineage entries (source + inference).
- Token totals in the report sum correctly across cases.

---

One honest risk: majority_vote() in std::probable uses exact string equality for consensus. That is fine for the factual Q&A cases in the MVP. Do not use the ensemble path for open-ended generation and do not claim it does semantic matching -- it does not. Note this in a comment near the ensemble function.
```

## 999 - kryos-calibration kickoff prompt

```text
Read the full spec before writing a single line:
  /c/Users/Krist/projects/active/kryos-ecosystem/projects/10-kryos-calibration-calibrationtracker-ece-in-std-probable.md

Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang.
Run things with `kryos run f.kry`. Confirm `kryos --version` >= 2.3.0 before starting.

---

The goal: extend `compiler/stdlib/probable.kry` with `CalibrationTracker` and `ece()` so that confidence values in `Probable<T>` can be measured rather than asserted. Right now a function can return `probable(result, 0.9)` and the 0.9 is a guess -- nothing checks it. ECE (Expected Calibration Error) closes that loop. The novelty is not the algorithm (it is 10-line textbook math) but the placement: calibration measurement living in the same stdlib module as the confidence type, not in a separate ML framework.

MVP -- the five things to add to `probable.kry`:

1. `@copy struct CalibrationSample { predicted: f64, correct: bool }`
2. `struct CalibrationTracker { samples: [CalibrationSample], bins: i64 }`
3. `fn calibration_tracker(bins: i64) -> CalibrationTracker`
4. `fn add_sample(tracker: CalibrationTracker, confidence: f64, correct: bool) -> CalibrationTracker`
5. `fn ece(tracker: CalibrationTracker) -> f64`

That is roughly 60 lines. Do NOT add `add_probable_sample`, `calibration_summary`, or any integration with kryos-bench yet. Add those only after the smoke test passes.

Build sequence: read the existing `probable.kry` first to match its exact style (no semicolons, `elif` not `else if`, free functions, explicit `return`, `let mut` for mutation). Append the new structs and functions. Then write `calibration_smoke.kry`, run it, and read the actual output.

The smoke test: create a tracker with 10 bins, add 100 samples all at confidence 0.7 with exactly 70 correct (`i % 10 < 7`). Call `ece()`. A perfectly calibrated predictor saying 0.7 and being right 70% of the time should produce ECE near 0.0. Then add 100 samples at confidence 0.9 with only 60 correct -- ECE should come back around 0.30.

You are done with the MVP when:
- `kryos run calibration_smoke.kry` prints "PASS: ECE within expected range" and "PASS: overconfident ECE detected"
- The perfectly-calibrated case returns ECE in [0.0, 0.05]
- The overconfident case returns ECE in [0.20, 0.35]
- `kryos test` passes with no regressions in the existing stdlib

One honest risk: `push(tracker.samples, sample)` on an array of a `@copy` struct. The spec says this is resolved as of v4.47+ but verify it compiles before assuming. If the type checker complains about the element type, check what version you are on and whether the untyped-array-of-aggregates issue applies. Do not paper over a compile error with a workaround before understanding what it is telling you.
```

## 999 - kryos manifest --caps kickoff prompt

```text
Read the spec first: /c/Users/Krist/projects/active/kryos-ecosystem/projects/01-kryos-manifest-cli-capability-manifest-extractor-.md. The Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang. Do not invent stdlib APIs -- check the source before using them.

The goal: add a `kryos manifest --caps <file-or-dir>` subcommand to kryos-cli that emits a machine-readable JSON listing each function's declared capabilities. That's it.

Why it matters: right now the only way to know what capabilities a Kryos function has is to read the source. The manifest makes that information a stable artifact -- something CI can diff, a registry can badge, and a sandbox runner can enforce against. The compiler already computes this data on every build; we are just exposing it.

Before touching any code, run this to confirm the toolchain is clean:

  cd /c/Users/Krist/projects/active/kryos-lang/compiler && cargo build -p kryos-cli 2>&1 | tail -5

If it fails, stop and report it. Do not build on a broken baseline.

MVP -- build exactly this, nothing more:

1. `compiler/crates/kryos-cli/src/commands/manifest_cmd.rs` -- walks .kry files, calls the same lex -> parse -> AST walk that audit_cmd.rs already does, builds a BTreeMap of function name to capability list, emits JSON.
2. Wire it into `main.rs` Commands enum and `commands/mod.rs`. Pattern: copy the audit command registration and adapt.
3. Flags: `--format json` (default), `--strict` (include unannotated fns as capabilities: []), `--output <file>`, `--deny <cap>[,<cap>]` (exit 1 if any fn has a listed cap).
4. Five fixture .kry files in `tests/manifest/` and matching golden JSON in `tests/manifest/expected/`. Rust integration tests that compare output to golden files.

Do NOT build yet: --diff, --badge, --watch, registry integration, the ci-cap-gate.kry demo script. Get the Rust command right first.

After each step, run the relevant cargo command and read the actual output. "Compiles" means you ran it and it said 0 errors. "Tests pass" means you ran `cargo test -p kryos-cli manifest` and read the result.

Done when:
- `cargo test -p kryos-cli manifest` passes with at least 5 golden tests
- `kryos manifest --caps compiler/stdlib/llm.kry` shows `chat` with `["net"]`
- `kryos manifest --caps compiler/stdlib/llm.kry --deny net` exits 1
- `kryos manifest --caps compiler/stdlib/llm.kry --deny io` exits 0
- Output JSON has schema, source, functions (each with capabilities array and annotated bool), unannotated_count

The one thing that will bite you: before writing manifest_cmd.rs, read audit_cmd.rs top to bottom and check how it handles `Decl::Impl { methods }` -- a flat walk of top-level declarations misses impl methods. Also read `compiler/stdlib/json.kry` before touching any Kryos demo scripts to see what json_* functions actually exist.
```

## 999 - kryos-plugin-sandbox kickoff prompt

```text
Read the full project spec before touching anything:
  /c/Users/Krist/projects/active/kryos-ecosystem/projects/12-kryos-plugin-sandbox-attenuation-safe-wasm-plugin-host.md

The Kryos compiler and stdlib live at:
  /c/Users/Krist/projects/active/kryos-lang

---

The goal is a plugin sandbox that enforces capability attenuation at load time. When a host loads an untrusted .wasm plugin, it checks that what the plugin claims it needs (declared in a sidecar manifest for now) is actually a subset of what the host permits. If the plugin declares "net" but the host only allows "compute", instantiation is refused before a single byte of plugin code runs.

Why it matters: capability attenuation is what separates Kryos from "just another language." If the annotation @capabilities(compute) does not produce a machine-verifiable contract in the binary, it is a comment, not a guarantee. This project is the first demo that it is not a comment.

---

The MVP is Part A only - no WASM codegen changes, no binary parsing, no live plugin execution. Here is the smallest useful version:

1. A sidecar JSON file next to each .wasm fixture (plugin.wasm.caps.json, format {"version":1,"declared":["compute"]}) stands in for the custom WASM section.
2. wasm_load_capability_verified(path: str, allowed_caps: [str]) -> Result<PluginHandle, CapabilityViolation> reads the sidecar, compares declared against allowed, and returns the appropriate error variant if they do not match.
3. Three test fixtures covering: allowed passes, exceeds-allowed rejected, sidecar missing rejected.
4. kryos test runs all three and they pass.

Do NOT build: the WASM binary parser, the compiler-side section emitter, the import cross-check, the lying_plugin compile path, or --strict-capabilities. All of that is Part B and beyond.

---

Before writing any code, run this:

  kryos --version
  echo 'fn main() { println("hello") }' > /tmp/hello.kry && kryos run /tmp/hello.kry

Read the actual output. If the compiler is not on PATH or fails, stop and report it rather than coding blind against a broken environment.

Then look at what string builtins and file builtins are actually available. Check /c/Users/Krist/projects/active/kryos-lang/compiler/stdlib/ before writing any std:: imports. Do not invent stdlib functions that are not there.

Write real Kryos: no semicolons, elif not else-if, @capabilities/@budget attributes on functions that touch io, Result<T,E> with Ok/Err, match on variants.

---

You are done with the MVP when:
- kryos test passes with all three cases (allowed, exceeds-allowed, missing sidecar)
- kryos run demo_host.kry tests/fixtures/safe_plugin/plugin.wasm compute prints "plugin loaded with caps: compute"
- kryos run demo_host.kry tests/fixtures/net_plugin/plugin.wasm compute prints "REJECTED: ExceedsAllowed(net)"
- No test output was assumed - you ran the command and read the actual lines

---

The one thing that will bite you: std::json may not be wired up in the current stdlib build. If json_parse is not available, do not fight it - write a 20-line substring parser for the known format {"version":1,"declared":["a","b"]}. It is not worth the yak shave. Note this in a comment and move on.
```

## 999 - kryos-rag kickoff prompt

```text
Start by reading the full project spec at /c/Users/Krist/projects/active/kryos-ecosystem/projects/04-kryos-rag-rag-pipeline-with-built-in-citation-lineage.md. The Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang. Check compiler/stdlib/ before assuming any stdlib function exists. Do not invent stdlib that is not there.

The goal: build a RAG pipeline in Kryos where every answer carries its source citations as a Tracked<str> value -- not a side-car dict, not a flag you pass to the framework, but in the type itself. Why this matters: LangChain attaches metadata as a Python dict that any function can silently drop; in Kryos, a function that takes Tracked<str> and returns bare str has VISIBLY discarded the lineage in its signature. The whole point of this project is to prove that claim with running code.

MVP is three files (src/rag.kry, src/lineage_utils.kry, tests/test_rag.kry) plus a main.kry demo. No vector DB, no embeddings, no HTTP server. Keyword match in rag_retrieve is fine -- the retrieval mechanism does not matter for proving the lineage chain.

Build in this order and run the compiler after each step before moving on:
1. src/main.kry that prints "kryos-rag: ok". Run it. Confirm output.
2. src/lineage_utils.kry with tracked_merge and tracked_to_citation (the spec has the full implementations). Test them with a two-chunk merge, no LLM needed.
3. src/rag.kry: RagChunk struct, rag_retrieve with @capabilities(io), rag_answer with @capabilities(net) @budget(tokens=4000, calls=1), rag_citations.
4. tests/test_rag.kry: three tests that prove lineage without touching a live LLM (the spec has all three test bodies). Run kryos test. All three must pass.
5. Wire main.kry for the live demo using ANTHROPIC_API_KEY. Run it. Read the actual output -- citations printed must match the RagChunk id values you fed in.

Do NOT build: streaming, multi-turn conversation, re-ranking, file I/O corpus loading, cost tracking, or an HTTP server. Those are in the spec's "full vision" section. Skip them.

You are done with the MVP when:
- kryos test passes all three assertions in test_rag.kry (no API key needed)
- kryos run src/main.kry with ANTHROPIC_API_KEY set prints a non-empty answer plus a citations list whose entries match the chunk IDs, and explain(answer) shows a source -> merge -> inference chain

The one thing most likely to bite you: tracked_to_citation reads entry.metadata, not entry.source, because tracked_source() stores the doc ID as "source=<id>" in the metadata field (confirmed in tracked.kry line 33). The test asserts citations[0] == "source=doc:001" -- that is intentional, not a bug. Read the actual field before writing the dedup logic.

Kryos syntax reminders: no semicolons, elif not else-if, @capabilities/@budget as function attributes, no generic impl blocks (all operations are free functions). If the compiler is at /c/Users/Krist/projects/active/kryos-lang, run it from there or confirm kryos is on PATH before step 1.
```

## 999 - Kryos Registry Capability Badging - Session Kickoff

```text
Read the project spec at /c/Users/Krist/projects/active/kryos-ecosystem/projects/05-kryos-registry-capability-badging.md in full before writing a single line of code. The Kryos compiler and stdlib live at /c/Users/Krist/projects/active/kryos-lang.

The goal is to give every package in the Kryos registry a machine-readable capability badge, and expose two new CLI sub-commands: `kryos pkg show <name>` (prints what capabilities a package uses before you install it) and `kryos pkg audit <name>` (exits 1 in CI if ffi or process appears in a new version without a prior version having them).

Why it matters: no mainstream package registry shows you this. npm, crates.io, PyPI all tell you nothing about what a package actually does to your system. Kryos can say "the compiler verified this package cannot open network connections" - that is a stronger claim than Deno's runtime-requested permissions because it is proved at compile time, not asserted by the module itself.

Before writing code, read these four files:
- compiler/crates/kryos-capabilities/src/model.rs (Capability enum, CapabilitySet)
- compiler/crates/kryos-capabilities/src/checker.rs (the AST walker that already exists)
- compiler/crates/kryos-package/src/registry.rs (RegistryEntry, generate_index_entry, parse_index_entry)
- compiler/crates/kryos-cli/src/main.rs (existing sub-command structure)

Build in this order, and run the build + affected tests after each step before moving to the next:

1. Add CapsBadge struct to kryos-package/src/registry.rs (schema, capabilities, dangerous, annotation_coverage_pct, inferred_uncovered fields). Derive Serialize/Deserialize. Run cargo test -p kryos-package to confirm it compiles.

2. Add extract_package_caps(src_dir: &Path) to kryos-capabilities (new extract.rs). It walks .kry files, invokes the existing checker on each, unions annotated fn capability sets, counts annotated vs total fns, and returns a CapsBadge. Run cargo test -p kryos-capabilities.

3. Add kryos manifest --caps CLI sub-command: reads kryos.toml, calls extract_package_caps("src/"), writes target/caps.json, prints a human summary. Run it against the http-router demo package and look at the actual output.

4. Extend generate_index_entry() to embed the caps badge as "capabilities": {...} if target/caps.json exists. Extend parse_index_entry() to read it back as Option<CapsBadge> (absent field = None, no error). Run tests.

5. Add kryos pkg show <name>. Looks up the latest registry entry, prints the capability table. If capabilities is None: "No capability badge - package predates capability badging."

6. Add kryos pkg audit <name>. Diffs latest vs previous version badge. Exits 1 with a clear message if ffi or process appears for the first time. Warn (not fail) on any other new capability. Add --strict flag to fail on any new capability.

7. Run kryos manifest --caps inside the http-router demo package, update its registry index entry with the result, commit and push to NORTHTEKDevs/kryos-registry.

8. Write tests: CapsBadge JSON round-trip, generate_index_entry includes capability field, parse_entry_no_caps gives None without error, audit detects dangerous escalation.

MVP done when:
- kryos manifest --caps writes a valid target/caps.json in a project with @capabilities annotations (run it and read the file)
- kryos pkg show http-router prints a capability table from the live registry, not "no badge"
- kryos pkg audit http-router exits 0 (no escalation)
- A synthetic test package that adds @capabilities(ffi) in v0.2.0 causes audit to exit 1
- cargo test -p kryos-package and cargo test -p kryos-capabilities are fully green (run both and paste the output)

Do NOT build yet: web UI for badges (that is project 06), sub-capability granularity (fs:read vs fs:write), coverage enforcement on publish, deny-by-default audit.

The one honest risk: the badge is generated on the author's machine at publish time. A malicious publisher could generate a clean badge from clean source and then swap in dirty source for the tarball. In v1, kryos pkg show must print a disclaimer: "Badge reflects source on author's machine at publish time. Not independently verified." Do not skip this line - it is the difference between a useful trust signal and a false one. Full mitigation (registry CI re-running the checker against the tarball) is v2 work.

Write real Kryos where Kryos code appears: no semicolons, elif not else-if, @capabilities/@budget attributes, Shared<T> for shared state. Check stdlib source before using any std module - do not invent functions. Never claim tests pass without running them and reading the output.
```
