# Dual-Backend + WASM Cluster: What it Unlocks for Kryos

**Cluster:** dual-backend-wasm
**Source grounding:** compiler/crates/kryos-codegen-cranelift/, compiler/crates/kryos-codegen-wasm/src/lib.rs (~1,823 lines), compiler/stdlib/wasm.kry, docs/15-codegen.md, docs/18-cross-compilation.md, BENCHMARKS.md, CLAUDE.md
**Analysis date:** 2026-06-14

---

## What Actually Exists (Confirmed in Source)

### Cranelift JIT (`kryos run`)
- `compiler/crates/kryos-codegen-cranelift/src/jit.rs` (2,022 lines): full JITModule built on cranelift-jit. Targets host machine only (cranelift_native::builder). Used by `kryos run` and the REPL.
- In-process: real kryos-rt function pointers are registered directly via `jit_builder.symbol(...)`. No linker, no external toolchain. Compiles and runs in seconds on development machines.
- The JitCompiler struct is documented as supporting "incremental compilation" (comment in source), meaning individual functions can be JIT-compiled and re-executed - the foundation for a notebook-style kernel.
- REPL is a named, implemented tool (`kryos repl` in CLAUDE.md tooling table).

### LLVM AOT (`kryos build --release`)
- `compiler/crates/kryos-codegen-llvm/src/codegen.rs` (8,630 lines): full LLVM IR emission, calls `llc` + `clang`/`link.exe`.
- Measured: within 1.42x of Rust on all 7 benchmarks (rc.2 re-measure, 2026-07-10). Beats Rust on matmul (0.96x) and hashmap (0.65x). Beats C/C++ on nbody, matmul, hashmap. Source: BENCHMARKS.md (auto-generated from results.json, not hand-edited).
- Supports cross-compilation to 9 named triples plus arbitrary LLVM triples. Debug info (DWARF / CodeView .pdb) is supported.

### WASM Backend (`kryos build --target wasm32-unknown-unknown`)
- `compiler/crates/kryos-codegen-wasm/src/lib.rs` (1,823 lines): real wasm-encoder-based backend. Emits standard .wasm binary.
- **Honest maturity: v0.3 / experimental.** Supported: i64/f64/bool scalars, strings (packed-i64 in linear memory), arrays, basic control flow (if/else/while/recursion). NOT supported: structs, enums, tuples, maps, closures, match, to_string(), string interpolation, stdlib modules, channels, actors, WASI.
- JS-host contract (not WASI): the .wasm module imports from the `env` namespace (`kryos_print_i64`, `kryos_string_concat`, DOM bindings, `kryos_http_fetch`, etc.). Host runner: `node tools/wasm-host/run.mjs` or a browser shim.
- `compiler/stdlib/wasm.kry`: Kryos-source wrapper around all 30+ host imports. Users call `wasm.dom_set_text(...)`, `wasm.http_fetch_get(...)` etc. without writing extern blocks.
- Browser DOM bindings already declared: `kryos_dom_set_text`, `kryos_dom_get_value`, `kryos_canvas_fill_rect`, `kryos_canvas_clear`, `kryos_alert`.
- WASM has zero CI coverage (per AUDIT-v2.8.0.md). The backend exists and is structurally sound; coverage is the gap.

### Single Source, Three Execution Models
The language surface (`.kry` files) is the same regardless of target. The same MIR intermediate representation feeds all three backends. The only platform-conditional code is filesystem path handling.

---

## What "Write Once, Three Ways to Run" Unlocks

### 1. Agent Toolchain Notebooks (PARTIAL novelty)

**What:** A Kryos program that implements an AI agent tool can be developed in the Cranelift REPL/JIT (sub-second iteration), shipped as a LLVM native binary (production speed), and previewed as a WASM module in a browser without rewriting or recompiling across runtimes.

**Why it matters:** Agent tools are typically Python scripts (slow, dynamic, no capability checks) or Rust/Go binaries (fast but no language-level capability model). A Kryos tool gets capability annotation verified at compile time on ALL three backends from one source. The `@budget(tokens=N, calls=M)` attribute and the std.cost tracker both work natively.

**Honest novelty rating: PARTIAL.** Jupyter notebooks + Python tooling offer fast iteration, and Rust + wasm-pack gets you native + wasm from one source. Kryos is differentiated by capability checking + budget enforcement being part of the same language that runs in all three modes - Python has none of that, and Rust's wasm-pack story doesn't include an integrated token budget. But the "fast dev loop then ship" pattern itself is not novel.

**Buildable today:** PARTIALLY. Cranelift JIT + REPL work today. LLVM AOT works today. WASM works for scalar/string/array tools (most LLM tool schemas map to those types). Structs-as-tool-params fail on WASM - that needs language work (struct support in the wasm backend).

**Example:**
```kryos
// tool.kry - same file runs via: kryos run (JIT), kryos build --release (native), kryos build --target wasm (browser)
@capabilities(net)
@budget(tokens=2000, calls=10)
fn weather_tool(city: str) -> str {
    let resp = http_get("https://api.weather.example.com/v1/current?q=" + city)
    return resp
}
```

### 2. Plugin Sandboxing via WASM (TRULY NOVEL combination)

**What:** Agent host applications can load third-party Kryos plugins as WASM modules. The WASM boundary IS the capability boundary - a plugin compiled to wasm32 physically cannot call `file_write` or `http_get` on the host unless the JS/host runner wires those imports. This makes the WASM sandbox the runtime enforcement layer for capabilities that `--strict-capabilities` would enforce statically.

**Honest novelty rating: PARTIAL.** WASM sandboxing is not novel - Wasmtime, Extism, and many plugin systems use it. What Kryos adds is the compile-time capability annotation on the guest source, so the host can verify BEFORE loading that a plugin only declared `@capabilities(compute)` - and then confirm at the WASM boundary that no net/io imports are present. The two-layer check (static annotation + wasm import manifest) is differentiated. No mainstream language offers both layers integrated.

**Buildable today:** PARTIALLY TODAY. The WASM backend builds. The capability checker runs statically. What does NOT exist: a host-side runtime API to read a compiled .wasm's declared capabilities and enforce the import set against them. That's a missing bridge - call it `kryos_wasm_load(path, allowed_caps)`. Needs language+tooling work to close.

**Example:**
```kryos
// plugin.kry - compiled to .wasm, loaded by agent host
@capabilities(compute)  // pure math, no I/O - verifiable from wasm import section
fn score(input: str) -> f64 {
    // can only call compute - host runner exposes nothing else
    return 0.95
}
```
The host's JS runner would only provide compute-tier imports to this module - no `kryos_http_fetch`, no DOM - making the capability guarantee hardware-enforced at the wasm ABI, not just a compiler promise.

### 3. Edge Deployment Without Code Duplication (PARTIAL novelty)

**What:** A Kryos service can be compiled to native LLVM for the origin server (full speed, full stdlib) and the same source can emit WASM for edge workers (Cloudflare Workers, Fastly Compute@Edge) where only compute + HTTP is available. The std.cost tracker and std.tracked lineage work on both.

**Honest novelty rating: PARTIAL.** Go and Rust both support edge targets, and TinyGo is specifically designed for WASM edge. Kryos's differentiation would be that capability annotations are enforced on both paths, so "this function only uses net + compute" is a compile-time guarantee on native AND a structural guarantee on wasm (no io imports emitted). That's more composable than Go or TinyGo.

**Buildable today:** PARTIALLY TODAY on the WASM side. The WASM backend supports the HTTP fetch import (`kryos_http_fetch`) and string/array ops needed for request handling. Missing: struct-typed request/response objects (struct support in wasm needed), and an actual Cloudflare Workers adapter (no KV binding, no CF-specific host imports). The architecture is sound; the adapter layer needs building.

**Example:**
```kryos
// edge_handler.kry - compiles to wasm for Cloudflare Workers
@capabilities(net, compute)
fn handle_request(url: str, method: str) -> str {
    if method == "GET" {
        return fetch_upstream(url)
    }
    return "405 Method Not Allowed"
}
```

### 4. Hot-Reload Development Server (PARTIAL novelty)

**What:** Because Cranelift JIT compiles individual functions into executable memory without a linker step, a Kryos dev server could watch `.kry` files, re-JIT changed functions, and swap them into a running process. The REPL already exercises this path (`kryos repl` + `jit_compile_function` API).

**Honest novelty rating: PARTIAL.** Erlang has hot code loading. Julia has a JIT. Deno and Bun reload JS instantly. None of those have the capability system. The unique piece is that re-loading a function that has changed its `@capabilities(...)` annotation would be a capability violation detectable at reload time - a security checkpoint on hot-reload that no other ecosystem provides as a language primitive.

**Buildable today:** PARTIALLY TODAY. `JitCompiler::compile_all_with_module` and `jit_compile_function` exist. A file-watcher that calls these and re-patches function pointers could be built in pure Rust on top of the existing API. The language work needed: re-linking cross-function calls after a hot swap (the current JIT compiles the whole module, not individual functions in a running server).

### 5. Kryos Playground (BUILDABLE TODAY via Cranelift JIT to WASM)

**What:** The existing kryos-playground repo (NORTHTEKDevs/kryos-playground) can run Kryos code in the browser. The architecture is: user types .kry source, it POSTs to a kryos-runner backend (the existing kryos-runner repo), the runner calls `kryos run` (Cranelift JIT), captures stdout, returns it. This does not require the wasm backend at all.

A more ambitious version compiles the Kryos compiler itself to WASM and runs entirely in-browser (as Rust programs can be compiled to wasm). That's a longer-term item.

**Honest novelty rating: HYPE** for the server-side playground pattern (every language has this). **PARTIAL novelty** if Kryos adds capability-gated execution in the playground runner: user code runs with `@capabilities(compute)` only, and the runner enforces this by rejecting programs that would require net/io. The existing static checker can enforce this before execution.

**Buildable today:** TODAY. The runner architecture already exists (`kryos-runner` repo). Adding capability-gating to the runner is a few lines: parse the submitted .kry, check that no top-level function exceeds `compute`, refuse to execute if violated. The capability checker (`kryos-capabilities` crate) already does the analysis.

---

## Proposed Kryos Functions / APIs

### `wasm_load_capability_verified(path: str, allowed: CapabilitySet) -> WasmModule`
Load a .wasm produced by Kryos, verify its import section matches the declared `@capabilities` in the embedded custom section, and refuse to instantiate if the import set exceeds `allowed`. This bridges the static annotation and the wasm structural check.
Why unique: no other system ties compile-time capability annotations to wasm import validation at load time.

### `wasm_export_capability_manifest(module: WasmModule) -> CapabilitySet`
Read back the capability set from a compiled .wasm module's custom section (written by the Kryos compiler). Lets a host application display "this plugin requires: compute" before the user approves loading it.
Why unique: capability provenance survives the compilation boundary as metadata.

### `jit_reload(compiler: &mut JitCompiler, changed_fns: [str]) -> Result<(), CapabilityViolation>`
Re-JIT only the listed functions, check their new capability annotations against the running program's existing budget, and patch call sites. Throws CapabilityViolation if a reload would escalate.
Why unique: capability-safe hot reload - a security checkpoint that no other language's hot-reload offers.

### `kryos build --emit-wasm-caps`
CLI flag: after emitting the .wasm, also write a `.wasm.caps` sidecar JSON listing every capability declared in the source. Hosts can read this without parsing the wasm binary.
Why unique: structured capability manifest as a build artifact.

---

## Honest Summary of Gaps and What Needs Language Work

| Gap | Severity | Path to Fix |
|-----|----------|-------------|
| WASM structs/enums not supported | HIGH - limits tool param shapes | Wasm backend v0.4: lower structs as packed arrays in linear memory |
| WASM CI coverage is zero | MEDIUM - backend may have regressions | Add WASM matrix to CI: compile + run via node wasm-host/run.mjs |
| No capability manifest in .wasm output | MEDIUM - needed for plugin story | Write declared capabilities to a custom section at codegen time |
| `--strict-capabilities` not implemented | MEDIUM - deny-by-default is planned, not present | Implement the strict-mode flag in kryos-capabilities checker |
| No host-side `wasm_load_capability_verified` API | MEDIUM - needed for plugin sandboxing to be trustworthy | New Rust crate: kryos-wasm-host |
| WASM string interpolation / to_string() missing | LOW-MEDIUM - affects ergonomics | Implement in wasm backend; host-side stringify for common types |
| Cranelift JIT is host-only (no cross-arch JIT) | LOW - expected limitation | Document clearly; non-fix |

---

## Competitive Positioning (Honest)

- **vs. Rust + wasm-pack:** Rust gives you native + wasm from one source, with no capability system. Kryos adds capability annotations that are enforced on both paths. Rust's wasm ecosystem is vastly more mature.
- **vs. Go/TinyGo:** TinyGo specifically targets WASM/embedded. No capability system. Go's stdlib is far larger.
- **vs. Python + Pyodide:** Pyodide runs Python in WASM but is 8MB+ and slow at startup. Kryos WASM is small and fast for compute-only code. Python has no capability system.
- **vs. JavaScript/TypeScript:** JS runs natively in browsers and at the edge. No compile-time capability checking. Kryos is not competing on ubiquity; it's competing on safety guarantees.

The actual differentiated position: capability + budget checking that survives the native-to-wasm compilation boundary. No other language in this space offers that as an integrated language primitive today.
