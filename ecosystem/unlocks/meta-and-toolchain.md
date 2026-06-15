# meta-and-toolchain Cluster Analysis

**Analyst cluster:** meta-and-toolchain  
**Date:** 2026-06-14  
**Source verified:** compiler/crates/kryos-capabilities/, compiler/crates/kryos-package/, compiler/crates/kryos-cli/, compiler/stdlib/llm.kry, compiler/stdlib/tracked.kry, compiler/stdlib/cost.kry, compiler/stdlib/probable.kry, docs/10-capabilities.md, compiler/ARCHITECTURE.md

---

## What this cluster covers

Self-hosting compiler + registry/playground/runner/mcp-template + JIT. The meta layer: what the toolchain enables for the ECOSYSTEM ITSELF rather than end-user programs. Four concrete sub-questions:

1. Capability-audited package registry -- every package publishes a capability+cost manifest; machine-verifiable.
2. Playground showing live capability/cost analysis.
3. MCP servers written in Kryos with provable tool-capability bounds.
4. LLM target: Kryos as the safest target for generated code.

---

## What is actually built today (verified in source)

### kryos-capabilities crate (~300 lines)

- `Capability` enum: net, io, ffi, compute, crypto, process, env, term, db, time, all. (`model.rs:13-36`)
- `CapabilitySet`: from_annotations, has(), is_subset_of(), excess_over(). (`model.rs:98-206`)
- `CapabilityChecker`: walks AST, checks annotated functions for builtin/path violations, attenuation (child subset of parent), cross-function propagation, escalation blocking. (`checker.rs`)
- `required_capability_for_builtin()`: maps 30+ builtins (file_read, http_get, tcp_connect, sha256, env_get, etc.) to their required capability. (`model.rs:230-265`)
- CRITICAL HONEST CAVEAT: enforcement is opt-in per annotated function. An unannotated function is NOT constrained today. `--strict-capabilities` (deny-by-default) is NOT implemented. (`docs/10-capabilities.md:9-19`)

### kryos-package crate (~1740 lines)

- `Manifest` struct with `CapabilitiesConfig { allowed: Vec<String> }` field -- the `[capabilities]` section of `kryos.toml` IS parsed and stored. (`manifest.rs:17,201-205`)
- Registry: Git-backed NDJSON index at `NORTHTEKDevs/kryos-registry`; sha256 checksums per tarball; `generate_index_entry()` emits checksum but NOT a capabilities manifest field. (`registry.rs:134-162`)
- `RegistryEntry` struct: name, version, checksum, dependencies, download_url. No capability field in the wire format TODAY. (`registry.rs:37-43`)
- Resolution, lock file, semver -- all present and working.

### CLI (kryos-cli, ~700 lines)

- `kryos run` (Cranelift JIT), `kryos build --release` (LLVM AOT), `kryos check`, `kryos test`, `kryos fmt`, `kryos doc`, `kryos lsp`, `kryos repl`, `kryos eval`, `kryos pkg`, `kryos bindgen` -- all subcommands present. (`main.rs`)
- Artifact cache flag exists (`--cache`/`--no-cache`). (`main.rs:83-90`)
- wasm backend flag exists (`--backend wasm`). (`main.rs:41-43`)
- `kryos doc` generates markdown from `///` doc comments via kryos-doc crate. (`doc/src/lib.rs`)
- kryos-lsp: completion, goto_def, hover, inlay_hints, diagnostics, formatting, document_symbols -- full LSP. (`crates/kryos-lsp/src/`)
- kryos-bindgen: C header to Kryos bindings (~1580 lines). (`ARCHITECTURE.md`)

### std.llm (stdlib/llm.kry)

- `@capabilities(net)` on `chat()`, `complete()`, `chat_within()`, `chat_tools()`, `continue_with_tool_results()` -- the LLM client is annotated and will be checked when it appears in a `@capabilities`-annotated caller.
- Budget hooks are extern FFI calls (`kryos_budget_active`, `kryos_budget_try_call`, `kryos_budget_charge_tokens`) -- these are runtime hooks, not compiler-enforced. The `chat()` function checks them at runtime.
- Tool calling (ToolDef, ToolCall, ToolResult, ToolTurn, chat_tools, continue_with_tool_results) -- fully implemented in Kryos source.
- Both OpenAI-wire and Anthropic-wire formats supported.

### std.tracked, std.cost, std.probable (stdlib)

- `Tracked<T>` with lineage: [LineageEntry], to_json(), explain() -- all in Kryos source. (`tracked.kry`)
- `ComputeCost` with wall_time_ms, tokens_used, api_calls, money_usd, energy_kwh; Budget with charge(), is_exhausted(), remaining_*(); CostTracker. (`cost.kry`)
- `Probable<T>` with confidence, best_of(), majority_vote(), entropy(), combine(). (`probable.kry`)

---

## Honest Novelty Analysis

### 1. Capability-audited package registry

**What Kryos has today:** `kryos.toml` parses `[capabilities] allowed = ["net","io"]`. The compiler's capability checker can verify a package's source against those declarations. The registry index stores sha256 checksums but does NOT include a capabilities manifest field in RegistryEntry or in generate_index_entry(). There is no automated "audit every published package and publish its capability signature" pipeline.

**What would be needed:** (a) emit a capability manifest alongside each tarball -- a JSON file listing each public function's @capabilities set, produced by running kryos check --emit-capabilities on the package source; (b) registry index schema updated to include a `capabilities` field; (c) `kryos pkg add` shows/enforces the capability profile before install.

**Novelty: PARTIAL.** npm has no capability system. Rust crates.io has no capability system. Deno's permission model is the closest: Deno requires runtime permission flags (`--allow-net`, `--allow-read`) and its registry (deno.land/x) does NOT publish machine-verifiable capability manifests -- it relies on code review. Kryos differs in two ways: (1) the check is compile-time, not runtime-flag, and (2) the annotation is on individual functions, not the entire binary. A registry that publishes per-function capability signatures with machine verification would be novel relative to every current package registry. However, the capability check today is opt-in only, which reduces the strength of the guarantee substantially.

**Buildable today: NEEDS-LANGUAGE-WORK.** The pieces exist (kryos-capabilities can check, manifest can carry the field), but the opt-in enforcement means a package with no @capabilities annotations passes all checks silently. The pipeline to emit+publish capability manifests does not exist. Deny-by-default capabilities needed before the guarantee is meaningful.

### 2. Playground with live capability/cost analysis

**What Kryos has today:** kryos-playground repo exists on NORTHTEKDevs. The kryos-runner repo is the execution engine. The compiler can emit capability diagnostics (kryos-capabilities produces Diagnostics with E0501-E0507 codes). The doc generator produces structured output from annotations.

**What would be needed:** the playground UI needs to call kryos check / kryos run and render the capability set of each function alongside the code, show which builtins each function would need capability for, and optionally show ComputeCost from a run.

**Novelty: PARTIAL.** The Rust Playground, Go Playground, Godbolt Compiler Explorer -- none of them show permission/capability analysis as a first-class pane. Showing "this function needs [net, io]" in the browser, derived from static analysis of the submitted code, with the attenuation graph, would be unique in web playgrounds. The cost/budget side would require a runtime instrumented run, which is harder. Static capability display is today's blocker: the playground needs to pipe the compiler's diagnostics JSON out. The compiler has the analysis; the UI integration is the gap.

**Buildable today: TODAY** (for static capability display -- the compiler already emits the diagnostics; the playground just needs to surface them). Cost display from a real run requires actually calling an LLM and measuring tokens -- that's buildable but more involved.

### 3. MCP servers written in Kryos with provable tool-capability bounds

**What Kryos has today:** kryos-mcp-template repo on NORTHTEKDevs. std.llm has tool calling (ToolDef, chat_tools). The capability system means an MCP server written in Kryos can have @capabilities(net) on its tool handlers and @capabilities() (pure) on its parse/validate helpers -- and the compiler will catch if a "pure" helper tries to exfiltrate via file_write. The wasm32 target exists, enabling sandboxed MCP runners.

**What's the claim:** an MCP server where the compiler PROVES each tool handler's capability profile. Compare: a Python MCP server can call subprocess.Popen() anywhere. A Kryos MCP server with deny-by-default capabilities (when implemented) would make it statically impossible for a tool declared as pure to call network or process builtins.

**Novelty: PARTIAL, opt-in only.** Today (opt-in enforcement), an unannotated Kryos function in an MCP server can still call file_read. The provability claim requires deny-by-default. The template exists; the strong guarantee doesn't yet. With deny-by-default + sub-capabilities (e.g. `fs:read` only), this would be genuinely novel in the MCP ecosystem -- no other MCP implementation language gives you compile-time tool-capability bounding.

**Buildable today with caveats:** An MCP server in Kryos is buildable today (template exists, std.llm handles tool calling). The "provable bounds" framing is only honest if you annotate every function and use strict mode -- which requires language work (--strict-capabilities not implemented).

### 4. LLM-emits-Kryos as the safest target for generated code

**The thesis:** when an LLM generates code, Kryos is safer than Python/JS because the generated code cannot exfiltrate (capability system) or run away (budget). An agent scaffold that asks an LLM to generate Kryos snippets, then `kryos run`s them, gets a sandboxed execution with static guarantees.

**What Kryos has today:** kryos run (Cranelift JIT) gives fast iteration. The capability checker already catches obvious violations in annotated code. The wasm32 target gives an additional isolation layer.

**Honest assessment:** the "safest target" claim rests heavily on deny-by-default capabilities, which are not yet implemented. With opt-in enforcement, an LLM-generated Kryos function with no @capabilities annotation can do anything. The wasm32 target + wasm host sandbox is a real today-buildable safety story independent of capabilities. The budget enforcement (std.llm + @budget hooks) IS implemented and DOES halt runaway agent loops -- that part is today.

**Novelty: PARTIAL.** No mainstream language + runtime combination today gives you: compile-time capability checking + language-level token budget enforcement + data provenance as stdlib types. Kryos combines all three. The combination is novel. Each piece individually has analogs (wasm sandboxing, OTel cost tracking, lineage DBs), but the integrated language-level design is the differentiator.

**Buildable today: TODAY for the subset that is implemented.** An LLM-in-Kryos agentic program with @budget annotation + std.llm.chat_within will halt on budget exhaustion today. Capability-based exfiltration prevention needs the language work.

---

## Proposed Kryos Functions

### `kryos_capability_manifest()`

Add to `kryos-capabilities` crate: a function that, given a compiled module, returns a structured capability manifest (per-public-function capability sets as JSON). This would be called by `kryos pkg publish` to embed the manifest in the registry entry. The manifest enables machine-verifiable capability auditing at install time.

Signature (Rust): `pub fn emit_capability_manifest(module: &Module) -> CapabilityManifest`  
Where `CapabilityManifest` is a serializable struct mapping function names to CapabilitySets.

### `std::registry::capability_manifest_of(pkg: str) -> CapabilityManifest`

A stdlib function that fetches and parses the capability manifest for a named registry package. Enables runtime checks ("does this dynamically-selected package have only net capability?") before importing it.

Signature: `fn capability_manifest_of(pkg_name: str) -> CapabilityManifest`

### `std::sandbox::run_kry_snippet(code: str, caps: CapabilitySet, budget: Budget) -> SnippetResult`

A function that compiles and runs a Kryos code snippet in a wasm32 sandbox with explicitly bounded capabilities and budget. Enables the "LLM emits Kryos, we run it safely" use case today.

Signature: `fn run_kry_snippet(code: str, caps: CapabilitySet, budget: Budget) -> SnippetResult`  
Where `SnippetResult = { output: str, cost: ComputeCost, capability_violations: [str] }`

---

## What needs language work before the claims are fully honest

1. **`--strict-capabilities` / deny-by-default mode** -- the single biggest gap. Without it, capability-based safety is only as strong as developer discipline. This is the prerequisite for the registry manifest, the MCP provability claim, and the LLM-target safety story.
2. **Sub-capabilities** (`fs:read` vs `fs:write`, `net:outbound` vs `net:listen`) -- mentioned in docs/10-capabilities.md but not in model.rs. Needed for fine-grained tool bounding in MCP servers.
3. **Registry capabilities field** -- the wire format (RegistryEntry) and index entry format need a `capabilities` field alongside `checksum`. The manifest.rs `CapabilitiesConfig` exists but is not propagated to the published index entry.
4. **`kryos pkg publish --emit-capabilities`** -- the toolchain needs a subcommand step that runs the capability checker over the package and embeds the result in the tarball/index entry.
5. **Playground capability pane** -- purely frontend/integration work; the compiler already has the analysis.
