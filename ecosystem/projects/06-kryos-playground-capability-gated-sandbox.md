# Project 06 -- Kryos Playground: Capability-Gated Sandbox

**Pitch:** The online Kryos REPL refuses to run any program whose static
capability analysis detects functions exceeding `@capabilities(compute)` --
the language's own compile-time capability system IS the sandbox policy, not
a separate OS-level jail layered on top.

---

## Context for a Fresh Session

This spec is self-contained. All repos referenced are on GitHub under
`NORTHTEKDevs` or locally at `~/projects/active/kryos-lang`.

**Key repos:**
- `NORTHTEKDevs/kryos-runner` -- already-deployed serverless execution
  sandbox for the playground. Receives `.kry` source, runs `kryos run`,
  returns stdout/stderr. The integration point for this project.
- `NORTHTEKDevs/kryos-playground` -- frontend REPL (editor + output pane).
  Calls kryos-runner via HTTP.
- `NORTHTEKDevs/kryos-dev-site` -- marketing/docs site that embeds or links
  the playground.
- `compiler/crates/kryos-capabilities/` (inside
  `~/projects/active/kryos-lang`) -- Rust crate containing `Capability` enum,
  `CapabilitySet`, `CapabilitiesConfig`, and `build_fn_capability_map()`.
  This is the library used by this project.

**Key compiler/stdlib paths:**
- `compiler/stdlib/cost.kry` -- `ComputeCost`, `Budget`, `budget_new`
- `compiler/stdlib/probable.kry` -- `Probable<T>`, `probable()`, `certain()`
- `compiler/stdlib/tracked.kry` -- `Tracked<T>`, `tracked_source()`,
  `transform()`
- `compiler/stdlib/llm.kry` -- `chat()`, `complete()`, `@budget` integration
- `compiler/stdlib/agent.kry` -- `AgentMemory`, alignment levels
- `docs/10-capabilities.md` -- authoritative status of the capability system

**Installed toolchain:**
```
kryos run <file>           # Cranelift JIT -- fast, no linker
kryos build --release      # LLVM AOT -- native binary
kryos check <file>         # type-check only, no codegen
```

Kryos syntax essentials (no semicolons; `elif` not `else if`; closures
`|x| expr`; `@capabilities(net,io)` attribute syntax;
`@budget(tokens=N, calls=M)` attribute syntax):

```kryos
@capabilities(compute)
fn safe_fn(x: i64) -> i64 {
    return x * 2
}

@budget(tokens = 10000, calls = 5)
@capabilities(net)
fn agent_fn(q: str) -> str {
    use std::llm::{anthropic_config, user, chat}
    let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
    return chat(cfg, [user(q)]).text
}
```

---

## Why This Is Novel

**Novelty rating: PARTIAL**

Every language has an online playground. Most use OS-level sandboxing: seccomp
BPF filters (Rust playground, Go playground), gVisor or nsjail (Judge0), or
CloudFlare Workers isolation (Wasm-based). These are host-level mechanisms:
they restrict what the OS permits after the program is loaded.

Kryos adds a compile-time gate BEFORE execution: the capability checker
statically determines which system resources each function in the submitted
program may touch. The runner rejects programs at the analysis phase, before
a single JIT instruction runs, if the static capability set of any function
exceeds the playground allowlist.

**Who already does something similar:**

| Approach | What it does | Why Kryos is different |
|---|---|---|
| Rust playground (play.rust-lang.org) | seccomp-BPF syscall filter at OS level; no network | Runtime OS policy; the compiler has no capability model |
| Go playground | network denied via iptables + seccomp | Runtime OS policy; no per-function static model |
| Deno Deploy | `--allow-net`, `--allow-read` flags at module load | Runtime-requested; caller provides the flag; no compile-time per-function proof |
| WebAssembly host (wasmtime --wasi) | WASI capability model at import-link time | Wasm module boundary only; does not track intra-module per-function sets |
| Judge0 | nsjail + cgroups + network namespace | Purely OS-level; language-agnostic; no semantic analysis |

**The Kryos difference, stated precisely:** The sandbox rule is expressed
as a Kryos capability set (`{compute}`) and enforced by the same `kryos-capabilities`
crate that enforces capability annotations in production code. The playground
is not running a separate policy engine -- it is running the language's own
type-checker in pre-execution mode. If the language's capability model
strengthens (e.g. deny-by-default ships), the sandbox automatically
strengthens too, for free.

**Honest limitations:**
- Today enforcement is opt-in per annotated function. An unannotated function
  is unconstrained by the checker -- it will not appear as a violation. The
  pre-execution gate therefore catches explicit violations of annotated
  functions and functions using stdlib modules that require non-compute
  capabilities (`std::net`, `std::http`, `std::db`, `std::fs`, etc.), but it
  cannot catch a function that calls `file_read` without any `@capabilities`
  annotation unless deny-by-default (`--strict-capabilities`) is also on.
- The current OS-level isolation in kryos-runner (if any) remains necessary
  as a defense-in-depth layer. The capability gate is additive, not a
  replacement.
- Sub-capabilities (e.g. `filesystem:read` vs `filesystem:write`) are not yet
  implemented; the playground gate operates on the top-level capability names
  only.

**The novelty that is real and matters for the demo:** No other online
playground shows a capability-aware rejection message like:

```
Playground error [E0502]: function `fetch_data` declares @capabilities(net)
but the playground only permits @capabilities(compute).
Remove the network call or run this code locally with `kryos run`.
```

This is a language-level semantic message, not a generic "operation not
permitted" from the OS.

---

## Which Kryos Primitives Are Used

**Used today (no language work needed):**

- `kryos-capabilities` crate (`compiler/crates/kryos-capabilities/`) --
  `build_fn_capability_map()` returns `HashMap<String, CapabilitySet>` for
  every annotated function in a source file. Already runs on every compile.
  Used here in library mode, called before JIT.
- `CapabilitySet::from_annotations()` -- parses `@capabilities(...)` args
  and produces the set of `Capability` variants: `Net`, `Io`, `Ffi`,
  `Compute`, `Crypto`, `Process`, `Env`, `Term`, `Db`, `Time`, `All`.
- `kryos run` (Cranelift JIT) -- the execution backend invoked by
  kryos-runner after the capability gate passes.
- `kryos check` -- type-check-only pass; no codegen. Can be called before
  running to get compiler diagnostics without executing user code.
- `@capabilities(compute)` attribute -- the only capability the playground
  allows; already a valid Kryos annotation.
- Stdlib modules that require non-compute capabilities (so the checker
  surface catches their use): `std::net`, `std::http`, `std::db`,
  `std::fs`, `std::process`, `std::crypto`, `std::ffi`, `std::env`,
  `std::term`, `std::llm` (requires `net`).
- `std::json` (json_stringify, json_object, json_string) -- used in the
  runner response body to return structured rejection reasons.

**Kryos language work needed first for full coverage:**

- `--strict-capabilities` mode (deny-by-default for unannotated functions) --
  PLANNED, not yet implemented. Without it, unannotated functions that call
  `file_read` or `http_get` are not caught by the checker. The MVP works
  correctly for annotated functions only; strict mode would close the gap.
- Sub-capability enforcement (`filesystem:read` etc.) -- not yet implemented;
  not needed for this project's MVP.

---

## Architecture

### Components

```
[User Browser]
      |
      | POST /run  {source: "..."}
      v
[kryos-playground frontend]   <-- Next.js / static; NORTHTEKDevs/kryos-playground
      |
      | HTTP POST /execute
      v
[kryos-runner service]        <-- NORTHTEKDevs/kryos-runner (already deployed)
      |
      +-- Step 1: write source to temp file
      |
      +-- Step 2: [NEW] capability gate
      |     call kryos-capabilities checker in library mode
      |     build capability map for all annotated functions
      |     if any fn capability set intersects {Net, Io, Ffi, Process, Db, Crypto, Env, Term, Time, All}
      |         return 200 { ok: false, error: "capability_violation", diagnostics: [...] }
      |
      +-- Step 3: kryos run <temp_file>  (Cranelift JIT, stdout/stderr captured)
      |
      +-- Step 4: return { ok: true, stdout: "...", stderr: "..." }
      v
[playground UI] renders output or capability-violation panel
```

### Data Model

The runner returns a single JSON shape for both success and rejection:

```json
{
  "ok": false,
  "error": "capability_violation",
  "diagnostics": [
    {
      "function": "fetch_data",
      "declared_capabilities": ["net"],
      "violation": "net is not permitted in the playground (only compute is allowed)",
      "error_code": "E0502",
      "suggestion": "Remove the @capabilities(net) annotation and the http_get call, or run locally with `kryos run`."
    }
  ]
}
```

On success:

```json
{
  "ok": true,
  "stdout": "Hello, World!\n",
  "stderr": "",
  "wall_time_ms": 42
}
```

### Capability Gate -- Rust implementation inside kryos-runner

The gate is a new function in kryos-runner (Rust) that calls into the
`kryos-capabilities` crate:

```rust
// In kryos-runner/src/capability_gate.rs

use kryos_capabilities::{build_fn_capability_map, Capability, CapabilitySet};

const PLAYGROUND_ALLOWLIST: &[Capability] = &[Capability::Compute];

pub struct CapabilityViolation {
    pub function: String,
    pub declared: CapabilitySet,
    pub violating: Vec<Capability>,
}

pub fn check_playground(source_path: &str) -> Vec<CapabilityViolation> {
    let map = build_fn_capability_map(source_path);
    let mut violations = Vec::new();
    for (fn_name, cap_set) in map {
        let forbidden: Vec<Capability> = cap_set
            .iter()
            .filter(|c| !PLAYGROUND_ALLOWLIST.contains(c))
            .collect();
        if !forbidden.is_empty() {
            violations.push(CapabilityViolation {
                function: fn_name,
                declared: cap_set,
                violating: forbidden,
            });
        }
    }
    violations
}
```

### Playground UI -- capability-violation panel

When `ok === false` and `error === "capability_violation"`, the playground
renders a styled panel instead of the output pane:

```
+------------------------------------------------+
| Capability Gate Blocked Execution              |
|                                                |
| Function "fetch_data" declares @capabilities(net)
| The playground only permits @capabilities(compute)
|                                                |
| This is Kryos's compile-time capability system |
| protecting the sandbox -- the same checker that|
| protects production code.                      |
|                                                |
| To use network access:                         |
|   kryos run your_file.kry  (run locally)       |
+------------------------------------------------+
```

### Example Kryos Programs That Demonstrate the Gate

Program that PASSES the gate (compute-only):

```kryos
use std::probable::{Probable, probable, is_confident}

@capabilities(compute)
fn classify_confidence(score: f64) -> Probable<str> {
    if score > 0.85 {
        return probable("high", score)
    } elif score > 0.5 {
        return probable("medium", score)
    }
    return probable("low", score)
}

fn main() {
    let result = classify_confidence(0.92)
    if is_confident(result, 0.8) {
        println("Classification: " + result.value + " (confidence: " + to_string(result.confidence) + ")")
    }
}
```

Program that FAILS the gate (net capability detected):

```kryos
@capabilities(net)
fn fetch_weather(city: str) -> str {
    return http_get("https://api.weather.com/v1/current?city=" + city)
}

fn main() {
    println(fetch_weather("Anchorage"))
}
```

Expected rejection output:

```
Playground capability gate blocked execution:
  function "fetch_weather": declares @capabilities(net)
  playground only permits @capabilities(compute)
  error code: E0502
  run locally: kryos run your_file.kry
```

Program that FAILS with std.llm (net implied by LLM API call):

```kryos
use std::llm::{anthropic_config, user, chat}

@capabilities(net)
fn ask_model(q: str) -> str {
    let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
    return chat(cfg, [user(q)]).text
}

fn main() {
    println(ask_model("What is the capital of Alaska?"))
}
```

This is a key demo: the message "to use LLM calls, run locally with
`kryos run`" communicates directly that Kryos's AI-native stdlib features
work -- they just need a local environment with real API keys, as they
should. The playground is the discovery surface; the capability gate
explains why the AI features need local execution.

---

## MVP Scope vs Full Vision

### MVP (smallest shippable slice -- build in one session)

1. Add `capability_gate.rs` to kryos-runner: call `build_fn_capability_map`,
   filter against `{Compute}` allowlist, return structured violations JSON.
2. Wire it into the runner's execute endpoint BEFORE the `kryos run` call.
3. Update playground frontend: handle `ok: false, error: capability_violation`
   response, render a styled explanation panel.
4. Add UI text: "Playground limits programs to @capabilities(compute). To use
   net, io, ffi, or AI features, install Kryos and run locally."
5. Three test programs: one that passes, one that fails on `@capabilities(net)`,
   one that fails on `@capabilities(io)`.

**No language changes needed for MVP.** The `kryos-capabilities` crate is
already a Rust library; calling `build_fn_capability_map` from kryos-runner
is a crate dependency addition, not a language change.

**Honest MVP limitation:** Programs with NO `@capabilities` annotation and
that call `file_read` or `http_get` directly are NOT caught by the current
checker (opt-in enforcement, not deny-by-default). The MVP gate catches
annotated functions only. Document this in the UI: "Note: the capability gate
currently checks annotated functions. Programs that call file_read or http_get
without annotations are blocked by the runner's OS-level sandbox."

### Full Vision (future sessions)

- Enable `--strict-capabilities` in the checker when it ships; the gate
  automatically tightens to catch unannotated functions too.
- Show a capability badge on every submitted program: "This program uses:
  compute only" or "compute + net" with colored chips.
- Add a "capability explorer" mode: run the checker, show the full capability
  map for the submitted source as a panel beside the editor. Educational
  feature -- lets users see what capabilities their functions declare before
  running.
- Share links: encode source in URL, include the capability verdict in the
  share. Example: `playground.kryos.dev?src=...&caps=compute` lets a link
  advertise "this is a compute-only program."
- Registry integration (from Project 05): if a shared snippet imports a
  package from the registry, the playground fetches the package's capability
  badge and adds its capabilities to the gate check.

---

## Build Plan (Ordered Steps for a Fresh Session)

**Pre-flight check (do first, takes 2 min):**
```bash
# Verify kryos is installed
kryos --version

# Verify kryos-runner repo is cloned
ls ~/projects/active/  # look for kryos-runner or clone from NORTHTEKDevs

# Verify kryos-playground repo is cloned
ls ~/projects/active/  # look for kryos-playground or clone from NORTHTEKDevs

# Verify the capabilities crate is accessible as a library
grep -r "build_fn_capability_map" ~/projects/active/kryos-lang/compiler/crates/kryos-capabilities/
```

### Step 1 -- Read kryos-runner source (15 min)

Read these files to understand the current runner before modifying it:

- `kryos-runner/src/main.rs` (or `handler.rs`) -- the execute endpoint
- `kryos-runner/Cargo.toml` -- current dependencies
- `kryos-runner/README.md` -- deployment notes

Goal: understand how source is received, written to disk, and executed.
Do NOT guess the structure; read the actual files first.

### Step 2 -- Add kryos-capabilities as a dependency (10 min)

In `kryos-runner/Cargo.toml`, add:

```toml
[dependencies]
kryos-capabilities = { path = "../../kryos-lang/compiler/crates/kryos-capabilities" }
```

If kryos-runner and kryos-lang are in different locations on this machine,
adjust the path. Verify it resolves: `cargo check` from the kryos-runner
directory.

### Step 3 -- Implement capability_gate.rs (30 min)

Create `kryos-runner/src/capability_gate.rs` with the `check_playground`
function shown in the Architecture section above. Key points:

- Call `build_fn_capability_map(source_path)` -- accepts a file path to the
  `.kry` source already written to disk.
- Iterate the returned map; filter capabilities not in `PLAYGROUND_ALLOWLIST`.
- Return `Vec<CapabilityViolation>` (empty = pass).

Add `mod capability_gate;` to `main.rs`.

Verify the module compiles: `cargo check`.

### Step 4 -- Wire gate into execute endpoint (20 min)

In the handler that processes `/execute` or `/run` POST requests:

1. Write source to temp file (already done by current runner).
2. Call `check_playground(&temp_path)`.
3. If violations is non-empty: serialize to JSON and return immediately
   (do NOT call `kryos run`).
4. If empty: proceed with current `kryos run` invocation.

The JSON response shape is defined in the Data Model section above.

Verify: `cargo build` passes.

### Step 5 -- Write integration test (20 min)

Add `kryos-runner/tests/capability_gate_test.rs` with three test cases:

```rust
#[test]
fn test_compute_only_passes() {
    // Source with @capabilities(compute) only -> violations empty
}

#[test]
fn test_net_capability_blocked() {
    // Source with @capabilities(net) -> violations contains "net"
}

#[test]
fn test_io_capability_blocked() {
    // Source with @capabilities(io) -> violations contains "io"
}
```

Run: `cargo test`. All three must pass before proceeding.

### Step 6 -- Update playground frontend (30 min)

In `kryos-playground` (Next.js or static HTML -- read the repo first):

1. Handle the `capability_violation` error response from the runner.
2. Render the capability-violation panel (see Architecture section).
3. Include the function name(s) and capability name(s) from `diagnostics`.
4. Add a permanent footer to the editor panel: "Playground limits programs
   to @capabilities(compute). Install Kryos to use net, io, ai, and ffi."

Verify locally: run the playground dev server, submit each of the three
test programs from the Architecture section, confirm correct behavior for
each.

### Step 7 -- Deploy and smoke-test (20 min)

Deploy kryos-runner (per its existing deploy process; check README). Deploy
the updated playground frontend. Hit the live URL with each of the three
test programs. Confirm the capability violation panel renders on the live
deployment.

---

## Success Criteria / Demo Script

The demo takes under 3 minutes:

1. Open playground URL. In the editor, paste the compute-only
   `classify_confidence` program. Click Run. Output panel shows
   `Classification: high (confidence: 0.92)`. No errors.

2. Replace with the `fetch_weather` program (net capability). Click Run.
   Output panel is replaced by the capability-violation panel:
   `function "fetch_weather" declares @capabilities(net) -- playground
   only permits @capabilities(compute)`. The program never executed.

3. Point to the message: "This is Kryos's own compile-time capability
   checker. The same analysis that protects your production code from
   unauthorized network access also protects the playground sandbox. No
   separate security layer -- the language's type system IS the sandbox."

4. Paste the `ask_model` / `std::llm` program. Same rejection with a note
   about running locally. This demonstrates that Kryos has native AI/LLM
   stdlib -- it just needs real API keys and local execution.

---

## Risks and Honest Unknowns

**Risk 1: `build_fn_capability_map` API surface may have changed.**
The spec cites the function name from source inspection of the `kryos-capabilities`
crate at the time of writing. A fresh session must read the actual crate source
(not trust this spec) and verify the public API before calling it. If the
function signature differs, adjust the gate implementation.

**Risk 2: unannotated functions are not caught by the current checker.**
This is the biggest honest gap in the MVP. A user who submits:

```kryos
fn main() {
    println(http_get("https://example.com"))
}
```

with NO `@capabilities(net)` annotation will bypass the capability gate
today because the checker only constrains annotated functions. The OS-level
sandbox in kryos-runner is the backstop. Document this gap in the UI and
track it as the blocker for `--strict-capabilities`.

**Risk 3: kryos-runner may not be structured as a Rust crate that can
easily add a crate dependency.**
If kryos-runner is a simple shell script around `kryos run`, the capability
gate needs to be implemented differently -- as a pre-execution `kryos check`
call that parses compiler output for capability error codes (E0501-E0507)
rather than calling the Rust crate directly. This is a valid fallback:
`kryos check` already emits structured diagnostics. Parse stderr for
`E0502` (capability escalation) before invoking `kryos run`. Read the
runner source before choosing the implementation path.

**Risk 4: the playground may be embedded in kryos-dev-site, not a
standalone app.**
Check the `NORTHTEKDevs/kryos-playground` and `NORTHTEKDevs/kryos-dev-site`
repos. If the REPL is inlined in the dev site, the frontend change is in
kryos-dev-site, not kryos-playground. Adjust the build plan accordingly.

**Risk 5: sub-capabilities are not implemented.**
A function annotated `@capabilities(filesystem:read)` (if someone writes
it) would fail to parse correctly today. Do not test or document
sub-capability syntax in the playground. The gate operates on top-level
capability names only: `net`, `io`, `ffi`, `compute`, `crypto`, `process`,
`env`, `term`, `db`, `time`, `all`.

**Risk 6: demo story depends on the AI/LLM program being blocked with a
useful message.**
The `std::llm` demo works only if the user annotates their function with
`@capabilities(net)`. If they write an unannotated llm-calling function,
the capability gate does not catch it (see Risk 2). The demo scripts in
this spec explicitly annotate functions so the demo is deterministic. In
the live playground, add an example snippet in the editor sidebar that
shows annotated programs so users encounter the gate behavior naturally.
