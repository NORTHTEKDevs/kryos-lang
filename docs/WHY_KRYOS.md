# Why Kryos?

Kryos is a compiled systems language: memory-safe without lifetime annotations (ARC + move semantics, a Swift-like trade-off), with the simplicity of Go and AI-native capabilities neither offers. It is NOT equivalent to Rust's borrow-checker guarantees — see "What Kryos Is Not".

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

Under `--strict-capabilities` (the default for new `kryos new` projects), this is deny-by-default: a function with no capability annotation cannot perform I/O, network access, or use hardware accelerators, so supply-chain attacks that sneak network calls into utility functions become compile errors. Capability checking is not globally deny-by-default outside strict mode.

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

| Benchmark (medians of 5) | Kryos LLVM | Rust -O | ratio vs Rust |
|---|---|---|---|
| hashmap 1M+1M | 0.080s | 0.118s | 0.68x (beats Rust) |
| matmul 512² | 0.618s | 0.653s | 0.95x (beats Rust) |
| mandelbrot 1000²×1000 | 0.368s | 0.368s | 1.00x |
| fib(40) | 0.349s | 0.347s | 1.01x |
| fannkuch-redux(10) | 0.197s | 0.195s | 1.01x |
| nbody 2M steps | 0.141s | 0.105s | 1.34x (beats clang/clang++ -O2) |
| binary_trees d16 | 1.098s | 0.759s | 1.45x |

All 7 benchmarks land within 1.45x of Rust, and Kryos beats Rust outright on
matmul (0.95x) and hashmap (0.68x). The honest worst case is binary_trees at
1.45x (Rc-like `Shared` refcount traffic vs Rust's unique-ownership `Box`);
nbody (1.34x) still beats clang/clang++ -O2. Full methodology, spreads, and
the per-benchmark analysis: [BENCHMARKS.md](../BENCHMARKS.md).

The compiler applies five MIR-level optimization passes before handing off to the backend:

1. **Constant folding** -- evaluates compile-time-known expressions
2. **Dead code elimination** -- removes unreachable blocks and unused variables
3. **Function inlining** -- inlines small functions to reduce call overhead
4. **Tail-call optimization** -- converts tail-recursive functions to loops
5. **Strength reduction** -- replaces expensive operations with cheaper equivalents

These optimizations improve debug build performance significantly. Release builds stack them on top of LLVM's own optimization pipeline.

## Status

Kryos 1.0.0-beta.1 is a feature-complete compiler (beta: one primary author, not yet externally stress-tested) with:

- 21-crate Rust implementation (~50,000 lines)
- Dual backends: Cranelift (fast dev, ~500ms) and LLVM (optimized release; see BENCHMARKS.md for measured ratios vs Rust/C)
- 1,000+ Rust tests plus a Cranelift/LLVM backend-parity matrix, 0 clippy warnings
- Self type in traits, associated function syntax (`Type::method()`)
- @pure CSE/dead-call optimization, @test runner, @copy struct deep-copy on assignment (both backends; param passing documented in gotcha #23)
- 847 functions across 28 standard library modules (0 stubs)
- Full toolchain: LSP, formatter, doc generator, package manager, test runner, REPL with persistent state
- Self-hosting compiler: ~19K lines of Kryos implementing the full pipeline (stage-1 is Cranelift-compiled — a different backend — so it is not byte-identical to later stages)
- Bootstrap fixed point: SHA-256 proof that stage-2, stage-3, and stage-4 binaries are byte-identical, reached with the ownership and type checkers disabled on the self-host source (`--skip-ownership` / `KRYOS_SKIP_TYPES=1`); see `compiler/self-host/bootstrap-win.sh`. The per-module standalone compile check (`compiler/self-host/test_bootstrap.sh`) currently passes 11/16 modules.

## Who Is Kryos For?

- **AI/ML engineers** who want native tensor operations without Python overhead
- **Systems programmers** who want memory safety without the borrow-checker learning cliff (accepting ARC's trade-offs)
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
