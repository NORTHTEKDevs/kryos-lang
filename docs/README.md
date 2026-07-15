# Kryos Language Manual

The complete reference for the Kryos programming language.

Kryos is a compiled systems language with ownership-based memory safety, compile-time evaluation, capability-based security, and an AI-native runtime. The compiler is a native Rust implementation (21 crates) that compiles through Cranelift (debug, fast compilation) and LLVM (release, optimized native binaries).

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

fn main() {
    let origin = Point { x: 0.0, y: 0.0 }
    let target = Point { x: 3.0, y: 4.0 }

    for i in range(0, 5) {
        println("Step " + to_string(i) + ": distance = " + to_string(distance(origin, target)))
    }
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
- [Ownership](06-ownership.md) -- ARC-backed value semantics, Copy types, reuse-after-pass, advisory ownership diagnostics
- [Error Handling](07-error-handling.md) -- `try`/`catch`/`throw`, error propagation
- [Traits and Generics](08-traits-and-generics.md) -- Trait declarations, generic types, trait bounds, `impl` for traits
- [Concurrency](09-concurrency.md) -- `spawn`, actors, channels, message passing
- [Capabilities](10-capabilities.md) -- `@capabilities`, deny-by-default, capability tiers
- [Compile-Time Evaluation](11-comptime.md) -- `comptime` blocks, constant embedding
- [Modules and Packages](12-modules-and-packages.md) -- `use` statements, module resolution, package structure
- [FFI](13-ffi.md) -- `extern` blocks, native Rust FFI, type marshaling
- [AI Runtime](14-ai-runtime.md) -- Tensors, agents, probability, streams, lineage, cost tracking
- [Compilation Pipeline](15-codegen.md) -- Cranelift/LLVM backends, MIR, uniform slot model, runtime functions
- [Self-Hosting](20-self-hosting.md) -- Bootstrap chain (stage 0 → 1 → 2), verification, memory model, stability metrics

### Standard Library

- [Core Built-ins](stdlib/core-builtins.md) -- Always-available functions: I/O, math, strings, arrays, type conversion
- [std.collections](stdlib/collections.md) -- `map`, `filter`, `reduce`, `sort`, `zip`, `enumerate`, `find`, `any`, `all`, `flat_map`, `sum`, `count`
- [std.io](stdlib/io.md) -- File I/O, directories, paths, environment variables
- [std.net](stdlib/net.md) -- HTTP client, WebSocket, TCP sockets, URL encoding
- [std.term](stdlib/term.md) -- Terminal control, cursor, colors, raw mode, key reading
- [std.crypto](stdlib/crypto.md) -- SHA-256, SHA-512, MD5, HMAC, Base64, hex, random bytes, UUID
- [std.json](stdlib/json.md) -- `parse`, `stringify`, `pretty_print`, `get`, `set`; constructors `json_string`, `json_number`, `json_object`, `json_array`
- Maps are a built-in type (`map<K, V>`): `m[k]`, `m[k] = v`, `contains(m, k)`, `keys(m)`, `len(m)` -- no import needed (see [std.map](stdlib/map.md))
- [std.math](stdlib/math.md) -- `round`, `log10`, `log2`, `ln`, `gcd`, `lcm`; constants `PI`, `E`, `TAU` (`sqrt`/`pow`/`abs`/`min`/`max` are global builtins)
- [std.string](stdlib/string.md) -- `repeat`, `pad_left`, `pad_right`, `split_lines`, `to_upper`, `to_lower`, `trim`, `find`
- [std.set](stdlib/set.md) -- sorted-array set primitives: `insert`, `contains`, `remove`, `lower_bound` (or use `Set` from `std.collections`)
- [std.process](stdlib/process.md) -- `env_get`, `env_has`, `exit`, `argc`, `argv`, `args`, `command` (subprocess builder)
- [std.datetime](stdlib/datetime.md) -- Date/time operations
- [std.regex](stdlib/regex.md) -- Regular expressions
- [std.db](stdlib/db.md) -- SQLite database access

### Appendix

- [Operator Reference](appendix/operators.md) -- Full operator table with precedence
- [Keyword Reference](appendix/keywords.md) -- All reserved words
- [Attributes Reference](appendix/attributes.md) -- `@capabilities`, `@export`, and other attributes
- [Coming From Other Languages](appendix/coming-from.md) -- Kryos for Rust/JS/C developers
