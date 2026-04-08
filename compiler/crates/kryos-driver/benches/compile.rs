//! Compiler performance benchmarks.
//!
//! Measures each stage of the Kryos compilation pipeline independently,
//! plus full end-to-end paths (`check_source` and `compile_source`).
//!
//! Benchmark groups:
//! - `lex/*`       — lexer throughput on small and large inputs
//! - `parse/*`     — parser throughput
//! - `typecheck/*` — type checking pass
//! - `ownership/*` — ownership analysis pass
//! - `capabilities/*` — capability checking pass
//! - `mir/*`       — MIR lowering (+ comptime eval)
//! - `codegen/*`   — Cranelift AOT compilation
//! - `pipeline/*`  — full end-to-end (check_source, compile_source)
//!
//! Run with: `cargo bench -p kryos-driver`

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};

use kryos_capabilities::check_capabilities;
use kryos_codegen_cranelift::CraneliftBackend;
use kryos_codegen_cranelift::jit::jit_compile_module;
use kryos_driver::{check_source, compile_source, BuildConfig, BuildMode, OutputType};
use kryos_errors::SourceMap;
use kryos_lexer::Lexer;
use kryos_mir::lower_module;
use kryos_ownership::analyze_ownership;
use kryos_parser::parse;
use kryos_types::type_check;

// ---------------------------------------------------------------------------
// Benchmark source programs
// ---------------------------------------------------------------------------

/// Minimal program — baseline for overhead measurement.
const SMALL_PROGRAM: &str = r#"
fn main() {
    let x = 42
    println(to_string(x))
}
"#;

/// Medium program — exercises core language features that pass through
/// the full pipeline: structs, multiple functions, control flow, loops,
/// floating-point math, recursion, higher-order functions, arrays,
/// match expressions, try/catch, and string operations.
const MEDIUM_PROGRAM: &str = r#"
struct Point {
    x: f64
    y: f64
}

struct Rect {
    origin: Point
    width: f64
    height: f64
}

enum Color {
    Red,
    Green,
    Blue,
}

fn distance_sq(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x
    let dy = a.y - b.y
    return dx * dx + dy * dy
}

fn midpoint(a: Point, b: Point) -> Point {
    return Point {
        x: (a.x + b.x) / 2.0,
        y: (a.y + b.y) / 2.0,
    }
}

fn rect_area(r: Rect) -> f64 {
    return r.width * r.height
}

fn rect_perimeter(r: Rect) -> f64 {
    return 2.0 * (r.width + r.height)
}

fn fibonacci(n: i64) -> i64 {
    if n <= 1 {
        return n
    }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

fn factorial(n: i64) -> i64 {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        return a
    }
    return gcd(b, a % b)
}

fn is_even(n: i64) -> bool {
    return n % 2 == 0
}

fn abs_val(x: f64) -> f64 {
    if x < 0.0 {
        return 0.0 - x
    }
    return x
}

fn clamp(value: f64, lo: f64, hi: f64) -> f64 {
    if value < lo {
        return lo
    }
    if value > hi {
        return hi
    }
    return value
}

fn apply_twice(f: fn(i64) -> i64, x: i64) -> i64 {
    return f(f(x))
}

fn double(x: i64) -> i64 {
    return x * 2
}

fn triple(x: i64) -> i64 {
    return x * 3
}

fn sum_range(start: i64, end: i64) -> i64 {
    let mut total: i64 = 0
    let mut i = start
    while i < end {
        total = total + i
        i = i + 1
    }
    return total
}

fn count_down(n: i64) -> i64 {
    let mut count: i64 = 0
    let mut i = n
    while i > 0 {
        count = count + 1
        i = i - 1
    }
    return count
}

fn collatz_steps(n: i64) -> i64 {
    let mut steps: i64 = 0
    let mut val = n
    while val != 1 {
        if is_even(val) {
            val = val / 2
        } else {
            val = val * 3 + 1
        }
        steps = steps + 1
    }
    return steps
}

fn describe_number(x: i64) -> str {
    match x {
        0 => "zero",
        1 => "one",
        2 => "two",
        _ => "many",
    }
}

fn safe_divide(a: i64, b: i64) -> i64 {
    try {
        if b == 0 {
            throw "division by zero"
        }
        return a / b
    } catch e {
        return 0
    }
}

fn compute_points(n: i64) -> f64 {
    let mut total: f64 = 0.0
    let mut i: i64 = 0
    while i < n {
        let p1 = Point { x: 1.0, y: 2.0 }
        let p2 = Point { x: 4.0, y: 6.0 }
        total = total + distance_sq(p1, p2)
        i = i + 1
    }
    return total
}

fn main() {
    // Struct operations
    let origin = Point { x: 0.0, y: 0.0 }
    let target = Point { x: 3.0, y: 4.0 }
    let d = distance_sq(origin, target)
    let mid = midpoint(origin, target)

    let r = Rect {
        origin: origin,
        width: 10.0,
        height: 5.0,
    }
    let area = rect_area(r)
    let perim = rect_perimeter(r)

    // Recursion
    let fib10 = fibonacci(10)
    let fact5 = factorial(5)
    let g = gcd(48, 18)

    // Higher-order functions
    let doubled = apply_twice(double, 3)
    let tripled = apply_twice(triple, 2)

    // Loops
    let s = sum_range(0, 100)
    let c = count_down(50)
    let steps = collatz_steps(27)

    // Match
    let desc = describe_number(2)

    // Error handling
    let safe = safe_divide(10, 3)
    let safe_zero = safe_divide(10, 0)

    // Float math
    let clamped = clamp(15.0, 0.0, 10.0)
    let a = abs_val(0.0 - 5.5)

    // Compute-intensive
    let total = compute_points(100)

    println("done")
}
"#;

/// Generate a large source file by repeating parameterized function
/// definitions. Each generated function has unique names to avoid
/// duplicate-definition errors while still exercising the full pipeline.
fn generate_large_program(num_functions: usize) -> String {
    let mut source = String::with_capacity(num_functions * 300);

    // Struct definitions used by the functions.
    source.push_str(
        "struct Vec2 {\n\
         \x20   x: f64\n\
         \x20   y: f64\n\
         }\n\n",
    );

    // Generate many unique functions with varied bodies.
    for i in 0..num_functions {
        // Alternate between different function patterns.
        match i % 5 {
            0 => {
                // Arithmetic function
                source.push_str(&format!(
                    "fn compute_{i}(a: i64, b: i64) -> i64 {{\n\
                     \x20   let x = a + b * {i}\n\
                     \x20   let y = x - a\n\
                     \x20   if y > 0 {{\n\
                     \x20       return y\n\
                     \x20   }}\n\
                     \x20   return x\n\
                     }}\n\n"
                ));
            }
            1 => {
                // Loop function
                source.push_str(&format!(
                    "fn loop_{i}(n: i64) -> i64 {{\n\
                     \x20   let mut total: i64 = 0\n\
                     \x20   let mut j: i64 = 0\n\
                     \x20   while j < n {{\n\
                     \x20       total = total + j + {i}\n\
                     \x20       j = j + 1\n\
                     \x20   }}\n\
                     \x20   return total\n\
                     }}\n\n"
                ));
            }
            2 => {
                // Float math function
                source.push_str(&format!(
                    "fn float_{i}(x: f64) -> f64 {{\n\
                     \x20   let a = x + {i}.0\n\
                     \x20   let b = a * 2.0\n\
                     \x20   let c = b / 3.0\n\
                     \x20   return c - 1.0\n\
                     }}\n\n"
                ));
            }
            3 => {
                // Struct usage function
                source.push_str(&format!(
                    "fn vec_op_{i}(v: Vec2) -> f64 {{\n\
                     \x20   let scaled = Vec2 {{ x: v.x * {i}.0, y: v.y * {i}.0 }}\n\
                     \x20   return scaled.x + scaled.y\n\
                     }}\n\n"
                ));
            }
            4 => {
                // Recursive function
                source.push_str(&format!(
                    "fn recur_{i}(n: i64) -> i64 {{\n\
                     \x20   if n <= 0 {{\n\
                     \x20       return {i}\n\
                     \x20   }}\n\
                     \x20   return n + recur_{i}(n - 1)\n\
                     }}\n\n"
                ));
            }
            _ => unreachable!(),
        }
    }

    // Main that calls a subset of the generated functions.
    source.push_str("fn main() {\n");
    for i in 0..num_functions.min(20) {
        match i % 5 {
            0 => source.push_str(&format!("    let r{i} = compute_{i}(1, 2)\n")),
            1 => source.push_str(&format!("    let r{i} = loop_{i}(10)\n")),
            2 => source.push_str(&format!("    let r{i} = float_{i}(1.0)\n")),
            3 => {
                source.push_str(&format!(
                    "    let r{i} = vec_op_{i}(Vec2 {{ x: 1.0, y: 2.0 }})\n"
                ));
            }
            4 => source.push_str(&format!("    let r{i} = recur_{i}(5)\n")),
            _ => unreachable!(),
        }
    }
    source.push_str("    println(\"done\")\n");
    source.push_str("}\n");

    source
}

// ---------------------------------------------------------------------------
// Helpers — run pipeline stages up to a given point
// ---------------------------------------------------------------------------

/// Lex source and return tokens.
fn lex(source: &str) -> Vec<kryos_lexer::Token> {
    let mut sm = SourceMap::default();
    let file_id = sm.add_file("bench.kry".into(), source.into());
    Lexer::new(source, file_id).tokenize()
}

/// Parse source from a token stream and return the AST.
fn parse_source(source: &str) -> kryos_ast::Module {
    let tokens = lex(source);
    parse(tokens).expect("benchmark program must parse without errors")
}

// ---------------------------------------------------------------------------
// Lexer benchmarks
// ---------------------------------------------------------------------------

fn bench_lex(c: &mut Criterion) {
    let large_source = generate_large_program(200);

    let mut group = c.benchmark_group("lex");

    group.throughput(Throughput::Bytes(SMALL_PROGRAM.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("throughput", "small"),
        &SMALL_PROGRAM,
        |b, src| {
            b.iter(|| {
                let tokens = lex(black_box(src));
                black_box(tokens);
            });
        },
    );

    group.throughput(Throughput::Bytes(MEDIUM_PROGRAM.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("throughput", "medium"),
        &MEDIUM_PROGRAM,
        |b, src| {
            b.iter(|| {
                let tokens = lex(black_box(src));
                black_box(tokens);
            });
        },
    );

    group.throughput(Throughput::Bytes(large_source.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("throughput", "large_200fn"),
        &large_source.as_str(),
        |b, src| {
            b.iter(|| {
                let tokens = lex(black_box(src));
                black_box(tokens);
            });
        },
    );

    group.finish();
}

// ---------------------------------------------------------------------------
// Parser benchmarks
// ---------------------------------------------------------------------------

fn bench_parse(c: &mut Criterion) {
    let large_source = generate_large_program(200);

    let mut group = c.benchmark_group("parse");

    group.throughput(Throughput::Bytes(SMALL_PROGRAM.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("throughput", "small"),
        &SMALL_PROGRAM,
        |b, src| {
            b.iter(|| {
                let module = parse_source(black_box(src));
                black_box(module);
            });
        },
    );

    group.throughput(Throughput::Bytes(MEDIUM_PROGRAM.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("throughput", "medium"),
        &MEDIUM_PROGRAM,
        |b, src| {
            b.iter(|| {
                let module = parse_source(black_box(src));
                black_box(module);
            });
        },
    );

    group.throughput(Throughput::Bytes(large_source.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("throughput", "large_200fn"),
        &large_source.as_str(),
        |b, src| {
            b.iter(|| {
                let module = parse_source(black_box(src));
                black_box(module);
            });
        },
    );

    group.finish();
}

// ---------------------------------------------------------------------------
// Type checking benchmarks
// ---------------------------------------------------------------------------

fn bench_typecheck(c: &mut Criterion) {
    let medium_module = parse_source(MEDIUM_PROGRAM);
    let large_source = generate_large_program(200);
    let large_module = parse_source(&large_source);

    let mut group = c.benchmark_group("typecheck");

    group.bench_function("medium", |b| {
        b.iter(|| {
            let diags = type_check(black_box(&medium_module));
            black_box(diags);
        });
    });

    group.bench_function("large_200fn", |b| {
        b.iter(|| {
            let diags = type_check(black_box(&large_module));
            black_box(diags);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Ownership analysis benchmarks
// ---------------------------------------------------------------------------

fn bench_ownership(c: &mut Criterion) {
    let medium_module = parse_source(MEDIUM_PROGRAM);
    let large_source = generate_large_program(200);
    let large_module = parse_source(&large_source);

    let mut group = c.benchmark_group("ownership");

    group.bench_function("medium", |b| {
        b.iter(|| {
            let result = analyze_ownership(black_box(&medium_module));
            black_box(result);
        });
    });

    group.bench_function("large_200fn", |b| {
        b.iter(|| {
            let result = analyze_ownership(black_box(&large_module));
            black_box(result);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Capability checking benchmarks
// ---------------------------------------------------------------------------

fn bench_capabilities(c: &mut Criterion) {
    let medium_module = parse_source(MEDIUM_PROGRAM);
    let large_source = generate_large_program(200);
    let large_module = parse_source(&large_source);

    let mut group = c.benchmark_group("capabilities");

    group.bench_function("medium", |b| {
        b.iter(|| {
            let diags = check_capabilities(black_box(&medium_module));
            black_box(diags);
        });
    });

    group.bench_function("large_200fn", |b| {
        b.iter(|| {
            let diags = check_capabilities(black_box(&large_module));
            black_box(diags);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// MIR lowering benchmarks
// ---------------------------------------------------------------------------

fn bench_mir(c: &mut Criterion) {
    let medium_module = parse_source(MEDIUM_PROGRAM);
    let large_source = generate_large_program(200);
    let large_module = parse_source(&large_source);

    let mut group = c.benchmark_group("mir");

    group.bench_function("lower_medium", |b| {
        b.iter(|| {
            let mir = lower_module(black_box(&medium_module));
            black_box(mir);
        });
    });

    group.bench_function("lower_large_200fn", |b| {
        b.iter(|| {
            let mir = lower_module(black_box(&large_module));
            black_box(mir);
        });
    });

    // MIR lowering + comptime evaluation combined
    group.bench_function("lower_and_consteval_medium", |b| {
        b.iter(|| {
            let mut mir = lower_module(black_box(&medium_module));
            kryos_mir::consteval::run_comptime_pass(&mut mir);
            black_box(mir);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Codegen benchmarks (Cranelift)
// ---------------------------------------------------------------------------

fn bench_codegen_cranelift(c: &mut Criterion) {
    let medium_module = parse_source(MEDIUM_PROGRAM);
    let medium_mir = lower_module(&medium_module);

    let large_source = generate_large_program(200);
    let large_module = parse_source(&large_source);
    let large_mir = lower_module(&large_module);

    let backend = CraneliftBackend::new();

    let mut group = c.benchmark_group("codegen");

    group.bench_function("cranelift_medium", |b| {
        b.iter(|| {
            let result = backend.compile_module(black_box(&medium_mir));
            let _ = black_box(result);
        });
    });

    group.bench_function("cranelift_large_200fn", |b| {
        b.iter(|| {
            let result = backend.compile_module(black_box(&large_mir));
            let _ = black_box(result);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Full pipeline benchmarks
// ---------------------------------------------------------------------------

fn bench_pipeline(c: &mut Criterion) {
    let large_source = generate_large_program(200);

    let check_config = BuildConfig {
        input: "bench.kry".into(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
        skip_ownership: false,
    };

    let mut group = c.benchmark_group("pipeline");

    // check_source — analysis only (lex + parse + typecheck + ownership + capabilities)
    group.throughput(Throughput::Bytes(MEDIUM_PROGRAM.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("check_source", "medium"),
        &MEDIUM_PROGRAM,
        |b, src| {
            b.iter(|| {
                let (diags, sm) = check_source(black_box(src), "bench.kry");
                black_box((diags, sm));
            });
        },
    );

    group.throughput(Throughput::Bytes(large_source.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("check_source", "large_200fn"),
        &large_source.as_str(),
        |b, src| {
            b.iter(|| {
                let (diags, sm) = check_source(black_box(src), "bench.kry");
                black_box((diags, sm));
            });
        },
    );

    // compile_source — full pipeline through MIR (no backend link step)
    group.throughput(Throughput::Bytes(MEDIUM_PROGRAM.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("compile_source_to_mir", "medium"),
        &MEDIUM_PROGRAM,
        |b, src| {
            b.iter(|| {
                let result = compile_source(black_box(src), "bench.kry", &check_config);
                black_box(result);
            });
        },
    );

    group.throughput(Throughput::Bytes(large_source.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("compile_source_to_mir", "large_200fn"),
        &large_source.as_str(),
        |b, src| {
            b.iter(|| {
                let result = compile_source(black_box(src), "bench.kry", &check_config);
                black_box(result);
            });
        },
    );

    group.finish();
}

// ---------------------------------------------------------------------------
// JIT fibonacci runtime benchmark
// ---------------------------------------------------------------------------

/// Fibonacci-only source — no runtime calls (println, to_string, etc.) so the
/// JIT can compile and execute it without needing the full kryos-rt symbol table
/// beyond what `jit_compile_module` already registers.
const FIB_PROGRAM: &str = r#"
fn fibonacci(n: i64) -> i64 {
    if n <= 1 { return n }
    return fibonacci(n - 1) + fibonacci(n - 2)
}
"#;

fn bench_jit_fibonacci(c: &mut Criterion) {
    // Pre-compile the fibonacci function to MIR once.
    let fib_module = parse_source(FIB_PROGRAM);
    let fib_mir = lower_module(&fib_module);

    let mut group = c.benchmark_group("jit");

    // Benchmark 1: JIT compilation time (compile fibonacci to native code)
    group.bench_function("compile_fibonacci", |b| {
        b.iter(|| {
            let ptrs = jit_compile_module(black_box(&fib_mir))
                .expect("JIT compilation must succeed");
            black_box(ptrs);
        });
    });

    // Benchmark 2: JIT execution of fib(30) — measures actual native runtime
    // We compile once, then benchmark the execution.
    let ptrs = jit_compile_module(&fib_mir).expect("JIT compilation must succeed");
    let fib_ptr = ptrs
        .get("fibonacci")
        .expect("fibonacci function must be in JIT output");
    let fib_fn: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(*fib_ptr) };

    // Sanity check: fib(10) == 55
    assert_eq!(fib_fn(10), 55, "JIT fibonacci sanity check failed");

    group.bench_function("execute_fib_30", |b| {
        b.iter(|| {
            let result = fib_fn(black_box(30));
            black_box(result);
        });
    });

    // Benchmark 3: Full round-trip — parse + typecheck + MIR lower + JIT compile + execute fib(30)
    group.bench_function("full_roundtrip_fib_30", |b| {
        b.iter(|| {
            let module = parse_source(black_box(FIB_PROGRAM));
            let mir = lower_module(&module);
            let ptrs = jit_compile_module(&mir).expect("JIT compilation must succeed");
            let fib_ptr = ptrs.get("fibonacci").unwrap();
            let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(*fib_ptr) };
            let result = f(30);
            black_box(result);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_lex,
    bench_parse,
    bench_typecheck,
    bench_ownership,
    bench_capabilities,
    bench_mir,
    bench_codegen_cranelift,
    bench_pipeline,
    bench_jit_fibonacci,
);
criterion_main!(benches);
