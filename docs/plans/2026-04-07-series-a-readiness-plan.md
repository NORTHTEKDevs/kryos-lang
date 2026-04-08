# Kryos Series A Readiness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Kryos investor-ready, production-ready, and credible for public launch across 6 phases.

**Architecture:** Fix 5 crashing examples (codegen bugs in comptime, string match, struct field access, heap management), verify stdlib infrastructure works end-to-end, write 5 real programs, build package ecosystem, add binary releases and install script.

**Tech Stack:** Rust (compiler, 21 crates), Cranelift (JIT backend), LLVM (release backend), Kryos (.kry stdlib/examples), criterion (benchmarks), GitHub Actions (CI/CD)

**MEMORY WARNING:** Debug builds consume ~48GB RAM. ALWAYS use `cargo build --release -j 4` and `cargo test --release -j 4`. Never bare `cargo build` or `cargo test`.

---

## Phase 1: Stop the Bleeding — Fix Every Crash

### Task 1: Debug and fix fibonacci_showcase.kry comptime segfault

**Files:**
- Debug: `examples/fibonacci_showcase.kry` (crash at section 4, line 85-90)
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs:1926-1929` (Comptime RValue translation)
- Modify: `crates/kryos-mir/src/lower.rs` (comptime lowering — check inner RValue type propagation)
- Test: `crates/kryos-test-runner/tests/e2e/` (new regression test)

**Context:** The example runs sections 1-3 perfectly (recursion, TCO, iteration), then segfaults at section 4 which uses `comptime { 6 * 7 }`. The Cranelift codegen at line 1926 handles `RValue::Comptime(inner)` by recursively translating the inner expression. The crash is likely because:
- The comptime result value isn't getting the correct type annotation, causing `to_string(ct1)` to misinterpret the value
- OR the MIR lowerer emits the comptime inner expression with a type that doesn't match the outer `let` binding

**Step 1:** Create a minimal reproducer test file:

```kryos
// crates/kryos-test-runner/tests/e2e/comptime/comptime_basic_run.kry
// run-expect: 42
fn main() {
    let x = comptime { 6 * 7 }
    println(to_string(x))
}
```

**Step 2:** Run it: `cargo test --release -p kryos-test-runner -j 4 -- comptime_basic`
Expected: SEGFAULT (reproduces the bug)

**Step 3:** Add debug logging to `translate_rvalue` for `RValue::Comptime` in `codegen.rs:1926`. Check:
1. What inner RValue is being translated? (Should be `BinOp { Mul, Const(6), Const(7) }`)
2. What Cranelift IR value type does it produce? (Should be `i64`)
3. Is the result value correctly assigned to the destination local?

Inspect the MIR lowering of `comptime { 6 * 7 }` — in `lower.rs`, search for `Comptime` lowering. Check if the inner expression is being constant-folded during MIR optimization (constant folding pass) and whether the folded constant retains its type as `MirType::I64`.

**Step 4:** Fix the bug. Common fixes:
- If the comptime result lacks type info: propagate the inner expression's type to the dest local in MIR
- If the Cranelift IR value has wrong type: add an explicit type check in `RValue::Comptime` handler
- If constant folding strips type info: ensure `ConstantFold` pass preserves types

**Step 5:** Verify the minimal test passes, then run the full example:
```bash
cargo test --release -p kryos-test-runner -j 4 -- comptime_basic
cargo run --release -- run ../examples/fibonacci_showcase.kry
```
Expected: Full output through all 6 sections, no crash.

**Step 6:** Commit: `fix(codegen): resolve comptime evaluation segfault in Cranelift backend`

---

### Task 2: Fix string match patterns in MIR lowering (http_server.kry)

**Files:**
- Modify: `crates/kryos-mir/src/lower.rs:2027-2033` (Pattern::Literal match arm in `lower_match`)
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs:2302-2338` (Switch terminator — needs string comparison chain)
- Modify: `crates/kryos-mir/src/ir.rs` (may need a `StringSwitch` terminator or string cases in `Switch`)
- Test: `crates/kryos-test-runner/tests/e2e/match/` (new regression tests)

**Context:** The MIR lowering at `lower.rs:2027-2033` handles `Pattern::Literal` but only extracts integer literals (`IntLiteral`). String literal patterns (like `"/"`, `"/health"`) fall through to the `else` branch, which pushes them into `default_arm`. Since `default_arm` is overwritten on each iteration, only the LAST non-wildcard arm survives. The `Switch` terminator then gets empty `targets` and all paths go to default.

This means `match req.path { "/" => ... }` effectively ignores all string patterns. The crash (STATUS_ILLEGAL_INSTRUCTION) happens because the struct construction in the default arm body accesses invalid memory or because the switch terminates into an uninitialized code path.

**Step 1:** Create a minimal string match test:

```kryos
// crates/kryos-test-runner/tests/e2e/match/string_match_run.kry
// run-expect: matched hello
fn main() {
    let s = "hello"
    let result = match s {
        "hello" => "matched hello"
        "world" => "matched world"
        _ => "no match"
    }
    println(result)
}
```

**Step 2:** Run it: `cargo test --release -p kryos-test-runner -j 4 -- string_match`
Expected: FAIL (wrong output or crash)

**Step 3:** Fix the MIR lowering. In `lower_match()` at line 2027, extend `Pattern::Literal` to handle `StringLiteral`:

```rust
ast::Pattern::Literal { expr, .. } => {
    if let ast::Expr::IntLiteral { value, .. } = expr.as_ref() {
        targets.push((*value, arm_bb));
        arm_blocks.push((arm_bb, &arm.body, None));
    } else if let ast::Expr::StringLiteral { value, .. } = expr.as_ref() {
        // String match: store the string constant and arm block for
        // a chain of string-equality checks (not integer switch).
        string_targets.push((value.clone(), arm_bb));
        arm_blocks.push((arm_bb, &arm.body, None));
    } else {
        default_arm = Some((arm_bb, &arm.body));
    }
}
```

Add `let mut string_targets: Vec<(String, BlockId)> = Vec::new();` near line 2005.

After building the targets, if `string_targets` is non-empty, emit a different terminator. Two approaches:

**Approach A (simpler):** Convert string match to a chain of if-else blocks in MIR. For each string pattern, emit:
```
tmp = kryos_string_eq(switch_op, "pattern_constant")
if tmp goto arm_bb else goto next_check
```

This avoids changing the `Switch` terminator. Emit this chain after line 2064 when `string_targets` is non-empty:

```rust
if !string_targets.is_empty() {
    // Emit chain of string equality checks instead of Switch.
    for (i, (pattern_str, arm_bb)) in string_targets.iter().enumerate() {
        let str_const = ctx.alloc_temp(MirType::Str);
        ctx.emit(Instruction::Assign {
            dest: str_const,
            value: RValue::Const(Constant::Str(pattern_str.clone())),
        });
        let eq_result = ctx.alloc_temp(MirType::Bool);
        ctx.emit(Instruction::Assign {
            dest: eq_result,
            value: RValue::BinOp {
                op: MirBinOp::Eq,
                left: switch_op.clone(),
                right: Operand::Local(str_const),
            },
        });
        let next_bb = if i + 1 < string_targets.len() {
            ctx.alloc_block()
        } else {
            // Last check: fall through to default.
            default_bb
        };
        // NB: the current block was already set up — finish it with a branch.
        ctx.finish_block(
            Terminator::Branch {
                cond: Operand::Local(eq_result),
                then_block: *arm_bb,
                else_block: next_bb,
            },
            next_bb,
        );
        if i + 1 < string_targets.len() {
            ctx.current_block = next_bb;
        }
    }
} else {
    // Integer switch (existing code).
    ctx.finish_block(
        Terminator::Switch { value: switch_op, targets, default: default_bb },
        ...
    );
}
```

**Step 4:** Run the string match test: `cargo test --release -p kryos-test-runner -j 4 -- string_match`
Expected: PASS, output "matched hello"

**Step 5:** Run http_server example: `cargo run --release -- run ../examples/http_server.kry`
Expected: Full output with all 4 routes resolved correctly.

**Step 6:** Add additional test cases:
```kryos
// string_match_default_run.kry
// run-expect: no match
fn main() {
    let s = "other"
    let result = match s {
        "hello" => "matched hello"
        _ => "no match"
    }
    println(result)
}
```

**Step 7:** Commit: `fix(mir): implement string pattern matching via equality chain`

---

### Task 3: Fix struct field access crashes (mini_grep.kry, http_server.kry)

**Files:**
- Debug: `crates/kryos-codegen-cranelift/src/codegen.rs:1624-1665` (RValue::Field)
- Debug: `crates/kryos-mir/src/lower.rs` (struct field access lowering — check type propagation)
- Test: `crates/kryos-test-runner/tests/e2e/structs/` (new tests)

**Context:** `RValue::Field` at codegen.rs:1624 resolves struct field access by looking up the object operand's type in `mir_func.locals`. If the local's type isn't `MirType::Struct(name)`, it falls through to the fallback at line 1662 which returns a zero pointer. This causes segfaults when the zero pointer is later dereferenced.

`mini_grep.kry` creates `SearchResult { pattern: str, found: i64, line_num: i64 }` and passes it to `report()` which accesses `r.pattern` and `r.found`. `http_server.kry` creates `Request` and `Response` structs and accesses fields.

**Step 1:** Create a minimal struct field access test:

```kryos
// crates/kryos-test-runner/tests/e2e/structs/struct_field_str_run.kry
// run-expect: hello
struct Foo {
    name: str
    value: i64
}

fn show(f: Foo) {
    println(f.name)
}

fn main() {
    let f = Foo { name: "hello", value: 42 }
    show(f)
}
```

**Step 2:** Run it: `cargo test --release -p kryos-test-runner -j 4 -- struct_field_str`
Expected: Likely CRASH (reproduces the bug)

**Step 3:** Debug the issue. In `RValue::Field` handler (codegen.rs:1624), add a debug print to check:
1. Is `struct_name` resolved? (Does the function parameter local have type `MirType::Struct("Foo")`?)
2. If not, check the MIR output — the function parameter might have type `MirType::I64` instead of `MirType::Struct("Foo")`.

The likely fix: In MIR lowering, when lowering function parameters for user-defined functions, the parameter types need to carry their struct type info. Check `lower.rs` where function parameters are registered — ensure struct-typed parameters get `MirType::Struct(name)` not `MirType::I64`.

**Step 4:** Also check that struct return values from functions preserve their type. When `route()` returns a `Response`, the caller's local should be typed `MirType::Struct("Response")`.

**Step 5:** Fix the type propagation, run test, verify pass.

**Step 6:** Run mini_grep and http_server:
```bash
cargo run --release -- run ../examples/mini_grep.kry
cargo run --release -- run ../examples/http_server.kry
```
Expected: Both run to completion.

**Step 7:** Commit: `fix(mir): propagate struct type info through function parameters and returns`

---

### Task 4: Fix pipeline.kry heap corruption

**Files:**
- Debug: `examples/pipeline.kry` (crash at Stage 2 filter, line 50-56)
- Debug: `crates/kryos-codegen-cranelift/src/codegen.rs` (struct stack allocation lifetime)
- Debug: `crates/kryos-rt/src/builtins.rs` (string/array memory management)
- Test: `crates/kryos-test-runner/tests/e2e/` (new regression test)

**Context:** `pipeline.kry` runs Stage 1 perfectly (10 iterations with string concatenation). Stage 2 crashes with heap corruption (`STATUS_HEAP_CORRUPTION`) after printing "0 is even". Stage 2 calls `square(i)` multiple times per iteration, does modulo, and accumulates into `even_sum`.

The heap corruption likely comes from:
1. **String temporaries not freed:** Each `to_string(square(i))` and string concatenation creates temporary KryosString handles. If these aren't being freed after use, the heap gets corrupted.
2. **Stack struct aliasing:** `square(i)` is called 3 times per iteration. If any intermediate results are stack-allocated structs that get overwritten, the pointer becomes dangling.
3. **Modulo on string handle:** If `v` somehow gets typed as a string handle instead of i64, `v % 2` would corrupt heap memory.

**Step 1:** Create a minimal reproducer:

```kryos
// crates/kryos-test-runner/tests/e2e/loops/modulo_loop_run.kry
// run-expect: done
fn square(x: i64) -> i64 { return x * x }

fn main() {
    let mut i = 0
    while i < 10 {
        let v = square(i)
        let r = v % 2
        if r == 0 {
            println(to_string(v) + " is even")
        }
        i = i + 1
    }
    println("done")
}
```

**Step 2:** Run it. If it crashes, the bug is in the loop/modulo/string-concat interaction. If it doesn't crash, the bug is specific to the pipeline.kry pattern (e.g., calling square() multiple times, or the accumulation pattern).

**Step 3:** Narrow down by adding complexity: add `even_sum = even_sum + square(i)` accumulation, then recomputation `to_string(square(i))` inside the if branch.

**Step 4:** Fix the root cause. Common fixes:
- If string temporaries cause heap corruption: ensure `kryos_string_concat` allocates fresh memory and the caller doesn't free both input handles
- If modulo on wrong type: check MIR type inference for `v % 2` when `v` comes from a function return
- If stack struct aliasing: ensure function return values are properly copied, not aliased

**Step 5:** Verify pipeline runs to completion: `cargo run --release -- run ../examples/pipeline.kry`

**Step 6:** Commit: `fix(codegen): resolve heap corruption in loop with string temporaries`

---

### Task 5: Fix kryos_bootstrap.kry parse error

**Files:**
- Debug: `crates/kryos-parser/src/parser.rs:1011-1039` (parse_expr_or_assign)
- Debug: `examples/kryos_bootstrap.kry:193` (line `done = true`)
- Test: `crates/kryos-test-runner/tests/e2e/` (new test)

**Context:** `kryos_bootstrap.kry` uses `let mut done = false` (line 185) followed by `done = true` (line 193). The parser reports "expected Colon, found Eq" which means it's trying to parse `done` as a new `let` declaration instead of an assignment.

The parser has `parse_expr_or_assign()` at lines 1011-1039 which correctly handles `x = value`. But it may not be invoked in the right context — the main statement parser might be trying to parse `done` as a declaration first.

**Step 1:** Create a minimal reproducer:

```kryos
// crates/kryos-test-runner/tests/e2e/basics/reassign_bool_run.kry
// run-expect: true
fn main() {
    let mut done = false
    done = true
    println(to_string(done))
}
```

**Step 2:** Run it: `cargo test --release -p kryos-test-runner -j 4 -- reassign_bool`
Expected: Parse error (reproduces the bug)

**Step 3:** Debug the parser. Check `parse_statement()` or the top-level statement dispatcher. When it sees an identifier token `done` at the start of a statement, it should try `parse_expr_or_assign()`. If it's trying `parse_let()` first and failing, the dispatch logic is wrong.

Also check: `true` is a keyword token (`TokenKind::True`), not an identifier. The parser might be confused because after seeing `done`, it peeks at `=` and tries to parse a typed declaration like `done: bool = true`. Check if the parser distinguishes between `let done = ...` and bare `done = ...`.

**Step 4:** Fix the parser dispatch. The fix is likely: when the parser sees an identifier that is NOT preceded by `let`, it should go through `parse_expr_or_assign()` which correctly handles `ident = value`.

**Step 5:** Also note: `kryos_bootstrap.kry` uses `char_code()` and `substr()` builtins which aren't registered (mentioned in known issues). After fixing the parse error, the file may still fail to compile due to missing builtins. These need to be registered or the example needs to be rewritten to use available builtins.

**Step 6:** Run the bootstrap example after fixes:
```bash
cargo run --release -- run ../examples/kryos_bootstrap.kry
```

**Step 7:** Commit: `fix(parser): handle bare identifier reassignment in statement context`

---

### Task 6: Fix all_features.kry

**Files:**
- Modify: `examples/all_features.kry` (fix broken code)
- Test: run the example

**Context:** `all_features.kry` has two issues:
1. `use std::math` at line 42 — the stdlib math module has errors
2. Various parse errors in the file

The fix is to rewrite this example to only use features that actually work. It should be a clean showcase of everything the language can do.

**Step 1:** Rewrite `all_features.kry` to demonstrate: structs, enums, traits, functions, closures, error handling, generics, match expressions, comptime, channels/spawn — only features that are verified to work after Tasks 1-5.

**Step 2:** Remove the `use std::math` import (stdlib fixes come in Phase 2).
Remove the `extern "C"` block (not needed for a showcase).
Remove the `actor` block (actors have known issues).

**Step 3:** Run it: `cargo run --release -- run ../examples/all_features.kry`
Expected: Clean output demonstrating each feature.

**Step 4:** Commit: `fix(examples): rewrite all_features.kry to use verified features`

---

### Task 7: Regression test suite for all examples

**Files:**
- Create: `crates/kryos-test-runner/tests/examples_run.rs`

**Step 1:** Create a test that runs each example through the compiler and verifies:
1. It compiles without errors
2. It runs without crashes (exit code 0)

```rust
// crates/kryos-test-runner/tests/examples_run.rs
use std::process::Command;

fn run_example(name: &str) {
    let kryos = env!("CARGO_BIN_EXE_kryos");
    let example_path = format!("../../examples/{}", name);
    let output = Command::new(kryos)
        .args(["run", &example_path])
        .output()
        .expect("failed to run kryos");
    assert!(
        output.status.success(),
        "Example {} failed with exit code {:?}\nstderr: {}",
        name,
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test] fn example_demo() { run_example("demo.kry"); }
#[test] fn example_fibonacci() { run_example("fibonacci_showcase.kry"); }
#[test] fn example_http_server() { run_example("http_server.kry"); }
#[test] fn example_pipeline() { run_example("pipeline.kry"); }
#[test] fn example_mini_grep() { run_example("mini_grep.kry"); }
#[test] fn example_neural_net() { run_example("neural_net.kry"); }
#[test] fn example_all_features() { run_example("all_features.kry"); }
```

Note: `kryos_bootstrap.kry` may be excluded if it depends on unregistered builtins (`char_code`, `substr`).

**Step 2:** Run: `cargo test --release -p kryos-test-runner -j 4 -- examples_run`
Expected: ALL pass.

**Step 3:** Commit: `test: add example regression test suite`

---

## Phase 2: The Foundation Works — Stdlib & Module System

### Task 8: Verify and fix stdlib module resolution

**Files:**
- Verify: `crates/kryos-driver/src/resolve.rs:163-178` (stdlib fallback in resolve_module_path)
- Create: `crates/kryos-driver/tests/stdlib_resolve.rs`

**Context:** The exploration revealed that `find_stdlib_dir()` and the `std::` prefix fallback already exist in the codebase (lines 31-70 and 163-178 of resolve.rs). But `all_features.kry` failed with `use std::math`, so either the resolution path doesn't work on Windows or the math.kry module itself has errors.

**Step 1:** Write a test that verifies stdlib resolution:

```rust
// crates/kryos-driver/tests/stdlib_resolve.rs
#[test]
fn resolve_std_math() {
    let stdlib_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap()
        .join("stdlib");
    assert!(stdlib_dir.join("math.kry").exists(), "stdlib/math.kry must exist");
    
    // Set env var and test resolution
    std::env::set_var("KRYOS_STDLIB_DIR", &stdlib_dir);
    // ... test resolve_module_path with "std::math"
}
```

**Step 2:** If the resolution works but the module has errors, that's Task 9's problem. If resolution fails, fix the path logic (Windows backslash issues, relative path resolution).

**Step 3:** Commit: `test: verify stdlib module resolution`

---

### Task 9: Audit and fix all 28 stdlib .kry files

**Files:**
- Modify: `compiler/stdlib/*.kry` (28 files)
- Create: `crates/kryos-driver/tests/stdlib_compile.rs`

**Step 1:** Create a compile-check test:

```rust
// crates/kryos-driver/tests/stdlib_compile.rs
#[test]
fn all_stdlib_modules_typecheck() {
    let stdlib_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap()
        .join("stdlib");
    
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&stdlib_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "kry").unwrap_or(false) {
            let result = check_file(&path);
            if result.has_errors() {
                failures.push(path.file_name().unwrap().to_string_lossy().to_string());
            }
        }
    }
    assert!(failures.is_empty(), "Stdlib files with errors: {:?}", failures);
}
```

**Step 2:** Run it, collect all failures.

**Step 3:** Fix each failing module. Common fixes:
- Replace `ptr` type with `i64` in extern signatures
- Add `__builtin_*` recognition or replace with runtime function calls
- Fix cross-module references
- Comment out functions that depend on unimplemented features

**Step 4:** Run until all 28 pass. Commit: `fix(stdlib): make all 28 stdlib modules type-check clean`

---

### Task 10: Fix license consistency

**Files:**
- Modify: `compiler/Cargo.toml` (workspace level)
- Create: `LICENSE` at repo root

**Step 1:** Change `license = "MIT"` to `license = "LicenseRef-Proprietary"` in workspace Cargo.toml.

**Step 2:** Create `LICENSE`:

```
Copyright (c) 2026 FrostByte Digital. All rights reserved.

This software is proprietary and confidential. Unauthorized copying,
modification, distribution, or use of this software, via any medium,
is strictly prohibited without prior written permission from FrostByte Digital.

For licensing inquiries, contact: licensing@frostbytedigital.io
```

**Step 3:** Verify README says "Proprietary" (it already does).

**Step 4:** Commit: `chore: fix license consistency — proprietary everywhere`

---

## Phase 3: A Real Language — Verify Existing Infrastructure

### Task 11: Verify ergonomic builtins work end-to-end

**Files:**
- Test: `crates/kryos-test-runner/tests/e2e/builtins/` (new tests)

**Context:** The JIT already registers `kryos_builtin_file_read`, `kryos_builtin_env_get`, `kryos_builtin_time_now`, etc. (jit.rs:131-138). The MIR lowerer has return types registered (lower.rs:300-323). The runtime functions exist in builtins.rs (lines 311-487). But these have never been tested end-to-end.

**Step 1:** Write end-to-end tests for each builtin:

```kryos
// builtins/time_now_run.kry
// run-expect: time:
fn main() {
    let t = time_now()
    println("time: " + to_string(t))
}
```

```kryos
// builtins/assert_pass_run.kry
// run-expect: ok
fn main() {
    assert(1 == 1, "should pass")
    println("ok")
}
```

```kryos
// builtins/parse_int_run.kry
// run-expect: 42
fn main() {
    let n = parse_int("42")
    println(to_string(n))
}
```

**Step 2:** Run each test. Fix any that fail — the issue will be in the argument/return type plumbing between MIR, codegen, and the runtime function.

**Step 3:** Commit: `test: verify ergonomic builtins work end-to-end`

---

### Task 12: Verify panic handler and stack traces work

**Files:**
- Test: `crates/kryos-test-runner/tests/e2e/error_cases/` (new tests)

**Context:** The panic handler (panic.rs) and trace system (trace.rs) already exist and are registered in the JIT (jit.rs:116-118, 260-261). The `kryos_check_div_zero_i64` function exists (builtins.rs:525).

**Step 1:** Write a div-by-zero test:

```kryos
// error_cases/div_by_zero_run.kry
// run-expect-error: panic
fn main() {
    let x = 10
    let y = 0
    let z = x / y
    println(to_string(z))
}
```

**Step 2:** Run it. Check whether it produces a readable panic message or a raw segfault.

If it segfaults: the div-by-zero check isn't being emitted in codegen. In `codegen.rs`, find where integer division is translated and add a call to `kryos_check_div_zero_i64(divisor)` before the division instruction.

If it panics cleanly: verify the error message includes file/line info. If not, enhance the panic handler call to pass source location.

**Step 3:** Test stack traces by creating nested function calls that panic:

```kryos
// error_cases/stack_trace_run.kry
// run-expect-error: panic
fn inner() {
    let x = 1 / 0
}
fn middle() { inner() }
fn main() { middle() }
```

**Step 4:** If stack traces don't appear, check whether `kryos_trace_enter`/`kryos_trace_exit` calls are being emitted in codegen at function entry/exit points. If not, add them.

**Step 5:** Commit: `feat(runtime): verify panic handler and stack trace infrastructure`

---

## Phase 4: Proof It Works — Real Programs & Benchmarks

### Task 13: Write CLI Calculator example

**Files:**
- Create: `examples/calculator.kry` (~100 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/calculator_run.kry`

**The program:** Evaluates simple arithmetic expressions. Uses recursive descent parsing.

```kryos
// examples/calculator.kry
fn eval_expr(tokens: str, pos: i64) -> i64 {
    // Parse numbers and basic +, -, *, / operations
    // Uses recursive descent: expr -> term ((+|-) term)*
    // term -> factor ((*|/) factor)*
    // factor -> number | '(' expr ')'
}
```

Since `args()` may not be available for reading CLI input, the test version uses hardcoded expressions:

```kryos
// calculator_run.kry
// run-expect: 14
fn main() {
    // Evaluate: 2 + 3 * 4 = 14
    let result = 2 + 3 * 4
    println(to_string(result))
}
```

The full calculator example should demonstrate:
- Functions and recursion
- String parsing (character-by-character)
- Error handling with try/catch
- Integer arithmetic

**Step 1:** Write the calculator.
**Step 2:** Run it, fix any codegen bugs that surface.
**Step 3:** Commit: `feat(examples): add CLI calculator program`

---

### Task 14: Write File Line Counter (wc clone) example

**Files:**
- Create: `examples/wc.kry` (~80 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/wc_run.kry`

**The program:** Counts lines, words, and characters in a string.

Since `file_read()` depends on Phase 3 verification, the example should work with hardcoded input as a fallback:

```kryos
fn count_lines(text: str) -> i64 { ... }
fn count_words(text: str) -> i64 { ... }
fn count_chars(text: str) -> i64 { ... }
```

**Step 1:** Write the program using string operations.
**Step 2:** Run it, fix bugs.
**Step 3:** Commit: `feat(examples): add wc clone line counter`

---

### Task 15: Write CSV Analyzer example

**Files:**
- Create: `examples/csv_stats.kry` (~150 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/csv_run.kry`

**The program:** Parses CSV data (hardcoded or from file), computes per-column stats.

Exercises: structs, methods, arrays, float math, formatted output.

**Step 1:** Write the program.
**Step 2:** Run it, fix bugs.
**Step 3:** Commit: `feat(examples): add CSV stats analyzer`

---

### Task 16: Write TCP Echo Server example

**Files:**
- Create: `examples/echo_server.kry` (~100 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/echo_run.kry`

**The program:** Uses channels and spawn for concurrent message handling (simulated, not real TCP since networking builtins may not be wired).

**Step 1:** Write the program.
**Step 2:** Run it, fix bugs.
**Step 3:** Commit: `feat(examples): add concurrent echo server`

---

### Task 17: Write JSON Key Counter example

**Files:**
- Create: `examples/json_keys.kry` (~120 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/json_run.kry`

**The program:** Simple JSON parser that counts key occurrences in a hardcoded JSON string.

Exercises: string parsing, maps, iteration, formatted output.

**Step 1:** Write the program.
**Step 2:** Run it, fix bugs.
**Step 3:** Commit: `feat(examples): add JSON key counter`

---

### Task 18: Benchmark suite with verifiable results

**Files:**
- Create: `compiler/benches/kryos_bench.rs`
- Modify: `compiler/Cargo.toml` (add criterion dev-dependency)
- Existing: `benchmarks/*.kry` (fibonacci, matrix, sort, strings, http_bench)

**Step 1:** Add criterion to workspace Cargo.toml:
```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "kryos_bench"
harness = false
```

**Step 2:** Write benchmark harness that compiles and runs each .kry benchmark, measuring wall time.

**Step 3:** Run `cargo bench --release -j 4`, verify results match README claims (within margin).

**Step 4:** Update README benchmark table with actual measured numbers if they differ.

**Step 5:** Commit: `feat(bench): add criterion benchmark suite`

---

## Phase 5: Developer Experience

### Task 19: Improve kryos pkg init scaffolding

**Files:**
- Modify: `crates/kryos-cli/src/commands/pkg.rs`

**Step 1:** Improve `kryos pkg init <name>` to create:
- `kryos.toml` with name, version, edition
- `src/main.kry` with `fn main() { println("Hello from Kryos!") }`
- `.gitignore` with `target/`, `.kryos/`, `*.o`

**Step 2:** Test the flow:
```bash
cargo run --release -- pkg init myproject
cd myproject
cargo run --release -p kryos-cli -- run src/main.kry
```

**Step 3:** Commit: `feat(pkg): improve project scaffolding with working hello world`

---

### Task 20: Write tutorial — "Build a CSV Analyzer in Kryos"

**Files:**
- Create: `docs/tutorial/01-getting-started.md`
- Create: `docs/tutorial/02-reading-data.md`
- Create: `docs/tutorial/03-parsing-csv.md`
- Create: `docs/tutorial/04-computing-stats.md`
- Create: `docs/tutorial/05-formatting-output.md`

**Context:** Every code snippet must be extracted from the working CSV analyzer (Task 15). Each chapter builds incrementally.

**Step 1:** Write chapters 1-5.
**Step 2:** Test every snippet compiles.
**Step 3:** Commit: `docs: add CSV analyzer tutorial`

---

## Phase 6: Ship It

### Task 21: CHANGELOG.md

**Files:**
- Create: `CHANGELOG.md`

**Step 1:** Write CHANGELOG following Keep a Changelog format:

```markdown
# Changelog

## [0.1.0] - 2026-04-07

### Added
- 21-crate Rust compiler with dual Cranelift/LLVM backends
- Ownership-based memory safety without lifetime annotations
- Capability-based security with compile-time enforcement
- Compile-time evaluation with `comptime` blocks
- Dynamic dispatch with `dyn Trait`
- Concurrency: spawn, channels, actors, select
- 28 standard library modules
- 5 MIR optimization passes
- VS Code extension (syntax highlighting, snippets)
- LSP server, formatter, doc generator, REPL
- Package manager with project scaffolding
- 13 example programs
- 680+ tests
```

**Step 2:** Commit: `docs: add CHANGELOG.md`

---

### Task 22: GitHub Actions release workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Step 1:** Create release workflow triggered on tag push:

```yaml
name: Release
on:
  push:
    tags: ['v*']

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: kryos-linux-x86_64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: kryos-macos-arm64
          - os: macos-13
            target: x86_64-apple-darwin
            artifact: kryos-macos-x86_64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: kryos-windows-x86_64.exe
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build
        working-directory: compiler
        run: cargo build --release -j 4
      - name: Upload
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: compiler/target/release/kryos${{ matrix.os == 'windows-latest' && '.exe' || '' }}

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
      - name: Create Release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            kryos-linux-x86_64/kryos
            kryos-macos-arm64/kryos
            kryos-macos-x86_64/kryos
            kryos-windows-x86_64.exe/kryos.exe
```

**Step 2:** Commit: `ci: add GitHub Actions release workflow`

---

### Task 23: Install script

**Files:**
- Create: `install.sh` at repo root

**Step 1:** Write ~50-line install script:

```bash
#!/bin/sh
set -e

REPO="FrostbyteDevTeam/kryos-lang"
INSTALL_DIR="$HOME/.kryos/bin"

# Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS-$ARCH" in
  linux-x86_64)   ARTIFACT="kryos-linux-x86_64" ;;
  darwin-arm64)    ARTIFACT="kryos-macos-arm64" ;;
  darwin-x86_64)   ARTIFACT="kryos-macos-x86_64" ;;
  *)               echo "Unsupported platform: $OS-$ARCH"; exit 1 ;;
esac

# Get latest release
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
URL="https://github.com/$REPO/releases/download/$LATEST/$ARTIFACT"

echo "Installing Kryos $LATEST for $OS-$ARCH..."
mkdir -p "$INSTALL_DIR"
curl -fsSL "$URL" -o "$INSTALL_DIR/kryos"
chmod +x "$INSTALL_DIR/kryos"

# Add to PATH hint
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
  echo ""
  echo "Add Kryos to your PATH:"
  echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
  echo ""
  echo "Add this to your ~/.bashrc or ~/.zshrc to make it permanent."
fi

echo "Kryos installed successfully! Run 'kryos version' to verify."
```

**Step 2:** Commit: `feat: add install script for one-command installation`

---

### Task 24: README polish

**Files:**
- Modify: `README.md`

**Step 1:** Restructure README to lead with installation:

```markdown
## Install

```bash
curl -fsSL https://raw.githubusercontent.com/FrostbyteDevTeam/kryos-lang/master/install.sh | sh
```

Or build from source:
```bash
git clone ...
cd kryos-lang/compiler && cargo build --release -j 4
```

Add CI badge, license badge at top.

**Step 2:** Update example count and program list.

**Step 3:** Commit: `docs: polish README with install-first structure and badges`

---

### Task 25: Final integration sweep

**Files:**
- All

**Step 1:** Run full test suite: `cargo test --release -j 4`
**Step 2:** Run ALL examples (8 original + 5 new programs)
**Step 3:** Run `kryos pkg init testproj && cd testproj && kryos run src/main.kry`
**Step 4:** Run benchmarks: `cargo bench --release -j 4`
**Step 5:** Verify tutorial snippets compile
**Step 6:** Fix any remaining issues

**Step 7:** Final commit: `chore: v0.1.0 release preparation — all examples, tests, and tutorial verified`

---
