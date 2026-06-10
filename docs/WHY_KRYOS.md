# Why Kryos?

Kryos is a compiled systems language that combines the safety of Rust with the simplicity of Go and AI-native capabilities that neither offers.

## The Problem

Modern systems programming forces a choice:

- **Rust**: Maximum safety, but steep learning curve and slow compilation
- **Go**: Fast compilation and simplicity, but no memory safety guarantees beyond garbage collection
- **C/C++**: Raw speed, but memory-unsafe and carrying decades of accumulated complexity
- **Zig**: Fine-grained control, but a niche ecosystem with limited tooling

None of these languages treat AI as a first-class concern. Tensor operations, agent coordination, probability-aware types, and reactive streams are afterthoughts bolted on through libraries -- not integrated into the language semantics.

## Kryos: A Pragmatic Middle Ground

### Ownership Without the PhD

Kryos uses ARC-backed move semantics enforced at compile time. Values move when passed to functions. The compiler tracks ownership and catches use-after-move and double-free errors. No lifetime annotations. No borrow checker complexity.

```kryos
fn process(data: [i64]) -> [i64] {
    push(data, 42)
    data
}

fn main() {
    let nums = [1, 2, 3]
    let nums = process(nums)   // move in, move back out -- no copies
    println(to_string(len(nums)))
}
```

Memory is reclaimed deterministically when values go out of scope. No GC pauses. No `'a` annotations. No wrestling with the compiler.

### Faster Than Rust to Compile

Kryos compiles through a dual-backend architecture:

- **Cranelift** for development: fast compilation, quick iteration cycles
- **LLVM** for release: optimized native binaries that match Rust's runtime performance

A hello-world program compiles in ~500ms on the Cranelift backend. Release builds through LLVM produce binaries competitive with `rustc --release`.

### AI-Native Runtime

Tensors, agents, probability types, and reactive streams are language primitives. Machine learning engineers can write inference pipelines without leaving the language or managing Python-to-C++ interop boundaries.

```kryos
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

The runtime includes 38 tensor operations backed by native Rust, autonomous agents with three-tier memory, confidence-aware probability types with ensemble methods, and lazy composable reactive streams.

### Capability-Based Security

Functions declare what system resources they need. The compiler enforces these constraints at compile time -- no runtime overhead, no surprises.

```kryos
@capabilities(net, io)
fn fetch_and_save(url: str, path: str) {
    let data = http_get(url)
    file_write(path, data)
}

// Calling a function that needs 'gpu' from here would be a compile error
```

This is deny-by-default: a function with no capability annotation cannot perform I/O, network access, or use hardware accelerators. Supply chain attacks that sneak network calls into utility functions become compile errors.

### Compile-Time Evaluation

The `comptime` keyword evaluates expressions during compilation. Unlike Rust's limited const evaluation, Kryos supports arithmetic, conditionals, and function calls at compile time.

```kryos
let table_size = comptime { 2 * 2 * 2 * 2 * 2 * 2 * 2 * 2 }  // 256
let magic = comptime { (100 + 200) * 3 - 50 }                   // 850
```

### Dynamic Dispatch When You Need It

Static monomorphization by default (zero-cost abstractions), with `dyn Trait` for runtime polymorphism when flexibility matters.

```kryos
trait Drawable {
    fn draw(self) -> str
}

fn render(items: [dyn Drawable]) {
    // Runtime dispatch -- useful for heterogeneous collections
}
```

### Concurrency Primitives

OS threads via `spawn`, typed channels for communication, actors for stateful message passing, and `select` for multiplexing. No async/await coloring problem.

```kryos
fn main() {
    let ch = chan()
    spawn {
        send(ch, 42)
    }
    let value = recv(ch)
}
```

## Performance

| Benchmark | Kryos (Cranelift) | Kryos (LLVM) | Rust (release) | Go |
|-----------|-------------------|--------------|----------------|----|
| fib(42) | 1190ms | 594ms | 594ms | 1094ms |
| Sum 100M | 46ms | ~5ms | 5ms | 27ms |
| Nested 1Kx1K | 5ms | 5ms | 5ms | 6ms |
| Compilation (hello) | ~500ms | -- | ~600ms | ~720ms |

Kryos LLVM release builds match Rust. Cranelift debug builds prioritize compilation speed over runtime speed.

The compiler applies five MIR-level optimization passes before handing off to the backend:

1. **Constant folding** -- evaluates compile-time-known expressions
2. **Dead code elimination** -- removes unreachable blocks and unused variables
3. **Function inlining** -- inlines small functions to reduce call overhead
4. **Tail-call optimization** -- converts tail-recursive functions to loops
5. **Strength reduction** -- replaces expensive operations with cheaper equivalents

These optimizations improve debug build performance significantly. Release builds stack them on top of LLVM's own optimization pipeline.

## Status

Kryos v2.3.0 is a complete, production-capable compiler with:

- 21-crate Rust implementation (~50,000 lines)
- Dual backends: Cranelift (fast dev, ~500ms) and LLVM (optimized release, Rust parity)
- 925+ tests, all passing, 0 clippy warnings
- Self type in traits, associated function syntax (`Type::method()`)
- @pure CSE/dead-call optimization, @test runner, @copy struct deep-copy on assignment (both backends; param passing documented in gotcha #23)
- 847 functions across 28 standard library modules (0 stubs)
- Full toolchain: LSP, formatter, doc generator, package manager, test runner, REPL with persistent state
- Self-hosting compiler: 19K lines of Kryos implementing the full pipeline, stage-1 verified
- 3-stage bootstrap: SHA-256 identity proof that stage-1, stage-2, and stage-3 binaries are identical

## Who Is Kryos For?

- **AI/ML engineers** who want native tensor operations without Python overhead
- **Systems programmers** who want Rust's safety without the learning cliff
- **Security-conscious teams** who need compile-time capability enforcement
- **Startups** that need fast iteration (fast compilation) AND production performance (LLVM release)

## What Kryos Is Not

Kryos is not a Rust replacement for all use cases. It trades some of Rust's flexibility (named lifetimes, complex trait bounds) for approachability. If you need fine-grained lifetime control across async boundaries, Rust is still the right tool.

Kryos is not garbage collected. It uses deterministic destruction through ownership. If you want a GC, use Go or Java.

## Try It

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang
cd kryos-lang/compiler
cargo build --release -j 4
cargo run --release -- run examples/fibonacci.kry
```
