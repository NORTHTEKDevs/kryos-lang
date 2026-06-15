# Capability-Safety Unlock Analysis

**Cluster:** capability-safety
**Source verified:** `compiler/crates/kryos-capabilities/src/{lib.rs,model.rs,checker.rs}`, `docs/10-capabilities.md`
**Date:** 2026-06-14

---

## What the system actually is today

The `kryos-capabilities` crate is a compile-time static analysis pass that runs over the AST after parsing. It enforces three real rules:

1. **Annotated functions must only call builtins/stdlib matching their declared set.** A `@capabilities(net)` function that calls `file_write` (which requires `io`) gets a compile error E0505.

2. **Attenuation: child scopes cannot exceed parent capabilities.** A function nested inside an actor, or a spawn expression inside a `@capabilities(net)` function, cannot introduce `io` that the parent does not have. Error E0503.

3. **Cross-function propagation.** If function `a` (`@capabilities(io)`) calls function `b` (`@capabilities(io, net)`), the checker catches the missing `net` at the call site. Error E0507.

4. **Prohibited escalation actions.** Calls to `add_capability`, `widen_sandbox`, `increase_budget`, etc. are compile errors regardless of context. Error E0504.

**Critical honest caveat (from docs and the checker source):** The guard in `check_callee_capabilities` for bare builtin calls reads:

```rust
if self.has_annotated_scope() {
    // only enforced inside explicitly annotated @capabilities scopes
```

An unannotated function is fully unconstrained. `fn save() { file_write("secret.txt") }` with no `@capabilities` annotation compiles cleanly. The deny-by-default end state -- where an unannotated function is restricted to pure computation -- requires `--strict-capabilities` mode, which is **not implemented**. The integration test `builtin_file_write_without_annotation_passes` proves this explicitly.

**What is NOT implemented (per docs/10-capabilities.md implementation status box):**
- Sub-capabilities (`filesystem:read`, `network:http`) -- the doc describes them, the code does not have them; `Capability::from_str` has no colon-path parsing
- Runtime `CapabilityEnforcer` scope stack
- Append-only audit log
- Sandboxing / child sandbox creation
- `kryos check` CLI command for capability-only analysis
- License tier enforcement at compile time

The `model.rs` has `Budget` and `Sandbox` structs but they are parsed from annotations and not enforced in any way -- `check_function` calls `Budget::from_annotations` and `Sandbox::from_annotations` and assigns them to `_budget` / `_sandbox`, the leading `_` confirming they are currently ignored.

---

## What the compile-time attenuation does unlock TODAY

### Unlock 1: Auditable third-party / AI-generated code without reading it

**Novelty: PARTIAL**

When you run `kryos check` on a third-party package (once that command exists), or when the compiler processes it as a dependency, every annotated function's capability surface is visible in the function signature -- you can see `@capabilities(net, io, crypto)` without reading the body. Combined with cross-function propagation, calling a library function that has `@capabilities(net)` from your `@capabilities(compute)` function is a compile error E0507.

Who else does this: Wasm component model `wit` interfaces declare imported capabilities at the module boundary, but that is a module-level contract, not a per-function annotation that propagates through the call graph. Rust `#[cfg]` and `unsafe` are single-feature gates, not a composable set. Java `SecurityManager` is runtime and deprecated. OCaml effects are closest in spirit but cover control flow, not resource access, and are not yet in stable OCaml.

**Buildable today:** Partially. The cross-function propagation check (E0507) works for annotated callers calling annotated callees. The gap: an unannotated wrapper function can launder capabilities through because it has no annotated scope.

**Example use:** A Kryos plugin marketplace where package authors annotate every public function. Consumers can read the registry manifest's capability summary (extractable by the compiler) and trust that a `@capabilities(compute)` math library cannot phone home.

---

### Unlock 2: Least-privilege agent code as a language property

**Novelty: PARTIAL**

The intended use case in `docs/10-capabilities.md` is explicit: "FFI calls, dynamic code loading, plugin systems -- all run inside sandboxes with explicitly granted capabilities." For AI agent code specifically, you write:

```kryos
@capabilities(net, db)
fn run_agent_action(input: str) -> str {
    // This function and everything it calls can only touch net and db.
    // file_write, exec, http_post to exfil endpoints in other modules
    // are compile errors if those modules are annotated.
}
```

This is materially better than a runtime sandbox for one specific reason: the constraint is visible at the call site to human reviewers, not hidden in a runtime policy config. The `@budget(tokens=N, calls=M)` attribute (implemented in the runtime, see `kryos-rt`) adds a second axis: even within the declared capability set, the function cannot loop infinitely calling the LLM.

Who else does this: Python `restrictedexec` / PyPy sandbox are runtime only and abandoned. Java security sandboxes are runtime and deprecated (Java 17). Deno has a capability CLI flag at process level, not function level. WebAssembly sandbox is memory-level (no exfil within the wasm linear memory) but does not constrain which host imports a module can call at the per-function level in the source language.

**Buildable today:** The compile-time check works for annotated code. Unannotated agent scaffolding is not constrained. Deny-by-default (`--strict-capabilities`) is the piece that makes it airtight.

---

### Unlock 3: Capability manifest as a machine-readable security contract

**Novelty: TRULY-NOVEL (as integrated language feature)**

No mainstream language produces a per-function capability manifest as a first-class compiler artifact. The `fn_capabilities` map built in `build_fn_capability_map` is available during compilation and could be serialized into the registry package index. This means:

- A CI gate can assert "this PR introduced no new `ffi` or `process` capabilities to public API functions" without running the code.
- A security review can grep the manifest rather than reading implementation files.
- A registry can badge packages: "compute-only" (no net/io/ffi) vs "network-capable" vs "system-access."

Who else does this: The Deno `deno info` command shows permissions at the file level, not per-function. Rust crates.io has no capability manifest. npm has no capability manifest. The closest is the WIT file in wasm components, which is a design-time contract, not compiler-extracted.

**Buildable today:** The data is there in the compiler. The missing piece is a `kryos manifest --caps` subcommand that serializes `fn_capabilities` to JSON. That is a one-crate addition, no language changes needed.

---

### Unlock 4: Attenuation-safe plugin architecture

**Novelty: PARTIAL**

The attenuation rule (E0503) means that a host application with `@capabilities(filesystem:read)` (once sub-caps are implemented) cannot be escalated by a plugin that declares `@capabilities(filesystem)`. The capability ceiling is enforced transitively through the call graph for annotated code.

This is a real structural advantage over:
- Python importlib plugins: no restriction, full process capabilities.
- Node.js `vm.runInContext`: memory sandbox only, no I/O restriction.
- Lua `load` with env sandbox: possible but manual, not compiler-enforced.
- Java SecurityManager: runtime-only, deprecated since Java 17.

The gap: without deny-by-default, a plugin written without annotations is unconstrained. The attenuation only fires when both the host function and the plugin function carry `@capabilities(...)`.

**Buildable today:** Works for annotated plugin functions. Requires `--strict-capabilities` for full coverage of unannotated code.

---

## What needs language work first

### Gap 1: Deny-by-default (`--strict-capabilities`)

This is the single most important missing piece. Without it, all the above unlocks have a bypass: write a function without `@capabilities(...)` and the checker ignores it. Once `--strict-capabilities` is implemented, unannotated functions are treated as `@capabilities(compute)` -- pure computation only. This turns Kryos's capability system from "opt-in documentation" into "a real security boundary."

**Language work required:** A compiler flag that changes the `has_annotated_scope()` guard in `check_callee_capabilities` to treat every function as if it were annotated with an empty set. Conceptually simple; the harder part is computing the implied capability set for functions that call other unannotated functions transitively.

### Gap 2: Sub-capabilities

`docs/10-capabilities.md` describes `filesystem:read`, `filesystem:write`, `network:http`, `network:raw_socket` etc. The code does not have them. `Capability::from_str` has no colon-path parser. Without sub-capabilities, you cannot express "this function may read files but not write them," which is one of the most important least-privilege patterns.

**Language work required:** Extend `Capability` to a `Capability::Sub(base, sub)` variant or a `CapabilityPath` struct, add colon-parsing in `from_str`, update `is_subset_of` to understand the hierarchy.

### Gap 3: Capability witnesses / typed values

Currently capabilities are a function-level annotation, not a type-level property. There is no way to express "this value was produced by a `@capabilities(net)` function and should only be consumed by one." This would enable capability-typed channels: a `NetHandle` that can only be passed to functions in a `@capabilities(net)` scope.

**Proposed addition:** A `cap<T, C>` type constructor where `C` is a capability set. `cap<str, net>` is a string that can only be created inside a `net`-capable scope and only used in one. This makes capability boundaries expressible in data types, not just function annotations.

### Gap 4: Runtime `CapabilityEnforcer`

The doc describes a runtime scope stack with uncatchable `CapabilityViolation` and `SandboxEscapeAttempt` exceptions. This does not exist in `kryos-rt` yet (the `_budget` and `_sandbox` fields are ignored). Without runtime enforcement, a compiled binary produced by a buggy or malicious compiler has no second line of defense.

**Language work required:** Implement the scope stack in `kryos-rt`; integrate with the Cranelift and LLVM backends to emit scope-push/pop calls around function entry/exit.

### Gap 5: Signed capability manifests

For the plugin marketplace story to be trustworthy, the capability manifest extracted from a package needs to be signed by the compiler and verifiable by the registry and by `kryos pkg add`. Otherwise a malicious package could ship a manifest claiming `compute-only` while the actual binary exfiltrates data.

**Language work required:** Not a language change -- a registry/toolchain integration: `kryos build` signs the capability manifest with the developer's key, and `kryos pkg add` verifies the manifest signature matches the binary's embedded hash.

---

## Proposed Kryos functions / language additions

### 1. `capabilities_of(fn_path) -> CapabilitySet`

A comptime intrinsic that returns the declared capability set of a function at compile time.

```kryos
@comptime
fn is_pure(f: fn) -> bool {
    let caps = capabilities_of(f)
    return caps.is_subset_of(@capabilities(compute))
}
```

Enables writing higher-order functions that refuse non-pure callbacks at compile time.

### 2. `cap<T, C>` -- capability-typed values

A type constructor binding a value to a capability requirement.

```kryos
// Only constructible in a net-capable scope
@capabilities(net)
fn open_connection(host: str) -> cap<TcpStream, net> { ... }

// Only callable in a net-capable scope
@capabilities(net)
fn send(conn: cap<TcpStream, net>, data: str) { ... }
```

This makes capability leakage a type error: you cannot smuggle a `cap<TcpStream, net>` into a `@capabilities(compute)` function because the type system rejects it.

### 3. `@capability_manifest` module attribute

Instructs the compiler to emit a `<package>.caps.json` alongside the binary:

```kryos
@capability_manifest
module my_agent { ... }
```

Output: `{ "functions": { "run": ["net", "db"], "process": ["compute"] }, "max_capability": ["net", "db"] }`

Consumable by CI, registry, and security tooling.

### 4. `@attest(capabilities(net, compute))` -- caller-side assertion

An annotation on a call site that asserts the callee's capabilities do not exceed the given set. A compile error if the callee's manifest disagrees.

```kryos
@capabilities(net, compute, io)
fn run_plugin(code: str) {
    @attest(capabilities(compute))
    let result = eval_plugin(code)
    // If eval_plugin has @capabilities(net) anywhere in its call graph,
    // this is a compile error.
}
```

### 5. `kryos manifest` CLI subcommand

Extracts the capability manifest for a package and outputs JSON or a human-readable table:

```
kryos manifest --caps ./my_agent.kry
Function          Capabilities
─────────────────────────────────
main              net, db
process_response  compute
log_event         io
```

Buildable without any language changes -- just serializes `fn_capabilities` from the checker.

---

## Honest summary

**What Kryos has today:** A real, working compile-time capability checker that enforces declared constraints transitively through annotated call graphs. The attenuation rule and escalation detection are implemented and tested. The `@budget` integration in `kryos-rt` (separate from `kryos-capabilities`) adds runtime enforcement of token/call budgets.

**What it does not have:** Deny-by-default, sub-capabilities, runtime capability enforcement, audit logging, sandboxing, signed manifests. These are the features that would make "capability-proven" a meaningful security claim rather than a documentation feature.

**Differentiated positioning (honest):** No mainstream general-purpose language has per-function capability sets that propagate transitively through the call graph as a first-class compiler feature. The closest analog is the wasm component model's wit interface, but that is a module-level boundary, not source-language per-function. Rust's `unsafe` is a single boolean. Deno's permissions are process-level flags. Java's SecurityManager is dead. OCaml effects do not cover I/O capabilities. On the axis of "language-integrated, source-visible, per-function capability propagation," Kryos is genuinely differentiated -- but the opt-in nature today significantly weakens the claim.

The path to "capability-proven" as a real product claim requires, in order: (1) `--strict-capabilities` / deny-by-default, (2) sub-capabilities, (3) `kryos manifest` CLI, (4) registry manifest signing. Items 3 and 4 are the go-to-market items; items 1 and 2 are the correctness items that make the story not a lie.
