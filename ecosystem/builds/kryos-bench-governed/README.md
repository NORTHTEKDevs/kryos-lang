# kryos-bench-governed

A budget-bounded LLM benchmark harness written in Kryos. The token/call ceiling
is a `@budget` attribute on the runner function, enforced by the runtime **before
each `chat()` request fires** (pre-call refusal). A buggy loop cannot overspend:
the ceiling is a language property, not a counter the programmer can forget to
decrement.

Every existing eval framework (HELM, lm-evaluation-harness, LangSmith) enforces
spending limits with external controls (shell timeouts, middleware, manual
counters) that a bug in the eval loop can bypass. Kryos makes the ceiling a proof
obligation satisfied by the runtime.

## File

- `bench.kry` — the whole harness (~300 lines, single file, stdlib only).
- `run.sh` — wrapper that points at a `@budget`-capable toolchain (see Toolchain).

## Run

```sh
# offline dry-run (no creds): prints the budget ceiling + the case list, exits 0
./run.sh

# live against a local OpenAI-compatible endpoint (Ollama here)
BENCH_BASE_URL=http://127.0.0.1:11434/v1 BENCH_MODEL=llama3.2:3b ./run.sh

# live against OpenAI / Anthropic
OPENAI_API_KEY=sk-...      ./run.sh
ANTHROPIC_API_KEY=sk-ant-... ./run.sh
```

Env vars read by the program: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
`BENCH_BASE_URL` (any OpenAI-compatible server), `BENCH_MODEL` (optional model id
override). No creds of any kind → offline mode.

## What it does

For each inline case it sends one `chat()` call, wraps the reply in a
`Probable<str>` (confidence from a string-match heuristic), records a
`ComputeCost` receipt, and builds a `Tracked<str>` audit trail
(source + inference lineage). It then scores accuracy, average confidence, a
2-bucket ECE, and total token/call/latency, and prints a report. The last result's
`explain(audit)` is printed to show the lineage lives on the value itself.

## Verified behavior (executed against Ollama `llama3.2:3b`)

- **Offline**: no creds → prints ceiling (`max tokens 50000 / max calls 100`) +
  cases, exits 0, sends nothing.
- **Live report**: 4 cases, accuracy 0.75, avg conf 0.91, total tokens 205 (= 52+51+54+48, sums correctly), per-case answers printed.
- **Budget enforcement** (`@budget(calls=1)`, then reverted): case 1 consumes the
  one allowed call; case 2 throws `llm error: @budget exhausted: no model calls
  left (remaining tokens: 49949)` **before any second request** — caught and shown
  as a pre-call-refusal banner.
- **Provenance**: `explain(audit)` shows 2 lineage entries (`[source]` user prompt
  + `[inference]` model + confidence).

## Honest limitations (by design, MVP scope)

- **Confidence is a string-match heuristic** (exact=1.0, substring=0.85, else=0.3,
  open-ended=0.8), NOT a calibrated probability. The ECE reflects the heuristic,
  not the model. Real calibration needs logprobs or a judge call (post-MVP).
- **No ensemble / `majority_vote`**: `std::probable::majority_vote` decides
  consensus by EXACT string equality, which is wrong for open-ended generation.
  The MVP runs a single inference per case and documents this in `bench.kry`.
- **`time_now()` is whole-second granularity**, so sub-second latencies read as 0.
  The latency fields are effectively seconds.
- **`@budget` is a coarse per-run ceiling**, not a per-case sub-budget. Per-case
  caps need a nested `@budget` function (post-MVP).
- Not built (out of MVP scope): JSON case loader, multi-model comparison,
  temperature sweep, semantic judge, registry packaging, per-case sub-budgets.

## Toolchain (important)

The installed `kryos` at `~/.local/bin` is **v4.43.0-rc.4 (May 2026)**, which
**predates `@budget`** (landed v4.46.0). Its `kryos_rt.lib` has no
`kryos_budget_*` symbols, so `kryos run` fails to link any program that calls
`chat()`:

```
bench.o : error LNK2001: unresolved external symbol kryos_budget_active
```

`run.sh` works around this by using the current HEAD build in the kryos-lang repo
plus that build's runtime static libs:

- binary: `kryos-lang/compiler/target/debug/kryos.exe` (HEAD; `run` uses Cranelift)
- `KRYOS_RT_LIB`        → `target/release/kryos_rt.lib` (exports `kryos_budget_*`)
- `KRYOS_STDLIB_NATIVE_LIB` → `target/release/kryos_stdlib_native.lib`

Canonical fix: rebuild + reinstall the toolchain from HEAD into `~/.local`.
