# Kryos Language v1.0 Blocker Resolution — Design Document

**Date:** 2026-04-01
**Scope:** Kryos Language (kryos-lang repo)
**Status:** Approved

---

## 1. Dynamic Array Codegen (LLVM)

**Current state:** Arrays are bare heap pointers via `malloc()`. `push()` is stubbed at `codegen.py:933-936`. `len()` warns that length isn't tracked.

**Design: Metadata Struct**

Replace bare pointers with a struct in LLVM IR:

```llvm
%Array_i32 = type { i32, i32, i32* }  ; { length, capacity, data* }
```

- `_gen_array_literal()`: Allocate struct + data buffer. Set length = element count, capacity = length (or minimum 8). Store elements via GEP into data pointer.
- `push(arr, val)`: Load length and capacity. If `length < capacity`, store at `data[length]`, increment length. If full, `realloc` data to `capacity * 2`, update capacity field, then store.
- `len(arr)`: Load the length field (GEP index 0). No runtime calculation needed.
- Array indexing: Bounds check `index < length`, then GEP into data pointer.
- `pop(arr)`: Decrement length, return `data[length]`. No realloc (shrink lazily).

**Changes required:**
- `codegen.py:_gen_array_literal()` — Emit struct allocation instead of bare pointer
- `codegen.py:_gen_push()` — Full implementation with capacity check and realloc
- `codegen.py:_gen_builtin_call()` for `len` — Read from struct field
- `codegen.py:_gen_index()` — Bounds check + GEP through struct data pointer
- `codegen.py:_gen_pop()` — Decrement length, return last element
- Tests: Push, pop, len, indexing, capacity growth, empty array operations

## 2. Actor Model Concurrency

**Current state:** `spawn` syntax works in parser and interpreter (OS threads). `@actor` token defined but not implemented. Rust VM has green thread opcodes (Spawn, Yield, Resume, ChanNew, ChanSend, ChanRecv).

**Design:**

### Syntax

```kryos
@actor
@capabilities("network")
fn worker(inbox: chan<Message>) {
    for msg in inbox {
        match msg {
            Request(data, reply) => send(reply, process(data))
            Shutdown => break
        }
    }
}

let ch = chan<Message>()
spawn worker(ch)
send(ch, Request(payload, reply_chan))
let result = recv(reply_chan)
```

### New Primitives

- `chan<T>()` — Create a typed channel (extends existing ChanNew)
- `send(ch, value)` — Send a value into a channel (moves ownership)
- `recv(ch)` — Receive a value from a channel (blocks until available)
- `select { ch1 => handler1, ch2 => handler2, timeout(5s) => handler3 }` — Multiplex across channels
- `ask(ch, msg)` — Send message with ephemeral reply channel, return response (request/response pattern)

### `@actor` Annotation

- Marks a function as an actor with isolated capability scope
- Actor capabilities cannot exceed parent's capabilities (attenuation only)
- Actor gets its own heap region (no shared mutable state)
- Messages are moved into channels, not shared

### Changes required

- `tokens.py`: Add `SELECT`, `SEND`, `RECV`, `ASK`, `CHAN` tokens
- `lexer.py`: Recognize new keywords
- `parser.py`: Parse `chan<T>()`, `send()`, `recv()`, `select {}`, `ask()`
- `ast_nodes.py`: New nodes — `ChanExpr`, `SendStmt`, `RecvExpr`, `SelectStmt`, `AskExpr`
- `interpreter.py`: Implement via Python threading + queue.Queue
- `codegen.py`: Emit opcodes for Rust VM channel operations
- `capabilities.py`: Enforce actor capability attenuation
- Tests: Channel send/recv, select multiplexing, ask pattern, capability isolation

## 3. Safety Architecture (Actor/Agent Scope Only)

**Activates when:** `@actor` is used, spawn depth > 1, or `@capabilities("autonomous")` is declared. Standard applications are unaffected.

### Capability Immutability Principle

Capabilities are sealed at compile time. No runtime mechanism — including self-healing, agent memory, tool invocation, actor messaging, or dynamic code loading — can escalate, modify, or bypass declared capabilities. Violation attempts are logged and trigger immediate actor termination.

### Self-Healing Restrictions

Self-healing can NEVER escalate capabilities. Permitted actions: retry, coerce types, clamp bounds, substitute defaults. Prohibited actions: add capabilities, widen sandbox paths, increase budget ceilings, remove spawn limits, modify capability annotations.

### Resource Budgets

```kryos
spawn worker(ch) @budget(
    max_cpu: 10s,
    max_memory: 64mb,
    max_spawn: 5,
    max_depth: 3,
    max_network_calls: 100
)
```

Budget enforcement is runtime-level, non-catchable. Exceeding any limit kills the actor immediately.

### Spawn Limits

- Global max actor count per process (default 1000, configurable)
- Max actor tree depth (default 5)
- Self-spawning requires `@allow_self_spawn` + `Admin` capability tier

### Kill Switch

`halt!` primitive: kills all actors, all green threads, all spawned processes. Non-catchable, non-interceptable by self-healing. Flushes audit log before exit.

Deadman's switch: actors heartbeat to supervisor within configurable interval. Missed heartbeat = killed.

### Sandbox Boundaries

```kryos
@capabilities("filesystem")
@sandbox(paths: ["/app/data", "/tmp"])
@sandbox(hosts: ["api.example.com"])
fn my_actor() { ... }
```

Filesystem and network access restricted to declared paths/hosts. Violations = actor termination.

### Audit Trail

Mandatory for actors with `network` or `filesystem` capabilities. Append-only log of all actions. Actors cannot delete or modify their own logs.

### Human-in-the-Loop Gate

`@capabilities("autonomous")` opts into unsupervised operation. Without it, certain actions require runtime human approval: spawning > N actors, network calls to undeclared hosts, writes outside sandbox, financial transactions. `autonomous` requires `Admin` tier licensing.

### Changes required

- `capabilities.py`: Capability attenuation enforcement, immutability checks
- `interpreter.py`: Budget tracking and enforcement on spawn
- `interpreter.py`: Self-healing restriction — check operation against allowed list
- New: `safety.rs` in Rust VM — budget enforcement, spawn counting, deadman switch
- New: `audit.rs` in Rust VM — append-only action log
- `tokens.py`/`parser.py`: Parse `@budget(...)`, `@sandbox(...)`, `halt!`
- `codegen.py`: Emit budget/sandbox metadata alongside actor code
- Tests: Budget exhaustion, capability escalation rejection, kill switch, audit immutability

## 4. WASM Target

**Current state:** Listed in Community tier licensing. Zero implementation.

**Design: LLVM-Based with JS FFI**

- `kryos build --target wasm32 app.kry` — Pipes LLVM IR through `llc -march=wasm32 -filetype=obj` + `wasm-ld`
- `@export` annotation marks functions as WASM module exports
- `@import("js")` annotation declares JavaScript host functions
- `std::web` stdlib module wraps JS interop for DOM access

```kryos
@export
fn fibonacci(n: i32) -> i32 {
    if n <= 1 { return n }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

@import("js")
fn console_log(msg: str)

@import("js")
fn document_querySelector(selector: str) -> JsRef
```

### Changes required

- `codegen.py`: Add `--target wasm32` support, emit wasm32-compatible IR (different memory model, no syscalls)
- `codegen.py`: Handle `@export` → WASM export table
- `codegen.py`: Handle `@import("js")` → WASM import declarations
- `cli.py`: Add `--target` flag to `kryos build`, invoke wasm-ld
- New: `kryos/stdlib/web_module.py` — DOM bindings via JS FFI
- New: `kryos/wasm/` — WASM-specific lowering and linking
- Tests: Simple function export, JS import, DOM binding smoke test

## 5. Rust VM Stabilization

**Current state:** `kryos run` dispatches to Rust VM via subprocess. AST serialized to JSON. Fallback to Python interpreter if binary missing.

**Design: CI Matrix Testing**

Run all 20 integration test programs (01_basics.kry through 19_spawn.kry) on BOTH runtimes in CI:

```yaml
matrix:
  runtime: [python, rust-vm]
steps:
  - run: kryos test tests/programs/ --runtime ${{ matrix.runtime }}
```

Add `--runtime` flag to `kryos test`:
- `--runtime python` — Force Python interpreter
- `--runtime rust` — Force Rust VM (fail if binary missing)
- Default: Rust VM with Python fallback (current behavior)

Capture and diff outputs between runtimes. Any divergence = test failure.

### Changes required

- `cli.py`: Add `--runtime` flag to `cmd_test` and `cmd_run`
- `cli.py`: Skip fallback when `--runtime rust` is explicit
- `.github/workflows/ci.yml`: Add matrix for runtime selection
- Test runner: Compare stdout between runtimes, report divergences

## 6. Performance Benchmarks

**Current state:** No benchmarking infrastructure.

**Design: `kryos bench` Command**

Five benchmark programs:

1. `fibonacci.kry` — Recursive fib(35). Tests: function call overhead, recursion
2. `matrix.kry` — 100x100 matrix multiply. Tests: loops, array access, arithmetic
3. `strings.kry` — String concatenation + parsing (10K iterations). Tests: allocation, GC pressure
4. `sort.kry` — Quicksort on 10K element array. Tests: collections, comparisons, swaps
5. `http_bench.kry` — Spawn 100 HTTP handlers, process requests. Tests: concurrency, I/O

Each runs on: Python interpreter, Rust VM (interpreter mode), Rust VM (JIT mode), LLVM native binary.

Output: comparison table with wall-clock time, relative speedup vs Python baseline.

Baselines stored in `benchmarks/baselines.json`. Regression = >10% slowdown from baseline.

### Changes required

- New: `benchmarks/` directory with 5 .kry programs
- New: `kryos/cli_commands/bench_cmd.py` — benchmark runner
- `cli.py`: Register `kryos bench` command
- Baseline JSON storage and comparison logic

## 7. GitHub-Backed Package Registry

**Current state:** Local registry at `~/.kryos/packages/`. `kryos publish` copies to local directory. No remote.

**Design:**

- `kryos add github:user/package@1.0.0` — Fetches tagged GitHub release tarball, extracts to `~/.kryos/packages/user/package/1.0.0/`
- Version pinning: `^1.2.3` (compatible), `~1.2.0` (patch-level), `=1.2.3` (exact)
- Lock file: `kryos.lock` — records exact resolved versions and GitHub commit SHAs
- `kryos install` — Reads `kryos.toml` dependencies, resolves versions, fetches from GitHub
- `kryos publish` — Stays local-only. GitHub releases ARE the publish mechanism (create a tagged release on GitHub, others can `kryos add` it)

### kryos.toml format

```toml
[dependencies]
http-utils = { github = "user/http-utils", version = "^1.0.0" }
json-tools = { github = "user/json-tools", version = "~2.1.0" }
local-pkg = { path = "../local-pkg" }
```

### Changes required

- `packages.py`: GitHub release fetching (HTTP GET to GitHub API, download tarball)
- `packages.py`: Semver range resolution (`^`, `~`, `=`)
- `packages.py`: Lock file generation and reading (`kryos.lock`)
- `packages.py`: Transitive dependency resolution (BFS on dependency graph)
- `cli.py`: Update `kryos add` to accept `github:user/pkg@version` syntax
- Tests: Version resolution, lock file consistency, transitive deps

---

## Design Principles

- Safety architecture is actor/agent-scoped only — standard apps unaffected
- Capability immutability is a hard invariant — no runtime escalation path
- Self-healing cannot modify capabilities under any circumstance
- Actor model over async/await — spawn + channels + select
- GitHub as package host — no custom registry infrastructure
- WASM via existing LLVM pipeline — minimal new code
- All benchmarks test real workloads, not synthetic microbenchmarks
