# Kryos Language Manual

The complete reference for the Kryos programming language.

Kryos is a compiled systems language with ownership-based memory safety, compile-time evaluation, capability-based security, and a first-class AI runtime. It compiles to LLVM IR for native binaries or runs interpreted with self-healing error recovery.

## Quick Start

```
// hello.kry
struct Point {
    x: f64,
    y: f64
}

fn distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x
    let dy = a.y - b.y
    return sqrt(dx * dx + dy * dy)
}

let origin = Point { x: 0.0, y: 0.0 }
let target = Point { x: 3.0, y: 4.0 }

for i in range(0, 5) {
    println("Step " + to_string(i) + ": distance = " + to_string(distance(origin, target)))
}
```

```bash
kryos run hello.kry
```

## Table of Contents

### Language

- [Getting Started](01-getting-started.md) -- Installation, first program, CLI overview, project structure
- [Variables and Types](02-variables-and-types.md) -- `let`, `let mut`, type annotations, numeric types, type inference
- [Functions](03-functions.md) -- `fn` declarations, first-class functions, closures, lambdas, recursion
- [Control Flow](04-control-flow.md) -- `if`/`elif`/`else`, `while`, `for`/`in`/`range`, `break`, `continue`, `match`
- [Structs and Enums](05-structs-and-enums.md) -- Data modeling, field access, `impl` blocks, methods, enum variants
- [Traits and Generics](06-traits-and-generics.md) -- Trait declarations, generic types, trait bounds, `impl` for traits
- [Ownership and Borrowing](07-ownership-and-borrowing.md) -- Move semantics, borrow checker, copy types, use-after-move, mutation-while-borrowed
- [Modules and Imports](08-modules-and-imports.md) -- `use`, `mod`, package resolution, `extern use`
- [Error Handling](09-error-handling.md) -- Self-healing runtime, `Result`, `Option`, heal actions, `heal-report`
- [Compile-Time Evaluation](10-comptime.md) -- `comptime` blocks, constant embedding, compile-time computation
- [Capability-Based Security](11-capabilities.md) -- `@capabilities`, deny-by-default, capability tiers, auditing with `kryos check`
- [Attributes and Decorators](12-attributes.md) -- `@export`, `@differentiable`, `@compute`, `@target`, `@layout`, `@no_std`
- [Concurrency](13-concurrency.md) -- Actors, `spawn`, `parallel for`, message passing
- [FFI](14-ffi.md) -- Python FFI (`py_import`, `py_call`), C FFI (`c_load`, `c_call`), type marshaling
- [LLVM Compilation](15-llvm-compilation.md) -- Codegen pipeline, `kryos build`, emitting IR, compiling to native binaries

### Standard Library

- [Core Built-ins](stdlib-core.md) -- 36 always-available functions: I/O, math, strings, arrays, type conversion
- [std::collections](stdlib-collections.md) -- `map`, `filter`, `reduce`, `sort`, `zip`, `enumerate`, `find`, `any`, `all`, `flat_map`, `sum`, `count`
- [std::io](stdlib-io.md) -- File I/O, directories, paths, glob, environment variables, temp files
- [std::net](stdlib-net.md) -- HTTP client, WebSocket, TCP sockets, URL encoding
- [std::term](stdlib-term.md) -- Terminal control, cursor, colors, raw mode, key reading, alt screen
- [std::crypto](stdlib-crypto.md) -- SHA-256, SHA-512, MD5, HMAC, Base64, hex, random bytes, UUID
- [std::json](stdlib-json.md) -- `json_parse`, `json_stringify`, `json_get`, `json_has`
- [std::map](stdlib-map.md) -- Dictionary operations: `map_new`, `map_set`, `map_get`, `map_has`, `map_keys`, `map_values`, `map_remove`, `map_merge`
- [std::math](stdlib-math.md) -- `round`, `log10`, `random`, `pi`, `e` (extends core math)
- [std::string](stdlib-string.md) -- `repeat`, `pad_left`, `pad_right`, `lines`, `to_int`, `to_float`, `index`, `count`, `to_upper`, `to_lower`
- [std::process](stdlib-process.md) -- `exec`, `exec_capture`, `exec_timeout`, `sleep`

### AI Runtime

- [Tensors and Autodiff](ai-tensors.md) -- N-dimensional tensors, shape tracking, broadcasting, reverse-mode autodiff
- [Agents](ai-agents.md) -- Autonomous agents, persistent memory, alignment modes, swarms
- [Probability Types](ai-probable.md) -- `Probable<T>`, confidence propagation, ensemble strategies
- [Reactive Streams](ai-streams.md) -- Lazy streams, windowing, backpressure, parallel map, merge
- [Data Lineage](ai-lineage.md) -- Provenance tracking, `Tracked.source`, transform audit trails
- [Cost Tracking](ai-cost.md) -- `Budget`, `CostTracker`, token/API/USD/energy accounting

### Appendix

- [Operator Reference](appendix-operators.md) -- Full operator table with precedence
- [Keyword Reference](appendix-keywords.md) -- All reserved words
- [CLI Reference](appendix-cli.md) -- Complete `kryos` command reference (17 commands)
- [License Tiers](appendix-license-tiers.md) -- Community, Pro, Enterprise, Cloud feature comparison
- [Architecture](appendix-architecture.md) -- Compiler pipeline, implementation map, file-by-file breakdown
