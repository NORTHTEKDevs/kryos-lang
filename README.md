# Kryos

A compiled systems language with ownership-based memory safety, capability-based security, and a first-class AI runtime. Tensors, agents, probability types, and reactive streams are language primitives, not library imports.

The compiler is a native Rust implementation (21 crates, 40k+ lines) with dual backends: Cranelift for fast debug builds and LLVM for optimized release binaries. 28 stdlib modules (11k+ lines of Kryos), 190+ built-in functions.

## Quick Start

```bash
# Build the compiler (Rust 1.75+)
git clone https://github.com/FrostbyteDevTeam/kryos-lang.git
cd kryos-lang/compiler
cargo build --release

# Run a program
./target/release/kryos run examples/demo.kry

# Compile to native binary (debug, fast -- Cranelift)
./target/release/kryos build examples/demo.kry -o demo

# Compile optimized (requires LLVM toolchain)
./target/release/kryos build examples/demo.kry --release -o demo
```

Hello world:

```
fn main() {
    println("Hello, Kryos!")
}
```

## Language Features

### Variables and Types

Immutable by default. Explicit `mut` for mutability. Type inference from initializers.

```
let x: i64 = 42
let name = "Kryos"
let mut counter = 0
counter = counter + 1

// Numeric types: i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, f32, f64
// Other primitives: bool, str
```

### Functions

First-class values. Pass as arguments, return from functions, store in variables.

```
fn fibonacci(n: i64) -> i64 {
    if n <= 1 { return n }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

fn apply_twice(f: fn(i64) -> i64, x: i64) -> i64 {
    return f(f(x))
}

fn double(x: i64) -> i64 { return x * 2 }

fn main() {
    println(to_string(apply_twice(double, 3)))  // 12
}
```

### Structs and Enums

```
struct Point { x: f64, y: f64 }

impl Point {
    fn distance(self: Point) -> f64 {
        return sqrt(self.x * self.x + self.y * self.y)
    }
}

enum Shape {
    Circle(f64),
    Rect(f64, f64)
}
```

### Traits and Generics

Traits with default methods, generics with trait bounds, monomorphization.

```
trait Printable {
    fn to_display(self: Self) -> str
}

fn identity<T>(value: T) -> T {
    return value
}
```

### Ownership

Move semantics enforced at compile time. No garbage collector. Primitive types (integers, floats, bools) are copy types.

```
let data = [1, 2, 3]
let copy = data          // data is MOVED
// println(data)         // COMPILE ERROR: use of moved value

let x: i64 = 42
let y = x                // x is COPIED (primitive)
println(to_string(x))    // OK
```

### Error Handling

```
try {
    throw "something went wrong"
} catch e {
    println("Caught: " + e)
}
```

### Concurrency

OS threads via `spawn`, actors for stateful message passing, channels for typed communication.

```
fn main() {
    let ch = chan()

    spawn {
        send(ch, 42)
    }

    let value = recv(ch)
    println(to_string(value))  // 42
}

actor Counter {
    let mut count: i64 = 0
    fn increment(amount: i64) { count = count + amount }
    fn get_count() -> i64 { return count }
}
```

### Capability-Based Security

Functions declare what system resources they need. Enforced at compile time.

```
@capabilities(net, io)
fn download(url: str, path: str) {
    let data = http_get(url)
    file_write(path, data)
}
```

### FFI

Call native Rust/C functions directly via `extern` blocks.

```
extern {
    fn kryos_tensor_rand(shape_ptr: i64, ndim: i64) -> i64
    fn kryos_tensor_matmul(a: i64, b: i64) -> i64
}
```

## AI-Native Runtime

### Tensors

38 native FFI functions backed by Rust. Creation, element-wise ops, reductions, linear algebra, ML ops.

```
extern {
    fn kryos_tensor_rand(shape_ptr: i64, ndim: i64) -> i64
    fn kryos_tensor_matmul(a: i64, b: i64) -> i64
    fn kryos_tensor_relu(handle: i64) -> i64
    fn kryos_tensor_softmax(handle: i64, dim: i64) -> i64
}

fn main() {
    let w_shape = [4, 8]
    let x_shape = [2, 4]
    let x = kryos_tensor_rand(x_shape as i64, 2)
    let w = kryos_tensor_rand(w_shape as i64, 2)
    let hidden = kryos_tensor_relu(kryos_tensor_matmul(x, w))
}
```

### Agents

Autonomous entities with three-tier memory, alignment modes, tool use, and swarm coordination.

```
let agent = agent_new("researcher", "Find relevant papers")
agent.memory = agent.memory.remember("query", "transformers", "working")
let child = agent.spawn_child("worker", "Process batch 1")
```

### Probability Types

Confidence-aware values with ensemble methods.

```
let result = probable("cat", 0.92)
if result.is_confident(0.8) { /* act on it */ }
let consensus = ensemble_majority_vote(predictions)
```

### Reactive Streams

Lazy, composable stream processing.

```
let result = stream_from_range(0, 1000)
    .filter(fn(x) { return x % 2 == 0 })
    .map(fn(x) { return x * x })
    .take(10)
    .collect()
```

### Data Lineage and Cost Tracking

Track data provenance and enforce compute budgets.

```
let data = tracked_source(raw_data, "database", "Customer records Q4")
let budget = budget_new(10.0, 100000, 500)  // $10, 100k tokens, 500 API calls
```

## Architecture

```
Source (.kry)
    |
    v
  Lexer (80+ token types)
    |
    v
  Parser (recursive descent + Pratt expressions)
    |
    v
  Type Checker (inference, validation, capability tracking)
    |
    v
  Ownership Analyzer (move tracking, use-after-move detection)
    |
    v
  Capability Checker (deny-by-default enforcement)
    |
    v
  MIR Lowering (SSA basic blocks, monomorphization)
    |
    v
  +--------------------+--------------------+
  |                    |                    |
  v                    v                    v
Cranelift JIT       LLVM Codegen         LSP Server
(fast debug)        (optimized release)  (diagnostics,
                                          hover, completion)
```

### 21 Crates

| Crate | Purpose |
|-------|---------|
| `kryos-cli` | CLI entry point (build, run, test, check, repl, fmt, lsp, pkg) |
| `kryos-lexer` | Tokenizer (80+ token types, string interpolation) |
| `kryos-parser` | Recursive descent parser with Pratt expression parsing |
| `kryos-ast` | AST node types |
| `kryos-types` | Type checker with inference |
| `kryos-ownership` | Ownership analysis (move tracking, use-after-move) |
| `kryos-capabilities` | Compile-time capability enforcement |
| `kryos-mir` | Mid-level IR (SSA, basic blocks, monomorphization) |
| `kryos-codegen-cranelift` | Cranelift JIT backend (debug builds) |
| `kryos-codegen-llvm` | LLVM IR backend (release builds) |
| `kryos-linker` | Native binary linking |
| `kryos-driver` | Compilation pipeline orchestration |
| `kryos-rt` | Runtime library (strings, arrays, maps, tensors, channels, spawn) |
| `kryos-stdlib-native` | Native stdlib (process, file I/O, terminal) |
| `kryos-lsp` | Language Server Protocol |
| `kryos-fmt` | Code formatter |
| `kryos-doc` | Documentation generator |
| `kryos-bindgen` | C header → Kryos binding generator |
| `kryos-package` | Package manager |
| `kryos-test-runner` | Test framework |
| `kryos-errors` | Diagnostic reporting |

## CLI

```
kryos build <file>       # Compile to native binary
kryos run <file>         # Compile and run
kryos check <file>       # Type-check without compiling
kryos test               # Run project tests
kryos repl               # Interactive REPL
kryos fmt <file>         # Format source
kryos lsp                # Start language server
kryos pkg init           # Create new project
kryos pkg add <dep>      # Add dependency
kryos version            # Version info
```

## Standard Library

28 stdlib modules covering: strings, math, collections, I/O, networking, crypto, JSON, regex, datetime, terminal, process management, tensors, agents, probability, streams, data lineage, cost tracking, and more.

See the [full manual](docs/README.md) for complete API documentation.

## Examples

- `examples/demo.kry` -- Language showcase: recursion, higher-order functions, float math, error handling, maps, tensors, process access
- `examples/neural_net.kry` -- Two-layer neural network forward pass using the native tensor runtime

## License

Proprietary. Copyright FrostByte Digital. All rights reserved.
