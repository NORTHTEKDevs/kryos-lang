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

Kryos values are ARC-backed (reference-counted) heap handles for `str`, `[T]`, `map<K, V>`, and structs/enums; primitives are `Copy`. Passing a value to a function shares it -- there's no destructive move, so reusing the original binding afterward just works. No lifetime annotations. No borrow checker complexity. No `.clone()` needed to keep using a value after passing it.

```kryos
fn process(data: [i64]) -> [i64] {
    push(data, 42)
    data
}

fn main() {
    let nums = [1, 2, 3]
    let nums = process(nums)   // shared in, returned out -- no copies
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
    let x = kryos_tensor_rand(arr_to_ptr(x_shape), 2)
    let w = kryos_tensor_rand(arr_to_ptr(w_shape), 2)
    let hidden = kryos_tensor_relu(kryos_tensor_matmul(x, w))
}
```

The runtime includes 38 tensor operations backed by native Rust, autonomous agents with three-tier memory, confidence-aware probability types with ensemble methods, and lazy composable reactive streams.

### Capability-Based Security

Functions declare what system resources they need. The compiler enforces these constraints at compile time -- no runtime overhead, no surprises.

```kryos
use std::net::{http_get}

@capabilities(net, io)
fn fetch_and_save(url: str, path: str) {
    let data = http_get(url)
    file_write(path, data.body)   // http_get returns HttpResponse; .body is the str
}

// Calling a function that needs 'gpu' from here would be a compile error
```

Under `--strict-capabilities` (the default for new `kryos new` projects), this is deny-by-default: a function with no capability annotation cannot perform I/O, network access, or use hardware accelerators, so supply-chain attacks that sneak network calls into utility functions become compile errors. Capability checking is not globally deny-by-default outside strict mode.

### Compile-Time Evaluation (planned, not implemented yet)

The `comptime` keyword is reserved and parses today, and is *intended* to evaluate expressions during compilation the way Zig's `comptime` blocks do. **It does not yet.** The compiler currently lowers a `comptime { }` block as ordinary runtime code -- it runs in place, once per execution, with full access to outer-scope variables and I/O, exactly like a bare block. The examples below produce the right numbers because runtime evaluation gives the same answer as compile-time evaluation would for pure arithmetic -- not because any folding happens. See [docs/11-comptime.md](11-comptime.md) for the honest current-vs-planned breakdown.

```kryos
let table_size = comptime { 2 * 2 * 2 * 2 * 2 * 2 * 2 * 2 }  // 256, computed at RUNTIME today
let magic = comptime { (100 + 200) * 3 - 50 }                   // 850, computed at RUNTIME today
```

### Dynamic Dispatch When You Need It

Static monomorphization by default (zero-cost abstractions), with `dyn Trait` for runtime polymorphism when flexibility matters.

```kryos
trait Drawable {
    fn draw(self) -> str
}

fn render(item: dyn Drawable) -> str {
    return item.draw()   // runtime dispatch through the trait object
}
```

> For a heterogeneous **collection**, use an enum with one variant per concrete
> type and `match`. Trait objects stored inside a container (`[dyn Drawable]`,
> `Option<dyn Drawable>`, a map value, ...) are rejected at compile time
> (`E0110`) and are not yet supported.

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
| hashmap 1M+1M | 0.082s | 0.127s | 0.65x (beats Rust) |
| matmul 512² | 0.620s | 0.648s | 0.96x (beats Rust) |
| mandelbrot 1000²×1000 | 0.369s | 0.368s | 1.00x |
| fannkuch-redux(10) | 0.202s | 0.198s | 1.02x |
| fib(40) | 0.351s | 0.340s | 1.03x |
| nbody 2M steps | 0.141s | 0.107s | 1.31x (beats clang/clang++ -O2) |
| binary_trees d16 | 1.097s | 0.773s | 1.42x |

All 7 benchmarks land within 1.42x of Rust, and Kryos beats Rust outright on
matmul (0.96x) and hashmap (0.65x). The honest worst case is binary_trees at
1.42x (Rc-like `Shared` refcount traffic vs Rust's unique-ownership `Box`);
nbody (1.31x) still beats clang/clang++ -O2. Re-measured on 1.0.0-rc.2,
2026-07-10. Full methodology, spreads, and the per-benchmark analysis:
[BENCHMARKS.md](../BENCHMARKS.md).

The compiler applies five MIR-level optimization passes before handing off to the backend:

1. **Constant folding** -- evaluates compile-time-known expressions
2. **Dead code elimination** -- removes unreachable blocks and unused variables
3. **Function inlining** -- inlines small functions to reduce call overhead
4. **Tail-call optimization** -- converts tail-recursive functions to loops
5. **Strength reduction** -- replaces expensive operations with cheaper equivalents

These optimizations improve debug build performance significantly. Release builds stack them on top of LLVM's own optimization pipeline.

## Status

Kryos 0.9.0 is a feature-complete compiler (pre-1.0: one primary author, not yet externally stress-tested, two known concurrency release blockers in docs/BUGS.md) with:

- 21-crate Rust implementation (~50,000 lines)
- Dual backends: Cranelift (fast dev, ~500ms) and LLVM (optimized release; see BENCHMARKS.md for measured ratios vs Rust/C)
- 1,000+ Rust tests plus a Cranelift/LLVM backend-parity matrix, 0 clippy warnings
- Self type in traits, associated function syntax (`Type::method()`)
- @pure CSE/dead-call optimization, @test runner, @copy struct deep-copy on assignment (both backends; param passing documented in gotcha #23)
- 1,200+ functions across 66 standard library modules (0 stubs)
- Full toolchain: LSP, formatter, doc generator, package manager, test runner, REPL with persistent state
- Self-hosting compiler: ~19K lines of Kryos implementing the full pipeline (stage-1 is Cranelift-compiled — a different backend — so it is not byte-identical to later stages)
- Bootstrap fixed point: SHA-256 proof that stage-2, stage-3, and stage-4 binaries are byte-identical, reached with the ownership and type checkers disabled on the self-host source (`--skip-ownership` / `KRYOS_SKIP_TYPES=1`); see `compiler/self-host/bootstrap-win.sh`. The per-module standalone compile check (`compiler/self-host/test_bootstrap.sh`) currently passes 16/16 modules.

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
