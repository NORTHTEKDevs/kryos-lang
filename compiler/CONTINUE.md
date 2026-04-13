# Kryos Compiler — Finish for Investor/Presentation Readiness

## Directive

I want perfection, and I don't care how long you need to work autonomously to get there. Fix ALL remaining issues for public release / VC readiness. Do not stop until every task is done and every test passes.

## CRITICAL BUILD CONSTRAINT

**Debug builds consume 48GB RAM and will OOM.** ALWAYS use:
```
cargo build --release -j 4
cargo test --release -j 4
```
Never omit `--release`. Never omit `-j 4`.

---

## Project Overview

Kryos is a 21-crate Rust compiler (~46k lines) for a custom systems language with:
- Cranelift AOT + JIT backends, LLVM IR text emitter
- Ownership/borrowing, capabilities, enums with payloads, closures, pattern matching, structs with methods, channels, try/catch
- 11 CLI subcommands: `build`, `run`, `check`, `repl`, `test`, `fmt`, `doc`, `bindgen`, `pkg`, `lsp`, `version`
- 823 Rust-level tests across 37 test suites (36 pass, 1 fails)
- 98 native build tests in `crates/kryos-test-runner/tests/native/*.kry` (97 pass, 1 fails)
- 18 file-level tests in `tests/*.kry` (17 pass, 1 fails)
- 13 example programs in `examples/*.kry`
- 16-file self-hosting compiler in `self-host/*.kry` (~19k lines)
- 6 MIR optimization passes: inline, constant fold, pure (CSE + dead call), DCE, TCO, strength reduction

### Crate Map

| Crate | Purpose | Lines |
|-------|---------|-------|
| `kryos-cli` | CLI entry point, 11 subcommands | ~700 |
| `kryos-lexer` | Tokenizer | ~1400 |
| `kryos-parser` | Recursive descent parser | ~2200 |
| `kryos-ast` | AST types | ~900 |
| `kryos-types` | Type checker (`check.rs` is 2684 lines) | ~3200 |
| `kryos-mir` | MIR lowering + 6 optimization passes (`lower.rs` is 4798 lines) | ~7500 |
| `kryos-codegen-cranelift` | Cranelift AOT + JIT (`codegen.rs` is 4451 lines, `jit.rs` ~700) | ~5800 |
| `kryos-codegen-llvm` | LLVM IR text emitter (`codegen.rs` is 3288 lines) | ~3500 |
| `kryos-linker` | Native linker invocation | ~400 |
| `kryos-driver` | Pipeline orchestration | ~600 |
| `kryos-rt` | Runtime library (builtins, panic, trace, string interning) | ~1200 |
| `kryos-stdlib-native` | Native stdlib (math, string, io, env functions) | ~400 |
| `kryos-errors` | Error types + colored diagnostics | ~800 |
| `kryos-ownership` | Ownership/borrow checker + Arc insertion | ~1200 |
| `kryos-capabilities` | Capability system | ~300 |
| `kryos-test-runner` | @test annotation runner + native build test harness | ~800 |
| `kryos-fmt` | Code formatter | ~1500 |
| `kryos-doc` | Documentation generator | ~800 |
| `kryos-lsp` | Language server protocol | ~1100 |
| `kryos-package` | Package manager (manifest, semver, resolve, lock, registry) | ~1740 |
| `kryos-bindgen` | C header → Kryos bindings | ~1580 |

### Kryos Language Syntax Cheat Sheet

- **Boolean operators**: `and`, `or`, `not` (NOT `&&`, `||`, `!`)
- **Types**: `i64`, `f64`, `str`, `bool` (NOT `int`, `Int`, `string`, `String`)
- **Variables**: `let x = 5` (immutable), `let mut x = 5` (mutable)
- **Functions**: `fn name(param: Type) -> ReturnType { body }`
- **Enum construction**: `Shape.Circle(5.0)` (dot syntax)
- **Match destructuring**: `Shape::Circle(r) => r * r` (double-colon in patterns)
- **String conversion**: `to_string(value)` builtin
- **Print**: `println(str_value)` — takes a string, not arbitrary types
- **Assert**: `assert(condition)` or `assert(condition, "message")`
- **Conditionals**: `if`, `elif`, `else`
- **Loops**: `for x in collection`, `while condition`, `break`, `continue`
- **Annotations**: `@pure`, `@test`, `@inline`, `@deprecated`
- **Imports**: `use module_name` (cross-file imports have issues — see bugs below)

---

## BUGS TO FIX (ordered by investor-visibility)

### Bug 1: Enum payloads lost when stored in arrays and iterated — HIGHEST PRIORITY

**Symptom**: `examples/shapes.kry` outputs all zeros for area/perimeter and "small" for every shape.

```
Circle(r=5)
  area      = 0
  perimeter = 0
  size      = small
```

**Root cause**: Enum payloads (f64 values inside `Shape::Circle(5.0)`, `Shape::Rectangle(10.0, 20.0)`, etc.) are lost when:
1. Enums are stored in an array: `let shapes = [Shape.Circle(5.0), ...]`
2. Iterated with `for sh in shapes { ... }`
3. Passed to a function: `area(sh)` → `match sh { Shape::Circle(r) => ... }`

**Proof that enums work in simpler cases**: The native test `enum_f64_payload.kry` creates `Shape.Circle(5.0)`, passes it directly to `area()`, matches and computes `3.14159 * r * r` = `78.53975` — and this test PASSES. The difference is it doesn't go through an array + for loop.

**Similarly**: `examples/proof.kry` line 8 does `match Shape.Circle(5.0) { Shape::Circle(r) => 3.14159 * r * r, ... }` inline and gets `78.53975` correctly.

**Where to investigate**:
- `crates/kryos-codegen-cranelift/src/codegen.rs` — how arrays of enums are allocated, how enum payloads are stored/loaded from array elements, how for-loop iteration extracts elements
- `crates/kryos-mir/src/lower.rs` — how array literals with enum elements are lowered to MIR
- The bug is likely in how enum values are copied into array slots or extracted during iteration. Enum payloads require storing both a discriminant tag AND payload data. If the array element size only accounts for the tag (or copies only the tag), payloads read as zero.

**Verification**: Fix the bug, then run `cargo run --release -j 4 -- run examples/shapes.kry`. Expected output should show `area = 78.53975` for Circle(r=5), `area = 200` for Rectangle(10x20), etc.

### Bug 2: Closures capturing function-typed parameters — linker error

**Symptom**: `cargo test --release -j 4` fails 1/37 test suites: `native_build_tests` in `kryos-test-runner`.

The failing test is `crates/kryos-test-runner/tests/native/closure_capture_fn.kry`:
```kryos
fn compose(f: fn(i64) -> i64, g: fn(i64) -> i64) -> fn(i64) -> i64 {
    return fn(x: i64) -> i64 {
        return f(g(x))
    }
}
```

**Error**: `unresolved external symbol g` / `unresolved external symbol f` — the closure captures `f` and `g` (function-typed parameters) but codegen emits them as external symbol references instead of reading from the closure environment.

**Where to investigate**:
- `crates/kryos-codegen-cranelift/src/codegen.rs` — closure codegen, specifically how captured variables of function type (`fn(i64) -> i64`) are stored in and loaded from the closure environment struct
- The simpler closure tests (`closure_basic.kry`, `closure_capture.kry`, `closure_escape.kry`, `closure_refcount.kry`) all PASS — those capture `i64` or `str` values. The bug is specific to capturing `fn(...)` typed values.

**Verification**: `cargo test --release -j 4 -p kryos-test-runner --test native_runner` should pass all 98 tests (currently 97/98).

### Bug 3: Cross-file module imports don't resolve — `use math` fails

**Symptom**: `kryos test` reports `tests/modules/main.kry` as FAIL:
```
error[E0102]: undefined variable `add`
 --> modules/main:4:18
  4 |     let result = add(3, 4)
     |                  ^^^ here
  = note: did you mean `abs`?
```

**The module file exists**: `tests/modules/math.kry` defines `fn add(a: i32, b: i32) -> i32`.

**Note**: The math.kry file uses `i32` types while the rest of Kryos uses `i64` — this may or may not be related. The primary issue is that `use math` doesn't make `add()` available in scope.

**Where to investigate**:
- `crates/kryos-driver/src/lib.rs` — how multi-file compilation resolves `use` statements
- `crates/kryos-types/src/check.rs` — how imported symbols are added to the type environment
- `crates/kryos-parser/` — how `use module` is parsed and what path resolution it triggers

**Verification**: `cargo run --release -j 4 -- test` should show 18/18 file tests passing.

### Bug 4: `markdown.kry` heap corruption on exit

**Symptom**: `cargo run --release -j 4 -- run examples/markdown.kry` produces correct output but then crashes:
```
error: process didn't exit successfully (exit code: 0xc0000374, STATUS_HEAP_CORRUPTION)
```

The program runs to completion and prints everything correctly — the crash happens during cleanup/exit. This is likely a double-free or use-after-free in the string runtime.

**Where to investigate**:
- `crates/kryos-rt/src/builtins.rs` — string allocation/deallocation, reference counting
- `crates/kryos-rt/src/lib.rs` — global string intern table cleanup
- `crates/kryos-codegen-cranelift/src/codegen.rs` — how string temporaries from concatenation are managed

**Verification**: `cargo run --release -j 4 -- run examples/markdown.kry` should exit cleanly (exit code 0).

### Bug 5: REPL doesn't work for expressions

**Symptom**: The REPL wraps input in `fn main() { let __expr__ = <input> }` but doesn't handle bare expressions, multi-line input, or variable persistence between lines. Typing `let x = 5` then `println(to_string(x))` fails because each line is compiled independently.

```
kryos> let x = 5
error: no `main` function found
kryos> println(to_string(x))
error: undefined variable `x`
```

**Where to investigate**:
- `crates/kryos-cli/src/commands/repl.rs` (or wherever the REPL is implemented)
- The REPL needs to accumulate state across lines, wrapping the accumulated buffer in a `main()` function on each evaluation

**Verification**: Start `cargo run --release -j 4 -- repl`, type `let x = 5`, then `println(to_string(x))` — should print `5`.

---

## POLISH TASKS (after bugs are fixed)

### Task 6: Get ALL examples producing correct output

Run every example and verify correct output:

| Example | Status | Issue |
|---------|--------|-------|
| `hello.kry` | PASS | Works correctly |
| `fibonacci.kry` | PASS | Works correctly |
| `calculator.kry` | PASS | Works correctly |
| `proof.kry` | PASS | All 17 inline tests pass |
| `channels.kry` | PASS | Works correctly |
| `grep.kry` | PASS | Works correctly |
| `word_count.kry` | PASS | Works correctly |
| `struct_test.kry` | PASS | Works correctly |
| `pure_fn.kry` | PASS | Works correctly |
| `test_annotation.kry` | PASS | Works correctly |
| `shapes.kry` | FAIL | All zeros — Bug 1 |
| `markdown.kry` | FAIL | Heap corruption on exit — Bug 4 |
| `struct_test2.kry` | WEIRD | Prints parser debug output — remove debug prints or fix the example |

If a broken example can't be fixed quickly, remove it from `examples/` so investors don't stumble on it. Working examples only.

### Task 7: Clean up `struct_test2.kry`

This example prints raw parser debug output:
```
Parser created, tokens: 7
parse_fn: start
parse_fn: after p_expect(FN)
...
```

Either remove the debug logging from the parser path this example hits, or replace this example with something that demonstrates struct features cleanly.

### Task 8: Audit and fix `kryos version` output

Currently prints `kryos 0.2.0` but the project is at `0.2.1`. Update version in `Cargo.toml` or wherever the version string is sourced.

### Task 9: Remove or gate the REPL's `main()` requirement error

If you can't fix the REPL properly (Bug 5), at minimum make it not show `error: no main function found` for every expression. A message like "REPL is not yet implemented" is better than a broken experience.

---

## STRETCH GOALS (if time permits, in priority order)

### Stretch 1: Self-hosting Stage-2

The self-hosting compiler is in `self-host/*.kry` (16 files, ~19k lines). Stage-1 (Rust compiler compiles Kryos self-host) works. Stage-2 (the resulting binary compiles the self-host again) segfaults. This is THE most impressive demo for investors ("Kryos compiles itself"). Debugging the segfault would be high-impact but may be very deep.

### Stretch 2: Add more native tests for edge cases

98 native tests is strong. Adding tests for:
- Arrays of enums (to prevent regression on Bug 1)
- Closures capturing function values (to prevent regression on Bug 2)
- String-heavy programs (to catch heap corruption like Bug 4)

### Stretch 3: Make `kryos lsp` functional enough for basic diagnostics

The LSP crate has 1100 lines of real code. If it can at least report parse errors, that's demo-worthy for editors. Low priority unless the core bugs are all fixed.

---

## VERIFICATION CHECKLIST (run this at the end)

```bash
# 1. Full test suite — must be 0 failures
cargo test --release -j 4 2>&1 | grep "test result:"
# Every line should say "ok"

# 2. All examples run without errors
for f in examples/*.kry; do
    echo "=== $f ==="
    cargo run --release -j 4 -- run "$f" 2>&1
    echo "exit: $?"
done
# Every exit code should be 0, no zeros where real values expected

# 3. File-level tests — must be 18/18
cargo run --release -j 4 -- test 2>&1
# Should show "18 passed, 0 failed"

# 4. CLI commands don't crash
cargo run --release -j 4 -- --help
cargo run --release -j 4 -- version
cargo run --release -j 4 -- check examples/hello.kry
cargo run --release -j 4 -- fmt examples/hello.kry
cargo run --release -j 4 -- doc examples/hello.kry

# 5. Verify shapes.kry specifically (the most visible bug)
cargo run --release -j 4 -- run examples/shapes.kry
# Circle(r=5) area should be ~78.5, not 0
```

---

## WHAT NOT TO DO

- Do NOT add new features, new syntax, new CLI commands, or new crates
- Do NOT refactor working code — if it works, leave it alone
- Do NOT change the MIR optimization pipeline order
- Do NOT change the `declare_runtime_builtins()` JIT setup — it was carefully debugged
- Do NOT change `extern "C"` on runtime functions to `extern "C-unwind"` — Cranelift JIT on Windows can't catch unwinding panics through JIT frames
- Do NOT add external dependencies unless absolutely necessary
- Do NOT touch the self-hosting compiler (`self-host/`) unless working on Stretch 1
- Do NOT write debug builds — always `--release -j 4`
