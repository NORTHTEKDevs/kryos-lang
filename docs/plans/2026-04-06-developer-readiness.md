# Kryos Developer Readiness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Kryos usable enough that a developer can follow a tutorial, build real programs, and not rage-quit.

**Architecture:** Fix the stdlib resolution path so `use std::X` finds `compiler/stdlib/X.kry`, register ergonomic builtins so common operations don't need `extern` blocks, add runtime stack traces via software call tracking, write 5 real programs to surface and fix codegen bugs, scaffold a package ecosystem, and create a working tutorial.

**Tech Stack:** Rust (compiler), Kryos (stdlib/examples), Cranelift (codegen), criterion (benchmarks)

---

### Task 1: Fix module resolution for `std::` prefix

**Files:**
- Modify: `crates/kryos-driver/src/resolve.rs:61-113` (resolve_module_path function)
- Test: `crates/kryos-driver/tests/driver.rs` (add new test)

**Context:** Currently `use std::string` looks for `std/string.kry` relative to the importing file, walking up ancestor dirs for `src/`. It does NOT know about the stdlib directory at `compiler/stdlib/`. We need to add a fallback search path.

**Step 1:** In `resolve_module_path()`, after the existing search paths (sibling file, directory module, project src/), add a fallback that checks for a `stdlib/` directory relative to the compiler binary or `CARGO_MANIFEST_DIR`. The path should be: if the first segment is `std`, strip it and look in `<compiler_root>/stdlib/<rest>.kry`.

Specifically, in `resolve_module_path`, after line ~107 (the existing search loop), add:

```rust
// Fallback: stdlib directory for `use std::X` imports.
// Resolve "std::string" → "<stdlib_dir>/string.kry"
if segments.first().map(|s| s.as_str()) == Some("std") && segments.len() >= 2 {
    let stdlib_dir = find_stdlib_dir();
    if let Some(dir) = stdlib_dir {
        let rest = &segments[1..];
        let rel_path = rest.join("/");
        let candidate = dir.join(format!("{}.kry", rel_path));
        if candidate.exists() {
            return Ok(candidate);
        }
        let candidate_mod = dir.join(&rel_path).join("mod.kry");
        if candidate_mod.exists() {
            return Ok(candidate_mod);
        }
    }
}
```

Add a `find_stdlib_dir()` helper that checks:
1. `KRYOS_STDLIB_DIR` env var (for testing/overrides)
2. `<executable_dir>/../stdlib/`
3. `CARGO_MANIFEST_DIR` ancestor's `stdlib/` (for dev builds)

**Step 2:** Write a test in `crates/kryos-driver/tests/driver.rs` that creates a temp .kry file with `use std::math`, compiles it via `compile_file`, and verifies no import errors.

**Step 3:** Run `cargo test -p kryos-driver` — verify pass.

---

### Task 2: Verify stdlib .kry files compile

**Files:**
- Modify: various stdlib .kry files (fix errors as found)
- Create: `crates/kryos-driver/tests/stdlib_compile.rs`

**Context:** The 28 stdlib .kry files in `compiler/stdlib/` have never been tested through the compiler. They use a mix of `extern` FFI blocks and `__builtin_*` functions. Many likely have parse or type errors.

**Step 1:** Create a test file `crates/kryos-driver/tests/stdlib_compile.rs` that iterates over every .kry file in the stdlib directory and runs `check_file()` on each one. Collect all diagnostics.

```rust
#[test]
fn all_stdlib_modules_compile() {
    let stdlib_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap()
        .join("stdlib");
    
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&stdlib_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "kry").unwrap_or(false) {
            let (diags, _sm) = check_file(&path);
            let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
            if !errors.is_empty() {
                failures.push((path.file_name().unwrap().to_string_lossy().to_string(), errors.len()));
            }
        }
    }
    
    assert!(failures.is_empty(), "Stdlib files with errors: {:?}", failures);
}
```

**Step 2:** Run the test. It WILL fail — many stdlib files will have errors because:
- `__builtin_len` may not be recognized by the type checker
- `extern` blocks may use `ptr` type which may not parse
- Some files reference types/functions from other stdlib modules without importing them

**Step 3:** Fix each failing stdlib file. Common fixes:
- Replace `ptr` with `i64` in extern signatures (the compiler uses i64 slot model)
- Add missing `__builtin_*` recognition to type checker or replace with runtime function calls
- Add explicit imports where cross-module references exist
- Comment out or stub functions that depend on unimplemented features

**Step 4:** Run until all 28 modules pass `check_file()`.

---

### Task 3: Register ergonomic builtins for common operations

**Files:**
- Modify: `crates/kryos-mir/src/lower.rs:257-290` (builtin ret types)
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs:1295-1342` (print handling — extend pattern for other builtins)
- Modify: `crates/kryos-codegen-cranelift/src/jit.rs` (register new symbols)
- Modify: `crates/kryos-rt/src/builtins.rs` (add new wrapper functions)
- Test: `crates/kryos-test-runner/tests/e2e/basics/` (new run-expect tests)

**Context:** Developers should be able to call `file_read("data.txt")`, `time_now()`, `env_get("HOME")` etc. without writing `extern` blocks. These need to be recognized as builtins and mapped to runtime functions.

**Step 1:** Add i64-based wrapper functions to `crates/kryos-rt/src/builtins.rs` for the most common operations:

```rust
/// Read an entire file to a KryosString. Takes a KryosString path handle.
#[no_mangle]
pub extern "C" fn kryos_builtin_file_read(path_handle: i64) -> i64 {
    // Extract path string from handle, call std::fs::read_to_string,
    // wrap result in KryosString, return handle
}

#[no_mangle]
pub extern "C" fn kryos_builtin_file_write(path_handle: i64, content_handle: i64) -> i64 { ... }

#[no_mangle]
pub extern "C" fn kryos_builtin_env_get(key_handle: i64) -> i64 { ... }

#[no_mangle]
pub extern "C" fn kryos_builtin_time_now() -> i64 { ... }

#[no_mangle]
pub extern "C" fn kryos_builtin_assert(condition: i64, msg_handle: i64) { ... }

#[no_mangle]
pub extern "C" fn kryos_builtin_args() -> i64 { ... } // returns array handle

#[no_mangle]
pub extern "C" fn kryos_builtin_parse_int(s_handle: i64) -> i64 { ... }

#[no_mangle]
pub extern "C" fn kryos_builtin_parse_float(s_handle: i64) -> f64 { ... }

#[no_mangle]
pub extern "C" fn kryos_builtin_type_of(value: i64) -> i64 { ... } // returns string handle
```

**Step 2:** Register return types in MIR lowerer (`lower.rs:257+`):

```rust
("file_read", MirType::Str),
("file_write", MirType::Void),
("env_get", MirType::Str),
("time_now", MirType::I64),
("assert", MirType::Void),
("args", MirType::I64),  // array handle
("type_of", MirType::Str),
```

**Step 3:** Add special-case handling in Cranelift codegen for these builtins (similar to how `println` is handled). In `codegen.rs`, in the `RValue::Call` match, add mapping:

```rust
"file_read" => ("kryos_builtin_file_read", 1),
"file_write" => ("kryos_builtin_file_write", 2),
"env_get" => ("kryos_builtin_env_get", 1),
"time_now" => ("kryos_builtin_time_now", 0),
"assert" => ("kryos_builtin_assert", 2),
"args" => ("kryos_builtin_args", 0),
```

**Step 4:** Register in JIT builder (`jit.rs`).

**Step 5:** Write run-expect tests proving each works.

---

### Task 4: Wire panic handler and runtime checks into codegen

**Files:**
- Modify: `crates/kryos-codegen-cranelift/src/jit.rs` (register panic symbols)
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs` (emit panic calls)
- Modify: `crates/kryos-rt/src/builtins.rs` (add bounds check + div-by-zero helpers)
- Test: `crates/kryos-test-runner/tests/e2e/error_cases/` (new tests)

**Step 1:** Register panic functions in JIT builder:

```rust
jit_builder.symbol("kryos_panic", kryos_rt::panic::kryos_panic as *const u8);
jit_builder.symbol("kryos_panic_with_location", kryos_rt::panic::kryos_panic_with_location as *const u8);
```

**Step 2:** Add runtime check helper to builtins.rs:

```rust
#[no_mangle]
pub extern "C" fn kryos_check_div_zero(divisor: i64, file_ptr: *const u8, file_len: usize, line: u32, col: u32) {
    if divisor == 0 {
        kryos_panic_with_location(
            "division by zero\0".as_ptr(), 16,
            file_ptr, file_len, line, col
        );
    }
}

#[no_mangle]
pub extern "C" fn kryos_check_bounds(index: i64, length: i64, file_ptr: *const u8, file_len: usize, line: u32, col: u32) {
    if index < 0 || index >= length {
        // format message and call kryos_panic_with_location
    }
}
```

**Step 3:** In Cranelift codegen, before every integer division instruction, emit a call to `kryos_check_div_zero`. Before every array index, emit `kryos_check_bounds`. Pass the source file name and span line/col from the MIR instruction's metadata.

Note: MIR instructions currently don't carry source spans. You may need to add a `span: Option<(u32, u32)>` field to relevant MIR instructions, or pass the function's file name and approximate location.

**Step 4:** Write e2e tests:
- `error_cases/div_by_zero_run.kry`: `// run-expect: kryos panic` with `let x = 1 / 0`
- `error_cases/assert_fail_run.kry`: `// run-expect: kryos panic` with `assert(false, "test")`

---

### Task 5: Software call stack tracking (stack traces)

**Files:**
- Create: `crates/kryos-rt/src/trace.rs`
- Modify: `crates/kryos-rt/src/lib.rs` (add `pub mod trace`)
- Modify: `crates/kryos-rt/src/panic.rs` (print stack on panic)
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs` (emit trace_enter/exit)
- Modify: `crates/kryos-codegen-cranelift/src/jit.rs` (register trace symbols)
- Test: run-expect test that verifies stack trace output

**Step 1:** Create `crates/kryos-rt/src/trace.rs`:

```rust
use std::cell::RefCell;

struct TraceFrame {
    func_name: &'static str,  // we'll use C string pointers
    file: &'static str,
    line: u32,
}

thread_local! {
    static CALL_STACK: RefCell<Vec<TraceFrame>> = RefCell::new(Vec::with_capacity(64));
}

#[no_mangle]
pub extern "C" fn kryos_trace_enter(
    name_ptr: *const u8, name_len: usize,
    file_ptr: *const u8, file_len: usize,
    line: u32,
) {
    let name = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len)) };
    let file = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(file_ptr, file_len)) };
    CALL_STACK.with(|stack| {
        stack.borrow_mut().push(TraceFrame {
            func_name: unsafe { std::mem::transmute(name) },
            file: unsafe { std::mem::transmute(file) },
            line,
        });
    });
}

#[no_mangle]
pub extern "C" fn kryos_trace_exit() {
    CALL_STACK.with(|stack| { stack.borrow_mut().pop(); });
}

/// Format the current call stack for display.
pub fn format_stack_trace() -> String {
    CALL_STACK.with(|stack| {
        let stack = stack.borrow();
        if stack.is_empty() {
            return String::new();
        }
        let mut out = String::from("\nstack trace (most recent call last):\n");
        for (i, frame) in stack.iter().enumerate() {
            out.push_str(&format!("  {}: {}() at {}:{}\n", i, frame.func_name, frame.file, frame.line));
        }
        out
    })
}
```

**Step 2:** Modify `panic.rs` to call `format_stack_trace()` before aborting:

```rust
pub extern "C" fn kryos_panic_with_location(...) -> ! {
    let formatted = format_panic_with_location(msg, file, line, col);
    let stack = crate::trace::format_stack_trace();
    let _ = writeln!(std::io::stderr(), "{}{}", formatted, stack);
    std::process::abort();
}
```

**Step 3:** In Cranelift codegen, at the start of every function body (after the entry block), emit:

```
call kryos_trace_enter(<func_name_ptr>, <func_name_len>, <file_ptr>, <file_len>, <line>)
```

At every return instruction, emit `call kryos_trace_exit()` before the return. The function name and file name should be embedded as data constants in the object module.

**Step 4:** Register `kryos_trace_enter` and `kryos_trace_exit` in the JIT builder.

**Step 5:** Write a run-expect test that triggers a panic inside a nested function call and verifies the stack trace appears in stderr.

---

### Task 6: Write real program — CLI Calculator

**Files:**
- Create: `examples/calculator.kry` (~100 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/calculator_run.kry` (simplified test version)

**The program:**
- Reads command-line args (`args()`)
- Evaluates simple math expressions: `kryos run examples/calculator.kry "2 + 3 * 4"`
- Supports: +, -, *, /, parentheses
- Prints the result
- Error handling for invalid input

**Test version** (run-expect): A simplified version that evaluates hardcoded expressions and prints results.

**Fix any codegen bugs** that surface during development. Document each bug and fix.

---

### Task 7: Write real program — File Line Counter (wc clone)

**Files:**
- Create: `examples/wc.kry` (~150 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/wc_run.kry`

**The program:**
- `kryos run examples/wc.kry <filename>`
- Reads a file with `file_read(path)`
- Counts lines, words, characters
- Prints formatted output: `  42  301  1847 filename.txt`

**Test version:** Creates a temp file with known content, counts it, verifies output.

---

### Task 8: Write real program — JSON Key Counter

**Files:**
- Create: `examples/json_keys.kry` (~200 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/json_run.kry`

**The program:**
- Reads a JSON file
- Uses the stdlib `json.kry` module (via `use std::json`)
- Counts occurrences of each key
- Prints sorted output

**This task depends on Task 1 (module resolution) and Task 2 (stdlib compiles).**

---

### Task 9: Write real program — TCP Echo Server

**Files:**
- Create: `examples/echo_server.kry` (~150 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/echo_run.kry`

**The program:**
- Binds to a port
- Accepts connections in a loop
- Spawns a handler per connection
- Echoes back whatever is received
- Uses `extern` for TCP functions (or stdlib if available)

**Test version:** Simplified — spawns server, connects to it, sends data, verifies echo.

---

### Task 10: Write real program — CSV Analyzer

**Files:**
- Create: `examples/csv_stats.kry` (~250 lines)
- Create: `crates/kryos-test-runner/tests/e2e/programs/csv_run.kry`

**The program:**
- Reads a CSV file
- Parses header and rows
- Computes per-column stats: count, min, max, sum, average
- Prints formatted table

**This is the most complex program. It exercises:**
- File I/O
- String splitting and parsing
- Structs and methods
- Arrays and iteration
- Float math
- Formatted output

---

### Task 11: Package ecosystem — registry and publish

**Files:**
- Modify: `crates/kryos-cli/src/commands/pkg.rs` (add `publish` subcommand)
- Modify: `crates/kryos-package/src/lib.rs` (add registry module)
- Create: `crates/kryos-package/src/registry.rs`
- Modify: `crates/kryos-package/src/manifest.rs` (add `[registry]` section)

**Step 1:** Add a `registry` module to kryos-package that:
- Defines a default registry URL (GitHub-based index repo)
- Can fetch package metadata from the index
- Can download package tarballs

**Step 2:** Implement `kryos pkg publish`:
- Reads `kryos.toml` manifest
- Packages `src/` directory into a tarball
- Generates index entry JSON
- Prints instructions for pushing to registry (or auto-pushes if git remote is configured)

**Step 3:** Implement `kryos pkg update` (currently a stub):
- Reads `[dependencies]` from manifest
- Resolves versions against registry index
- Downloads and extracts packages to `<project>/.kryos/deps/`
- Generates `kryos.lock`

**Step 4:** Create 3 starter packages as examples:
- `std-test`: Test assertions and test runner
- `std-cli`: Argument parsing
- `std-csv`: CSV reader

These live in `packages/` at the repo root and serve as both usable packages and templates.

---

### Task 12: Project scaffolding improvements

**Files:**
- Modify: `crates/kryos-cli/src/commands/pkg.rs` (improve `init`)

**Step 1:** Improve `kryos pkg init` to create:
- `kryos.toml` with proper metadata
- `src/main.kry` with a working hello world
- `.gitignore` (ignoring `target/`, `.kryos/`, `*.o`)
- `README.md` with build instructions

**Step 2:** The generated `src/main.kry` should be:
```kryos
fn main() {
    println("Hello from Kryos!")
}
```

And it should compile and run successfully with `kryos run src/main.kry`.

---

### Task 13: Tutorial — "Build a CSV Analyzer in Kryos"

**Files:**
- Create: `docs/tutorial/01-getting-started.md`
- Create: `docs/tutorial/02-reading-files.md`
- Create: `docs/tutorial/03-parsing-csv.md`
- Create: `docs/tutorial/04-computing-stats.md`
- Create: `docs/tutorial/05-formatting-output.md`

**Context:** This tutorial walks through building the CSV analyzer from Task 10. Every code snippet must be extracted from the working program and verified to compile.

**Structure:**
1. Install Kryos, create a project, run hello world
2. Reading a file with `file_read()`, splitting into lines
3. Parsing CSV: splitting by comma, handling headers
4. Computing stats: structs for column data, min/max/avg
5. Formatting output: aligned columns, float formatting

Each chapter builds incrementally and includes the exact command to run and expected output.

---

### Task 14: Final integration testing

**Files:**
- Modify: `crates/kryos-test-runner/tests/e2e_runner.rs` (if needed)

**Step 1:** Run the full test suite: `cargo test --workspace`
**Step 2:** Run all 5 real programs as native binaries
**Step 3:** Run the tutorial from scratch in a clean directory
**Step 4:** Verify `kryos pkg init` → `kryos run` flow
**Step 5:** Fix any remaining issues

---
