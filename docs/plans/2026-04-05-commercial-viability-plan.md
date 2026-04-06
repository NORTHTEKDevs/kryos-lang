# Kryos Commercial Viability Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate all Python references, fix every incomplete feature, and align documentation to reality — zero holes, zero false promises, commercially viable language.

**Architecture:** 21-crate Rust compiler workspace. Source → Lexer → Parser → AST → Type Checker → Ownership → Capabilities → MIR → Codegen (Cranelift AOT/JIT + LLVM IR text) → Linker. Runtime: kryos-rt (Rust staticlib linked into compiled binaries).

**Tech Stack:** Rust (compiler), Cranelift (AOT/JIT codegen), LLVM IR text (release codegen), C FFI (runtime builtins)

**Test command:** `cd /c/Users/Krist/projects/active/kryos-lang/compiler && cargo test`

**Current baseline:** 557+ tests, all passing, 0 failures.

---

## Actual State of Features (Corrected Audit)

| Feature | Status | Detail |
|---------|--------|--------|
| Basic `use foo` | WORKS | Flat export of all declarations |
| Multi-segment `use foo::bar` | PARSED, NOT WIRED | extract_imports() discards segments after first |
| `use foo as alias` | PARSED, NOT WIRED | ImportPath.alias field ignored |
| `use foo::{a, b}` | PARSED, NOT WIRED | ImportPath.items field ignored |
| Spawn/threads | WORKS | kryos_spawn + kryos_spawn_wait_all tested |
| Channels | WORKS | MPMC channels with i64 wrappers, tested |
| Heap strings | WORKS | KryosString with concat/slice/find/hash/eq/free |
| Try/catch | WORKS | Result enum desugaring through codegen |
| References &T | PARSED, BROKEN | Collapses to MirType::Ptr, loses mutability |
| Comptime | PARSED, BROKEN | Passthrough — no compile-time evaluation |
| Select | PARSED, BROKEN | Hardcoded to branch 0 |
| Parallel for | LEXER ONLY | Token exists, no AST node or codegen |
| Actors | LOWERED, NO RUNTIME | Handlers lower to mangled fns, no scheduler |
| Python FFI | DOCS ONLY | No Rust code, all in documentation |

---

## Phase 1: Python Purge + Documentation Honesty

### Task 1.1: Remove Python from use-extern docs and examples

**Files:**
- Modify: `docs/12-modules-and-packages.md`

**Step 1: Edit modules doc to remove Python FFI**

Remove all `use extern "python"` examples, `[dependencies.python]` sections, `pip install` references, and `--category python` CLI examples. Keep `use extern "c"` and `use extern "rust"` since those are the real FFI targets.

Specifically:
- Line 3: Remove mention of `extern "python"` from implementation status banner
- Line 22: Remove `use extern "python" numpy`
- Lines 37-44: Rewrite extern imports section — only show C and Rust
- Lines 119-121: Remove `[dependencies.python]` from kryos.toml example
- Lines 178-179: Remove `--category python` CLI example
- Lines 185: Remove Python from dependency category description
- Lines 195: Remove Python from `kryos remove` example
- Lines 213, 217: Remove pip install references
- Line 288: Remove python row from category table
- Lines 307-309: Remove python section from deps list example
- Lines 315-327: Rewrite practical example without Python deps

**Step 2: Run doc spell check**

```bash
grep -rn "python\|Python\|pip\|numpy\|pandas\|matplotlib" docs/12-modules-and-packages.md
```
Expected: zero matches.

**Step 3: Commit**

```bash
git add docs/12-modules-and-packages.md
git commit -m "docs: remove Python FFI from modules and packages chapter"
```

### Task 1.2: Remove Python from capabilities doc

**Files:**
- Modify: `docs/10-capabilities.md`

**Step 1: Remove ffi:python references**

- Line 140-141: Remove `fn call_python()` code example
- Line 146: Remove `ffi:python` capability line
- Line 326: Remove `ffi:python` from table
- Line 362: Remove `ffi_call_python` from function list

Replace with C FFI examples only.

**Step 2: Verify**

```bash
grep -rn "python\|Python\|ffi_call_python" docs/10-capabilities.md
```
Expected: zero matches.

**Step 3: Commit**

```bash
git add docs/10-capabilities.md
git commit -m "docs: remove ffi:python capability — Kryos FFI is C/Rust only"
```

### Task 1.3: Remove Python from stdlib docs

**Files:**
- Modify: `docs/stdlib/email.md`
- Modify: `docs/stdlib/net.md`
- Modify: `docs/stdlib/process.md`
- Modify: `docs/stdlib/regex.md`
- Modify: `docs/stdlib/server.md`

**Step 1: Fix each file**

- `email.md` line 3: Remove "uses Python's built-in `smtplib`" — replace with "Uses SMTP over TCP sockets"
- `net.md` line 5: Remove "Python package" references
- `net.md` line 187: Remove "Requires the `websocket-client` Python package"
- `process.md` line 47-48: Replace `python --version` example with `uname -a` or `echo hello`
- `regex.md` line 3: Replace "Python's `re` module" with "Perl-compatible regex syntax (PCRE)"
- `server.md` line 5: Remove "uses Python's `http.server` internally" — replace with "Uses a threaded TCP listener"

**Step 2: Verify no Python remains in stdlib docs**

```bash
grep -rn "python\|Python\|pip\|smtplib\|http\.server\|websocket-client" docs/stdlib/
```
Expected: zero matches.

**Step 3: Commit**

```bash
git add docs/stdlib/
git commit -m "docs: remove Python implementation references from stdlib"
```

### Task 1.4: Remove "Coming from Python" sections

**Files:**
- Modify: `docs/02-variables-and-types.md` (remove lines ~295-300)
- Modify: `docs/03-functions.md` (remove lines ~201-208)
- Modify: `docs/04-control-flow.md` (remove lines ~48 Python mention, ~268-274)
- Modify: `docs/05-structs-and-enums.md` (remove lines ~290-294)
- Modify: `docs/06-ownership.md` (remove lines ~11, ~312-317)
- Modify: `docs/07-error-handling.md` (remove lines ~281-291)
- Modify: `docs/08-traits-and-generics.md` (remove lines ~255-267)
- Modify: `docs/09-concurrency.md` (remove lines ~342-357)
- Modify: `docs/appendix/coming-from.md` (remove Python column/section)
- Modify: `docs/appendix/attributes.md` (remove `ffi:python` line)
- Modify: `docs/README.md` (update cross-ref text)

**Step 1: In each file, remove the "Coming from Python" section or Python-specific lines**

Keep "Coming from Rust", "Coming from JavaScript", "Coming from C" sections. Only purge Python.

For `docs/06-ownership.md` line 11: Replace "Garbage collection (Python, Go, Java)" with "Garbage collection (Go, Java, C#)".

For `docs/04-control-flow.md` line 48: Rewrite to remove Python reference. Change to: "Common mistake: using `else if` instead of `elif`. Kryos uses `elif` as a single keyword."

For `docs/appendix/coming-from.md`: Remove the "Coming from Python" section entirely. Keep Rust, JavaScript, C.

**Step 2: Verify**

```bash
grep -rn "python\|Python" docs/*.md docs/stdlib/*.md docs/appendix/*.md
```
Expected: zero matches (except possibly plan docs which are internal).

**Step 3: Commit**

```bash
git add docs/
git commit -m "docs: remove all Python references — Kryos has zero Python dependency"
```

### Task 1.5: Update implementation status banners for accuracy

**Files:**
- Modify: `docs/06-ownership.md` — update status to say references (&T, &mut T) are not yet enforced
- Modify: `docs/09-concurrency.md` — update select status to "parsed but codegen incomplete"
- Modify: `docs/11-comptime.md` — update status to "parsed and lowered but no compile-time evaluation; expressions run at runtime"

**Step 1: Read each file's status banner and fix**

Each Kryos manual chapter starts with a `> **Implementation Status:**` blockquote. Update these to be brutally honest about what works and what doesn't.

**Step 2: Commit**

```bash
git add docs/
git commit -m "docs: update implementation status banners for honesty"
```

---

## Phase 2: Module System Completion

### Task 2.1: Wire multi-segment import paths in resolve.rs

**Files:**
- Modify: `compiler/crates/kryos-driver/src/resolve.rs:113-127`
- Test: `compiler/crates/kryos-driver/tests/driver.rs`

**Step 1: Write the failing test**

Add to `compiler/crates/kryos-driver/tests/driver.rs`:

```rust
#[test]
fn compile_file_with_multi_segment_import() {
    // Test: main imports ml::math using `use ml::math`.
    // ml/math.kry should be resolved as a submodule.
    let dir = std::env::temp_dir().join("kryos_module_tests_multi_seg");
    fs::create_dir_all(dir.join("ml")).unwrap();

    let math_src = r#"fn multiply(a: i32, b: i32) -> i32 {
    return a * b
}
"#;

    let main_src = r#"use ml::math

fn main() {
    let result = multiply(3, 4)
    println("multi-segment import works")
}
"#;

    fs::write(dir.join("ml").join("math.kry"), math_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
    };

    let result = compile_file(&main_path, &config);

    assert!(
        result.success,
        "expected success but got errors: {:?}",
        result.diagnostics
    );
    assert_eq!(result.error_count(), 0);
    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(func_names.contains(&"multiply"));
    assert!(func_names.contains(&"main"));
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p kryos-driver compile_file_with_multi_segment_import -- --nocapture
```
Expected: FAIL — currently only resolves first segment "ml" as `ml.kry`, which doesn't exist.

**Step 3: Fix extract_imports to use full path**

In `compiler/crates/kryos-driver/src/resolve.rs`, modify `extract_imports` to return the full import path segments, not just the first:

```rust
pub fn extract_imports(module: &Module) -> Vec<(ImportPath, Span)> {
    module
        .declarations
        .iter()
        .filter_map(|decl| {
            if let Decl::Import { path, span } = decl {
                Some((path.clone(), *span))
            } else {
                None
            }
        })
        .collect()
}
```

**Step 4: Fix resolve_module_path to handle multi-segment paths**

Update `resolve_module_path` to join all segments into a directory path:

```rust
pub fn resolve_module_path(segments: &[String], importing_file: &Path) -> Result<PathBuf, ResolveError> {
    let module_name = segments.join("::");
    let parent = importing_file
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let mut search_paths = Vec::new();

    // Build the relative path from segments.
    // `use foo::bar::baz` → `foo/bar/baz.kry` or `foo/bar/baz/mod.kry`
    let relative: PathBuf = segments.iter().collect();

    // 1. Sibling path: /path/to/<seg1>/<seg2>/.../<segN>.kry
    let sibling = parent.join(&relative).with_extension("kry");
    search_paths.push(sibling.clone());
    if sibling.is_file() {
        return Ok(sibling);
    }

    // 2. Directory module: /path/to/<seg1>/<seg2>/.../<segN>/mod.kry
    let dir_mod = parent.join(&relative).join("mod.kry");
    search_paths.push(dir_mod.clone());
    if dir_mod.is_file() {
        return Ok(dir_mod);
    }

    // 3. Project src/ directory
    let mut ancestor = parent.to_path_buf();
    loop {
        let src_dir = ancestor.join("src");
        if src_dir.is_dir() {
            let src_sibling = src_dir.join(&relative).with_extension("kry");
            search_paths.push(src_sibling.clone());
            if src_sibling.is_file() {
                return Ok(src_sibling);
            }
            let src_dir_mod = src_dir.join(&relative).join("mod.kry");
            search_paths.push(src_dir_mod.clone());
            if src_dir_mod.is_file() {
                return Ok(src_dir_mod);
            }
            break;
        }
        if !ancestor.pop() {
            break;
        }
    }

    Err(ResolveError::NotFound {
        module_name,
        search_paths,
    })
}
```

**Step 5: Update resolve_imports to use new signatures**

In `resolve_imports`, change the loop from:
```rust
for (module_name, span) in imports {
    let module_path = match resolve_module_path(&module_name, importing_file) {
```
to:
```rust
for (import_path, span) in imports {
    let module_name = import_path.segments.join("::");
    let module_path = match resolve_module_path(&import_path.segments, importing_file) {
```

**Step 6: Run test to verify it passes**

```bash
cargo test -p kryos-driver compile_file_with_multi_segment_import -- --nocapture
```
Expected: PASS

**Step 7: Run full test suite**

```bash
cargo test
```
Expected: All existing tests still pass (resolve_module_path signature changed but callers updated).

**Step 8: Commit**

```bash
git add compiler/crates/kryos-driver/
git commit -m "feat(modules): support multi-segment import paths (use foo::bar)"
```

### Task 2.2: Wire selective imports (use foo::{a, b})

**Files:**
- Modify: `compiler/crates/kryos-driver/src/resolve.rs`
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`
- Test: `compiler/crates/kryos-driver/tests/driver.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn compile_file_with_selective_import() {
    let dir = std::env::temp_dir().join("kryos_module_tests_selective");
    fs::create_dir_all(&dir).unwrap();

    let math_src = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b
}

fn subtract(a: i32, b: i32) -> i32 {
    return a - b
}

fn multiply(a: i32, b: i32) -> i32 {
    return a * b
}
"#;

    // Only import add and multiply — subtract should NOT be in scope.
    let main_src = r#"use math::{add, multiply}

fn main() {
    let a = add(3, 4)
    let b = multiply(5, 6)
    println("selective import works")
}
"#;

    fs::write(dir.join("math.kry"), math_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
    };

    let result = compile_file(&main_path, &config);
    assert!(result.success, "errors: {:?}", result.diagnostics);

    let mir = result.mir.unwrap();
    let func_names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    // add and multiply should be imported.
    assert!(func_names.contains(&"add"));
    assert!(func_names.contains(&"multiply"));
    // subtract should NOT be imported.
    assert!(!func_names.contains(&"subtract"),
        "subtract should not be imported with selective import, got: {func_names:?}");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p kryos-driver compile_file_with_selective_import -- --nocapture
```
Expected: FAIL — subtract is currently included because items filtering is not implemented.

**Step 3: Implement selective import filtering in resolve_imports**

In `resolve_imports`, after collecting non-import declarations, filter by import items when specified:

```rust
// Collect non-import declarations from the imported module.
// If selective imports are specified (items non-empty), only keep matching names.
for decl in imported_module.declarations {
    if matches!(decl, Decl::Import { .. }) {
        continue;
    }
    if !import_path.items.is_empty() {
        // Selective import: only include declarations whose name matches an item.
        let decl_name = decl_name_of(&decl);
        if let Some(name) = decl_name {
            if import_path.items.contains(&name) {
                resolved_decls.push(decl);
            }
        }
    } else {
        resolved_decls.push(decl);
    }
}
```

Add helper function at the bottom of resolve.rs:

```rust
/// Extract the name of a declaration, if it has one.
fn decl_name_of(decl: &Decl) -> Option<String> {
    match decl {
        Decl::Function { name, .. } => Some(name.clone()),
        Decl::Struct { name, .. } => Some(name.clone()),
        Decl::Enum { name, .. } => Some(name.clone()),
        Decl::Trait { name, .. } => Some(name.clone()),
        Decl::TypeAlias { name, .. } => Some(name.clone()),
        Decl::Actor { name, .. } => Some(name.clone()),
        _ => None,
    }
}
```

**Step 4: Run test**

```bash
cargo test -p kryos-driver compile_file_with_selective_import -- --nocapture
```
Expected: PASS

**Step 5: Run full suite, commit**

```bash
cargo test && git add compiler/crates/kryos-driver/ && git commit -m "feat(modules): selective imports (use foo::{a, b})"
```

### Task 2.3: Wire import aliases (use foo as bar)

**Files:**
- Modify: `compiler/crates/kryos-driver/src/resolve.rs`
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`
- Test: `compiler/crates/kryos-driver/tests/driver.rs`

**Note:** Aliases in Kryos currently don't create namespaces (no `bar.func()` syntax). The alias just controls how declarations are named when imported. For now, `use math as m` means the same as `use math` — flat import of all declarations. The alias is a no-op until qualified access is implemented.

This task is about NOT breaking when an alias is present, and laying the groundwork.

**Step 1: Write the passing test**

```rust
#[test]
fn compile_file_with_aliased_import() {
    let dir = std::env::temp_dir().join("kryos_module_tests_alias");
    fs::create_dir_all(&dir).unwrap();

    let math_src = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b
}
"#;

    let main_src = r#"use math as m

fn main() {
    let result = add(3, 4)
    println("aliased import works")
}
"#;

    fs::write(dir.join("math.kry"), math_src).unwrap();
    fs::write(dir.join("main.kry"), main_src).unwrap();

    let main_path = dir.join("main.kry");
    let config = BuildConfig {
        input: main_path.to_string_lossy().to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Mir,
        target: None,
        capabilities: Vec::new(),
        verbose: true,
    };

    let result = compile_file(&main_path, &config);
    assert!(result.success, "errors: {:?}", result.diagnostics);
}
```

**Step 2: Verify this already works** (since alias is currently ignored, it should pass)

```bash
cargo test -p kryos-driver compile_file_with_aliased_import -- --nocapture
```
Expected: PASS (alias is harmless no-op).

**Step 3: Commit test**

```bash
git add compiler/crates/kryos-driver/tests/driver.rs
git commit -m "test(modules): add aliased import test (use foo as bar)"
```

### Task 2.4: Add e2e module tests to the test suite

**Files:**
- Create: `compiler/crates/kryos-test-runner/tests/e2e/modules/basic_import.kry`
- Create: `compiler/crates/kryos-test-runner/tests/e2e/modules/math_helper.kry`

**Note:** The e2e test runner discovers `.kry` files in `tests/e2e/` but currently does NOT handle multi-file tests (it compiles each file independently). This task documents the gap. For now, module tests live in the driver integration tests where `compile_file` is available.

**Step 1: Add a skip-annotated placeholder**

Create `compiler/crates/kryos-test-runner/tests/e2e/modules/basic_import.kry`:

```kryos
// skip — multi-file tests not yet supported by e2e runner (tested in driver integration tests)
use math_helper

fn main() {
    let result = add(3, 4)
    println("import works")
}
```

**Step 2: Commit**

```bash
git add compiler/crates/kryos-test-runner/tests/e2e/modules/
git commit -m "test: add placeholder e2e module test (skip until runner supports multi-file)"
```

---

## Phase 3: Comptime Evaluator

### Task 3.1: Implement basic compile-time constant evaluation

**Files:**
- Create: `compiler/crates/kryos-mir/src/consteval.rs`
- Modify: `compiler/crates/kryos-mir/src/lib.rs` (add `pub mod consteval;`)
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs` (call consteval pass before codegen)
- Test: `compiler/crates/kryos-mir/tests/mir.rs`

**Step 1: Write the failing test**

Add to `compiler/crates/kryos-mir/tests/mir.rs`:

```rust
#[test]
fn comptime_const_folding() {
    // comptime { 2 + 3 } should become ConstInt(5) after const-eval.
    use kryos_mir::ir::*;
    use kryos_mir::consteval::eval_comptime;

    let input = RValue::Comptime(Box::new(RValue::BinOp {
        op: BinOp::Add,
        left: Box::new(RValue::ConstInt(2)),
        right: Box::new(RValue::ConstInt(3)),
    }));

    let result = eval_comptime(&input);
    assert_eq!(result, RValue::ConstInt(5));
}

#[test]
fn comptime_nested() {
    use kryos_mir::ir::*;
    use kryos_mir::consteval::eval_comptime;

    // comptime { (2 + 3) * 4 } → ConstInt(20)
    let input = RValue::Comptime(Box::new(RValue::BinOp {
        op: BinOp::Mul,
        left: Box::new(RValue::Comptime(Box::new(RValue::BinOp {
            op: BinOp::Add,
            left: Box::new(RValue::ConstInt(2)),
            right: Box::new(RValue::ConstInt(3)),
        }))),
        right: Box::new(RValue::ConstInt(4)),
    }));

    let result = eval_comptime(&input);
    assert_eq!(result, RValue::ConstInt(20));
}

#[test]
fn comptime_non_const_passes_through() {
    use kryos_mir::ir::*;
    use kryos_mir::consteval::eval_comptime;

    // comptime { <non-const expr> } → passes through unchanged.
    let input = RValue::Comptime(Box::new(RValue::Use(LocalId(0))));
    let result = eval_comptime(&input);
    // Can't evaluate at compile time — unwrap the Comptime wrapper.
    assert_eq!(result, RValue::Use(LocalId(0)));
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p kryos-mir comptime -- --nocapture
```
Expected: FAIL — `consteval` module doesn't exist.

**Step 3: Implement consteval.rs**

Create `compiler/crates/kryos-mir/src/consteval.rs`:

```rust
//! Compile-time constant evaluator.
//!
//! Walks MIR `RValue::Comptime` nodes and evaluates constant expressions
//! at compile time, replacing them with literal constants.

use crate::ir::{BinOp, RValue, UnOp, MirFunction, MirModule, Instruction};

/// Evaluate a comptime RValue. Returns a simplified RValue.
///
/// If the expression is fully constant, returns a ConstInt/ConstFloat/ConstBool.
/// If it contains non-const operands, unwraps the Comptime wrapper and returns
/// the inner expression unchanged.
pub fn eval_comptime(rvalue: &RValue) -> RValue {
    match rvalue {
        RValue::Comptime(inner) => eval_rvalue(inner),
        other => other.clone(),
    }
}

/// Attempt to evaluate an RValue as a constant.
fn eval_rvalue(rvalue: &RValue) -> RValue {
    match rvalue {
        RValue::ConstInt(v) => RValue::ConstInt(*v),
        RValue::ConstFloat(v) => RValue::ConstFloat(*v),
        RValue::ConstBool(v) => RValue::ConstBool(*v),
        RValue::ConstNone => RValue::ConstNone,
        RValue::ConstString(s) => RValue::ConstString(s.clone()),
        RValue::Comptime(inner) => eval_rvalue(inner),
        RValue::BinOp { op, left, right } => {
            let l = eval_rvalue(left);
            let r = eval_rvalue(right);
            eval_binop(*op, &l, &r)
        }
        RValue::UnaryOp { op, operand } => {
            let v = eval_rvalue(operand);
            eval_unop(*op, &v)
        }
        // Non-constant — can't evaluate at compile time. Return as-is.
        other => other.clone(),
    }
}

fn eval_binop(op: BinOp, left: &RValue, right: &RValue) -> RValue {
    match (op, left, right) {
        // Integer arithmetic
        (BinOp::Add, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a.wrapping_add(*b)),
        (BinOp::Sub, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a.wrapping_sub(*b)),
        (BinOp::Mul, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a.wrapping_mul(*b)),
        (BinOp::Div, RValue::ConstInt(a), RValue::ConstInt(b)) if *b != 0 => RValue::ConstInt(a / b),
        (BinOp::Mod, RValue::ConstInt(a), RValue::ConstInt(b)) if *b != 0 => RValue::ConstInt(a % b),

        // Float arithmetic
        (BinOp::Add, RValue::ConstFloat(a), RValue::ConstFloat(b)) => RValue::ConstFloat(a + b),
        (BinOp::Sub, RValue::ConstFloat(a), RValue::ConstFloat(b)) => RValue::ConstFloat(a - b),
        (BinOp::Mul, RValue::ConstFloat(a), RValue::ConstFloat(b)) => RValue::ConstFloat(a * b),
        (BinOp::Div, RValue::ConstFloat(a), RValue::ConstFloat(b)) if *b != 0.0 => RValue::ConstFloat(a / b),

        // Integer comparisons
        (BinOp::Eq, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstBool(a == b),
        (BinOp::Ne, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstBool(a != b),
        (BinOp::Lt, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstBool(a < b),
        (BinOp::Le, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstBool(a <= b),
        (BinOp::Gt, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstBool(a > b),
        (BinOp::Ge, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstBool(a >= b),

        // Boolean ops
        (BinOp::And, RValue::ConstBool(a), RValue::ConstBool(b)) => RValue::ConstBool(*a && *b),
        (BinOp::Or, RValue::ConstBool(a), RValue::ConstBool(b)) => RValue::ConstBool(*a || *b),

        // Bitwise ops
        (BinOp::BitAnd, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a & b),
        (BinOp::BitOr, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a | b),
        (BinOp::BitXor, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a ^ b),
        (BinOp::Shl, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a.wrapping_shl(*b as u32)),
        (BinOp::Shr, RValue::ConstInt(a), RValue::ConstInt(b)) => RValue::ConstInt(a.wrapping_shr(*b as u32)),

        // Can't fold — rebuild the BinOp node.
        _ => RValue::BinOp {
            op,
            left: Box::new(left.clone()),
            right: Box::new(right.clone()),
        },
    }
}

fn eval_unop(op: UnOp, operand: &RValue) -> RValue {
    match (op, operand) {
        (UnOp::Neg, RValue::ConstInt(v)) => RValue::ConstInt(-v),
        (UnOp::Neg, RValue::ConstFloat(v)) => RValue::ConstFloat(-v),
        (UnOp::Not, RValue::ConstBool(v)) => RValue::ConstBool(!v),
        (UnOp::BitNot, RValue::ConstInt(v)) => RValue::ConstInt(!v),
        _ => RValue::UnaryOp {
            op,
            operand: Box::new(operand.clone()),
        },
    }
}

/// Run the comptime pass over an entire MIR module.
///
/// Walks all functions and replaces `RValue::Comptime(inner)` nodes with
/// their evaluated constants where possible.
pub fn run_comptime_pass(module: &mut MirModule) {
    for func in &mut module.functions {
        run_comptime_function(func);
    }
}

fn run_comptime_function(func: &mut MirFunction) {
    for block in &mut func.blocks {
        for instr in &mut block.instructions {
            if let Instruction::Assign { value, .. } = instr {
                *value = eval_comptime(value);
            }
        }
    }
}
```

**Step 4: Register the module**

Add `pub mod consteval;` to `compiler/crates/kryos-mir/src/lib.rs`.

**Step 5: Wire consteval pass into the pipeline**

In `compiler/crates/kryos-driver/src/pipeline.rs`, in `compile_module_impl`, after MIR lowering and before codegen:

```rust
// Comptime evaluation pass — evaluate compile-time expressions.
kryos_mir::consteval::run_comptime_pass(&mut mir);
```

**Step 6: Run tests**

```bash
cargo test -p kryos-mir comptime -- --nocapture
cargo test
```
Expected: All pass.

**Step 7: Commit**

```bash
git add compiler/crates/kryos-mir/src/consteval.rs compiler/crates/kryos-mir/src/lib.rs compiler/crates/kryos-driver/src/pipeline.rs compiler/crates/kryos-mir/tests/mir.rs
git commit -m "feat(comptime): implement compile-time constant evaluator"
```

---

## Phase 4: Select Statement Fix

### Task 4.1: Fix select to use try_recv polling instead of hardcoded 0

**Files:**
- Modify: `compiler/crates/kryos-mir/src/lower.rs` (lines ~775-836)
- Modify: `compiler/crates/kryos-codegen-llvm/src/codegen.rs` (add `kryos_chan_try_recv_i64` extern)
- Create: add `kryos_chan_try_recv_i64` wrapper in `compiler/crates/kryos-rt/src/builtins.rs`

**Step 1: Add try_recv i64 wrapper to runtime**

Add to `compiler/crates/kryos-rt/src/builtins.rs`:

```rust
/// Non-blocking receive. Returns the value if available, or i64::MIN as sentinel
/// for "no data yet".
#[no_mangle]
pub extern "C" fn kryos_chan_try_recv_i64(handle: i64) -> i64 {
    let mut buf: i64 = 0;
    let result = crate::channel::kryos_chan_try_recv(
        handle as *mut u8,
        &mut buf as *mut i64 as *mut u8,
        8,
    );
    if result > 0 {
        buf
    } else {
        i64::MIN // sentinel: no data
    }
}
```

**Step 2: Add extern declaration to LLVM codegen**

In the channel runtime declarations section:

```rust
self.emit_line("declare i64 @kryos_chan_try_recv_i64(i64)");
```

**Step 3: Fix select lowering in lower.rs**

The current select lowering hardcodes `RValue::ConstInt(0)` as the readiness index. Replace with a runtime polling loop that calls `try_recv` on each channel and branches to the first ready one.

This is complex enough that it should be a separate MIR pattern:

```
bb_poll:
  // Try channel 0
  %ready0 = call kryos_chan_try_recv_i64(ch0)
  if %ready0 != i64::MIN → goto bb_branch0
  // Try channel 1
  %ready1 = call kryos_chan_try_recv_i64(ch1)
  if %ready1 != i64::MIN → goto bb_branch1
  // No channel ready — yield and retry
  call kryos_sleep(0.001)
  goto bb_poll
```

**NOTE:** This is a simplified busy-poll approach. True epoll/kqueue multiplexing would be better but is a future optimization. The busy-poll with a small sleep is correct and simple.

**Step 4: Test with existing select e2e test or add one**

**Step 5: Run full suite, commit**

```bash
cargo test && git commit -am "feat(select): implement try_recv polling for select statement"
```

---

## Phase 5: Parallel For — Honest Decision

### Task 5.1: Remove parallel for from the language (for now)

**Decision:** `parallel for` is the most incomplete feature — it's just a keyword in the lexer with zero AST/codegen support. Rather than implement a thread pool + work-stealing scheduler (weeks of work), remove the keyword and document it as a future feature.

**Files:**
- Modify: `compiler/crates/kryos-lexer/src/token.rs` (remove Parallel token)
- Modify: `compiler/crates/kryos-parser/src/parser.rs` (remove any parallel for parsing, if any)
- Modify: docs (update any parallel for references)

**Step 1: Check if parser actually handles `parallel`**

```bash
grep -rn "Parallel\|parallel" compiler/crates/kryos-parser/src/
```

If the parser doesn't reference it, the keyword is already a dead token.

**Step 2: Remove the token or leave as reserved keyword**

Option A: Remove entirely — `parallel` becomes a valid identifier.
Option B: Keep as reserved keyword but error on use — "parallel for is planned but not yet implemented."

**Recommendation:** Option B. Keep it reserved so future code doesn't conflict.

**Step 3: Update docs**

Remove any parallel for documentation. Add a note: "Parallel iteration is planned for a future release."

**Step 4: Commit**

```bash
git commit -am "docs: mark parallel for as future feature (keyword reserved, not implemented)"
```

---

## Phase 6: References & Borrowing — Honest Scope

### Task 6.1: Document current ownership model honestly

**Files:**
- Modify: `docs/06-ownership.md`

The current ownership model is **move semantics** — values are moved on assignment, and use-after-move is detected. This is correct and useful. The gap is that `&T` and `&mut T` are parsed but not properly enforced.

**Step 1: Update docs to describe what actually works**

The status banner should say:

> **Implementation Status:** Move semantics and use-after-move detection are fully implemented. Shared references (`&T`) and mutable references (`&mut T`) are parsed but not yet enforced — the compiler treats them as raw pointers. Full borrow checking is planned.

**Step 2: Commit**

```bash
git add docs/06-ownership.md
git commit -m "docs: honest ownership status — moves work, borrows are planned"
```

### Task 6.2: Make &T and &mut T type-safe at the MIR level (future session)

**NOTE:** This is a large effort (estimated 2-3 sessions). The MIR needs:
1. `MirType::Ref { inner: Box<MirType>, mutable: bool }` (not just Ptr)
2. Reference creation instructions in MIR
3. Codegen for references (alloca + pointer in LLVM, variable address in Cranelift)
4. Borrow lifetime tracking in a new analysis pass

This is documented here for future sessions but NOT part of Phase 6 execution.

---

## Phase 7: Actor Runtime — Honest Scope

### Task 7.1: Document actors as experimental

**Files:**
- Modify: `docs/09-concurrency.md`

Actors lower to mangled functions (`MyActor__on_message`) but there's no mailbox or scheduler. Mark as experimental.

**Step 1: Update status**

> **Implementation Status:** Spawn, channels, and select are fully functional. Actors are experimental — handlers compile but the actor scheduler and mailbox runtime are not yet implemented. Use channels for production concurrency.

**Step 2: Commit**

---

## Execution Order Summary

| Phase | Tasks | Impact | Estimated Effort |
|-------|-------|--------|-----------------|
| 1: Python Purge | 1.1-1.5 | Honesty, brand integrity | 1-2 hours |
| 2: Module System | 2.1-2.4 | Multi-file programs work | 2-3 hours |
| 3: Comptime | 3.1 | Compile-time evaluation works | 1-2 hours |
| 4: Select Fix | 4.1 | Concurrent select works | 1-2 hours |
| 5: Parallel For | 5.1 | Remove false promise | 30 minutes |
| 6: References | 6.1-6.2 | Honest docs now, impl later | 30 min (docs), weeks (impl) |
| 7: Actors | 7.1 | Honest docs | 30 minutes |

**Total for Phases 1-5:** One focused session (~8 hours)
**Phase 6 impl + Phase 7 impl:** Future sessions

---

## Success Criteria

After all phases:
- `grep -rn "python\|Python" docs/` returns zero matches (except internal plan docs)
- Every feature documented in the manual either works end-to-end or has an honest status banner
- Multi-segment imports, selective imports, and aliases work
- Comptime blocks evaluate constants at compile time
- Select statement polls channels correctly
- All existing tests pass + 10+ new tests added
- Zero false promises in documentation
