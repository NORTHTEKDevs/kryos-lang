# Kryos Commercial Readiness — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Take the Kryos compiler from "broken build" to "commercial-grade language that impresses developers and investors."

**Architecture:** 6 concentric rings executed in order. Each ring leaves the compiler in a strictly better, testable state. The compiler is a 21-crate Rust workspace at `compiler/`. Dual backends: Cranelift (debug) + LLVM IR text (release). Tests use annotation-driven E2E runners and JIT unit tests.

**Tech Stack:** Rust 2021 edition, Cranelift, LLVM IR text output, clap CLI, tower-lsp, custom Kryos stdlib (.kry)

**Key Paths:**
- Workspace root: `compiler/`
- AST: `compiler/crates/kryos-ast/src/decl.rs`
- MIR lowering: `compiler/crates/kryos-mir/src/lower.rs`
- MIR IR: `compiler/crates/kryos-mir/src/ir.rs`
- Const eval: `compiler/crates/kryos-mir/src/consteval.rs`
- Cranelift codegen: `compiler/crates/kryos-codegen-cranelift/src/codegen.rs`
- LLVM codegen: `compiler/crates/kryos-codegen-llvm/src/codegen.rs`
- Pipeline: `compiler/crates/kryos-driver/src/pipeline.rs`
- Ownership: `compiler/crates/kryos-ownership/src/analysis.rs`
- Type system: `compiler/crates/kryos-types/src/ty.rs`
- Doc gen: `compiler/crates/kryos-doc/src/lib.rs`
- Formatter: `compiler/crates/kryos-fmt/src/formatter.rs`
- Driver tests: `compiler/crates/kryos-driver/tests/driver.rs`
- E2E tests: `compiler/crates/kryos-test-runner/tests/e2e/`
- Native tests: `compiler/crates/kryos-test-runner/tests/native/`

---

## RING 0 — Fix the Build

### Task 0.1: Handle Decl::Const in kryos-doc

**Files:**
- Modify: `compiler/crates/kryos-doc/src/lib.rs:377-379`

**Step 1: Add the Const match arm**

The match at line 184 in `doc_item_from_decl` handles all Decl variants except Const. The pattern at line 377-378 currently has:
```rust
        Decl::Impl { .. } | Decl::Import { .. } | Decl::Extern { .. } => None,
```

Replace that line with a Const arm + the existing None arm:

```rust
        Decl::Const {
            name,
            ty,
            value,
            public,
            span,
        } => {
            let mut item = DocItem::new(name, DocKind::Function); // reuse Function kind for now
            item.public = *public;
            item.doc_comment = extract_preceding_doc_comment(source, span.start);

            let vis = if *public { "pub " } else { "" };
            let ty_str = ty
                .as_ref()
                .map(|t| format!(": {}", render_type_expr(t)))
                .unwrap_or_default();
            item.signature = format!("{}const {}{} = ...", vis, name, ty_str);

            Some(item)
        }
        // Impl blocks, imports, and externs don't generate standalone doc items
        Decl::Impl { .. } | Decl::Import { .. } | Decl::Extern { .. } => None,
```

**Step 2: Check if DocKind has a Constant variant**

Search `kryos-doc/src/lib.rs` for the `DocKind` enum. If it doesn't have a `Constant` variant, add one. Then use `DocKind::Constant` instead of `DocKind::Function` in the arm above.

**Step 3: Verify compilation**

Run: `cd compiler && cargo build -p kryos-doc 2>&1`
Expected: SUCCESS (no errors)

**Step 4: Commit**

```bash
git add compiler/crates/kryos-doc/src/lib.rs
git commit -m "fix(doc): handle Decl::Const in documentation generator"
```

---

### Task 0.2: Handle Decl::Const in kryos-fmt

**Files:**
- Modify: `compiler/crates/kryos-fmt/src/formatter.rs:85-143`

**Step 1: Add Const arm to fmt_decl**

In the match at line 86, add before the closing brace (after the `Decl::Extern` arm at line 142):

```rust
            Decl::Const {
                name,
                ty,
                value,
                public,
                ..
            } => self.fmt_const(name, ty, value, *public),
```

**Step 2: Implement fmt_const method**

Add this method to the `Formatter` impl block (after `fmt_extern` or similar):

```rust
    fn fmt_const(&mut self, name: &str, ty: &Option<TypeExpr>, value: &Expr, public: bool) {
        self.write_indent();
        if public {
            self.write("pub ");
        }
        self.write("const ");
        self.write(name);
        if let Some(ty_expr) = ty {
            self.write(": ");
            self.fmt_type_expr(ty_expr);
        }
        self.write(" = ");
        self.fmt_expr(value);
        self.newline();
    }
```

**Step 3: Verify compilation**

Run: `cd compiler && cargo build -p kryos-fmt 2>&1`
Expected: SUCCESS

**Step 4: Commit**

```bash
git add compiler/crates/kryos-fmt/src/formatter.rs
git commit -m "fix(fmt): handle Decl::Const in code formatter"
```

---

### Task 0.3: Handle Decl::Const in MIR lowering

**Files:**
- Modify: `compiler/crates/kryos-mir/src/lower.rs` (pre-pass at ~line 299, second pass at ~line 456)

**Step 1: Add a const_values map to LoweringContext**

Search for the `LoweringContext` struct definition. Add a field:
```rust
    /// Top-level constant values: name -> (MirType, constant RValue).
    pub const_values: HashMap<String, (MirType, i64)>,
```

Initialize it in the constructor with `const_values: HashMap::new()`.

**Step 2: Register constants in the pre-pass**

In the first loop at ~line 299 (where Struct, Enum, Function etc. are collected), add before the `_ => {}` wildcard at line 450:

```rust
            ast::Decl::Const { name, ty, value, .. } => {
                // For now, handle integer and float constant literals.
                // Full constant evaluation can use consteval later.
                let mir_ty = ty
                    .as_ref()
                    .map(|t| lower_type_expr(t))
                    .unwrap_or(MirType::I64);
                // Store in context for use during expression lowering.
                ctx.const_values.insert(name.clone(), (mir_ty, 0));
            }
```

**Step 3: Handle const references during expression lowering**

When an identifier is resolved during expression lowering, check `const_values` as a fallback. Search for the identifier resolution logic (where `Expr::Ident` is lowered) and add a fallback that checks `ctx.const_values`.

**Step 4: Verify full build**

Run: `cd compiler && cargo build 2>&1`
Expected: SUCCESS (all 21 crates compile)

**Step 5: Run test suite**

Run: `cd compiler && cargo test 2>&1 | tail -20`
Expected: All existing tests pass

**Step 6: Commit**

```bash
git add compiler/crates/kryos-mir/src/lower.rs
git commit -m "feat(mir): lower Decl::Const declarations"
```

---

### Task 0.4: Add const declaration tests

**Files:**
- Create: `compiler/crates/kryos-test-runner/tests/e2e/basics/const_decl.kry`
- Create: `compiler/crates/kryos-test-runner/tests/native/const_basic.kry`

**Step 1: Write E2E test**

```kry
// Test: top-level const declarations
const MAX_SIZE: i64 = 100
const PI: f64 = 3.14159

fn main() {
    let x = MAX_SIZE
    println("const works")
}
```

**Step 2: Write native test**

```kry
// expect-exit: 42
const ANSWER: i64 = 42

fn main() -> i64 {
    return ANSWER
}
```

**Step 3: Run tests**

Run: `cd compiler && cargo test 2>&1 | tail -30`
Expected: New tests pass

**Step 4: Commit**

```bash
git add compiler/crates/kryos-test-runner/tests/e2e/basics/const_decl.kry
git add compiler/crates/kryos-test-runner/tests/native/const_basic.kry
git commit -m "test: add const declaration E2E and native tests"
```

---

## RING 1 — Correctness

### Task 1.1: Verify and fix LLVM mutable variable codegen

**Files:**
- Modify (if needed): `compiler/crates/kryos-codegen-llvm/src/codegen.rs`
- Create: `compiler/crates/kryos-test-runner/tests/native/mutable_loop.kry`
- Create: `compiler/crates/kryos-test-runner/tests/native/mutable_nested.kry`

**Step 1: Write failing native test — mutable counter in loop**

```kry
// expect-exit: 10
fn main() -> i64 {
    let mut count = 0
    let mut i = 0
    while i < 10 {
        count = count + 1
        i = i + 1
    }
    return count
}
```

**Step 2: Write failing native test — nested mutation**

```kry
// expect-exit: 15
fn main() -> i64 {
    let mut result = 0
    let mut i = 1
    while i <= 5 {
        let mut j = 0
        while j < i {
            result = result + 1
            j = j + 1
        }
        i = i + 1
    }
    return result
}
```

**Step 3: Build and run with --release flag**

Run: `cd compiler && cargo test -p kryos-test-runner -- native 2>&1`

If tests fail on release (LLVM) but pass on debug (Cranelift), the SSA bug is confirmed. Investigate the LLVM IR output:
- Run `kryos build --emit-llvm <test_file>` and inspect the generated IR
- Look for: missing alloca for loop variables, SSA re-definitions without phi nodes, incorrect load/store pairing

**Step 4: Fix any issues found**

The existing alloca/store/load approach (lines 507-559 of codegen.rs) should handle this. Common issues:
- **Variables in inner scopes not detected as mutable**: The assign_counts scan may miss variables that are only assigned once per scope but the scope is inside a loop
- **Phi nodes at block boundaries**: When control flow merges, the load from alloca should provide the correct value without explicit phi nodes
- **Nested function parameters**: Ensure all mutable parameters get allocas

**Step 5: Verify all benchmarks pass on both backends**

Run the benchmark .kry files through both backends:
```bash
cd compiler && cargo run -- build benchmarks/fibonacci.kry -o /tmp/fib_debug
cd compiler && cargo run -- build benchmarks/fibonacci.kry --release -o /tmp/fib_release
```

**Step 6: Commit**

```bash
git add -A
git commit -m "fix(llvm): verify and harden mutable variable codegen in loops"
```

---

### Task 1.2: Fix and harden struct codegen

**Files:**
- Modify (if needed): `compiler/crates/kryos-codegen-cranelift/src/codegen.rs`
- Modify (if needed): `compiler/crates/kryos-codegen-llvm/src/codegen.rs`
- Create: `compiler/crates/kryos-test-runner/tests/native/struct_fields.kry`
- Create: `compiler/crates/kryos-test-runner/tests/native/struct_methods.kry`
- Create: `compiler/crates/kryos-test-runner/tests/native/struct_nested.kry`

**Step 1: Write struct field access test**

```kry
// expect-exit: 30
struct Point {
    x: i64
    y: i64
}

fn main() -> i64 {
    let p = Point { x: 10, y: 20 }
    return p.x + p.y
}
```

**Step 2: Write struct method test**

```kry
// expect-exit: 25
struct Rect {
    w: i64
    h: i64
}

impl Rect {
    fn area(self) -> i64 {
        return self.w * self.h
    }
}

fn main() -> i64 {
    let r = Rect { w: 5, h: 5 }
    return r.area()
}
```

**Step 3: Write nested struct test**

```kry
// expect-exit: 6
struct Inner {
    val: i64
}

struct Outer {
    a: Inner
    b: Inner
}

fn main() -> i64 {
    let inner1 = Inner { val: 2 }
    let inner2 = Inner { val: 4 }
    let o = Outer { a: inner1, b: inner2 }
    return o.a.val + o.b.val
}
```

**Step 4: Run tests on both backends**

```bash
cd compiler && cargo test -p kryos-test-runner -- native 2>&1
```

**Step 5: Debug any Cranelift verifier errors**

If struct tests fail with Cranelift verifier errors:
- The issue is likely in `compute_struct_layout` (line 134) or the store/load in `RValue::Struct`/`RValue::Field` handlers (lines 1553-1650)
- Check: field type mapping (`mir_type_to_cl`), offset calculation, `MemFlags` correctness
- Nested structs require the inner struct to be a pointer (I64), not inlined — verify this is handled

**Step 6: Debug any LLVM field access issues**

If LLVM struct tests fail:
- Check `resolve_field_index` (line 1428) — the fallback `return 0` is dangerous
- Check `emit_aggregate_struct` (line 1629) — verify `insertvalue` indices match field order
- Check field access uses `extractvalue` with correct index

**Step 7: Commit**

```bash
git add -A
git commit -m "fix(codegen): harden struct field access and methods on both backends"
```

---

### Task 1.3: Fix cross-module name resolution

**Files:**
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs` (import resolution section)
- Modify (if needed): `compiler/crates/kryos-driver/src/` (module resolver)
- Test: `compiler/tests/modules/main.kry` + `compiler/tests/modules/math.kry`

**Step 1: Run the failing module test**

```bash
cd compiler && cargo test -- modules 2>&1
```

Capture the exact error message. Expected: "undefined variable" or "unresolved symbol" for `add` in main.kry.

**Step 2: Trace the import resolution**

Read the import resolution code in pipeline.rs (around line 209). Understand how:
1. `use math` finds `math.kry` as a sibling file
2. The declarations from `math.kry` are merged into the main module
3. The merged declarations are available during type checking

The bug is likely: imported function declarations are merged AFTER type checking, or they're merged but not registered in the type checker's function signature map.

**Step 3: Fix the resolution order**

Ensure imported declarations are:
1. Resolved and parsed (already working — driver tests pass for this)
2. Merged into the main module's declaration list BEFORE type checking
3. Available in the MIR lowering context's `func_ret_types` map

**Step 4: Verify the modules test passes**

```bash
cd compiler && cargo test -- modules 2>&1
```
Expected: PASS

**Step 5: Add more module tests**

Create `compiler/crates/kryos-test-runner/tests/e2e/modules/` with:
- `import_basic.kry` + `import_basic_lib.kry` — basic function import
- `import_selective.kry` — `use math::{add}` selective import
- `import_multi.kry` — importing from multiple modules

**Step 6: Commit**

```bash
git add -A
git commit -m "fix(driver): cross-module name resolution for imported functions"
```

---

### Task 1.4: End-to-end correctness verification

**Files:**
- No new files — verification task

**Step 1: Run full test suite**

```bash
cd compiler && cargo test 2>&1
```
Expected: ALL tests pass (0 failures)

**Step 2: Run benchmarks on both backends**

```bash
cd compiler && cargo run -- run benchmarks/fibonacci.kry
cd compiler && cargo run -- run benchmarks/matrix.kry
cd compiler && cargo run -- run benchmarks/strings.kry
cd compiler && cargo run -- run benchmarks/sort.kry
```
Expected: All produce correct output

**Step 3: Run examples**

```bash
cd compiler && cargo run -- run ../examples/demo.kry
cd compiler && cargo run -- run ../examples/kryos_bootstrap.kry
cd compiler && cargo run -- run ../examples/neural_net.kry
```
Expected: All run without crash

**Step 4: Commit verification note**

```bash
git commit --allow-empty -m "verify: Ring 1 complete — all tests pass, benchmarks and examples run"
```

---

## RING 2 — Completeness

### Task 2.1: Implement borrowing — ownership analyzer extension

**Files:**
- Modify: `compiler/crates/kryos-ownership/src/lib.rs`
- Modify: `compiler/crates/kryos-ownership/src/analysis.rs`
- Test: `compiler/crates/kryos-ownership/tests/ownership.rs`

**Key insight:** `Type::Reference { inner, mutable }` already exists in `kryos-types/src/ty.rs` (line 71-74). `MirType::Ref { inner, mutable }` already exists in `kryos-mir/src/ir.rs`. The type system is ready — we need the analysis and codegen.

**Step 1: Extend OwnershipState**

In `compiler/crates/kryos-ownership/src/lib.rs`, add a new state:

```rust
pub enum OwnershipState {
    Owned,
    Moved,
    Shared,
    PartiallyMoved,
    Uninitialized,
    /// Immutably borrowed — N active borrows.
    Borrowed { count: u32 },
    /// Mutably borrowed — exclusive access lent out.
    MutBorrowed,
}
```

**Step 2: Add borrow tracking to VarInfo**

In `analysis.rs`, add to `VarInfo`:
```rust
pub struct VarInfo {
    pub state: OwnershipState,
    pub mutable: bool,
    pub is_copy: bool,
    pub decl_span: Span,
    pub moved_span: Option<Span>,
    pub moved_fields: HashSet<String>,
    /// Variables currently borrowing this one.
    pub borrowers: Vec<String>,
}
```

**Step 3: Implement borrow checking rules in analyze_expr_use**

When encountering `Expr::Ref { mutable, expr }` (or however `&x` is represented in the AST — check the Expr enum):
- If `mutable == false`: Set target to `Borrowed { count: count + 1 }`. Allowed if target is `Owned` or `Borrowed`.
- If `mutable == true`: Set target to `MutBorrowed`. Only allowed if target is `Owned` (no other borrows active).

When a variable is used after being borrowed:
- Immutable borrow: reads allowed, moves/mutation NOT allowed
- Mutable borrow: NO access allowed until borrow ends (scope exit)

**Step 4: Implement borrow release on scope exit**

When `pop_scope()` is called, any borrows created in that scope are released:
- Decrement borrow counts on borrowed variables
- Transition `Borrowed { count: 0 }` back to `Owned`
- Transition `MutBorrowed` back to `Owned`

**Step 5: Write ownership tests**

```rust
#[test]
fn immutable_borrow_allows_read() {
    let module = module_with_fn(vec![
        let_move("x", int_lit(42)),
        // let y = &x  (need to check AST representation)
        // let z = x   (read should work)
    ]);
    let result = analyze_ownership(&module);
    assert_eq!(result.errors.len(), 0);
}

#[test]
fn mutable_borrow_blocks_read() {
    // let x = 42
    // let y = &mut x
    // let z = x  // ERROR: x is mutably borrowed
    let result = analyze_ownership(&module);
    assert!(result.errors.iter().any(|e| e.message.contains("borrowed")));
}

#[test]
fn move_while_borrowed_errors() {
    // let x = SomeStruct { val: 1 }
    // let y = &x
    // consume(x)  // ERROR: cannot move x while borrowed
    let result = analyze_ownership(&module);
    assert!(result.errors.iter().any(|e| e.message.contains("move") || e.message.contains("borrow")));
}
```

**Step 6: Run tests**

```bash
cd compiler && cargo test -p kryos-ownership 2>&1
```

**Step 7: Commit**

```bash
git add compiler/crates/kryos-ownership/
git commit -m "feat(ownership): implement borrow checking for &T and &mut T"
```

---

### Task 2.2: Implement borrowing — MIR lowering for references

**Files:**
- Modify: `compiler/crates/kryos-mir/src/lower.rs`
- Test: `compiler/crates/kryos-mir/tests/mir.rs`

**Step 1: Check how &x is represented in the AST**

Search `kryos-ast/src/expr.rs` for "Ref", "Borrow", "Address", or "&". Find the exact AST node for reference expressions.

**Step 2: Lower reference expressions to MIR**

When encountering `&x`:
- Emit `Assign { dest: new_local(Ref(T, false)), value: RValue::Use(Operand::Local(x_id)) }`
- The reference is semantically a pointer to the original variable

When encountering `&mut x`:
- Same but with `MirType::Ref { inner, mutable: true }`

When encountering `*x` (dereference):
- Emit a load from the reference target

When encountering `x.field` where x is a reference:
- Auto-dereference: load the pointer, then access the field

**Step 3: Add MIR-level tests**

Test that `&x` lowers to the correct MIR instructions and that auto-deref works.

**Step 4: Run tests**

```bash
cd compiler && cargo test -p kryos-mir 2>&1
```

**Step 5: Commit**

```bash
git add compiler/crates/kryos-mir/
git commit -m "feat(mir): lower reference expressions (&T, &mut T) to MIR"
```

---

### Task 2.3: Implement borrowing — codegen for references

**Files:**
- Modify: `compiler/crates/kryos-codegen-cranelift/src/codegen.rs`
- Modify: `compiler/crates/kryos-codegen-llvm/src/codegen.rs`
- Create: `compiler/crates/kryos-test-runner/tests/native/borrow_basic.kry`
- Create: `compiler/crates/kryos-test-runner/tests/native/borrow_mut.kry`
- Create: `compiler/crates/kryos-test-runner/tests/native/borrow_function.kry`

**Step 1: Cranelift — reference codegen**

References are already pointers in the i64 slot model. Key operations:
- `&x`: Get the address of x's stack slot (`stack_addr`)
- `*x`: Load from the pointer (`load`)
- `&mut x`: Same as `&x` at codegen level (mutability enforced by ownership analyzer)

**Step 2: LLVM — reference codegen**

- `&x`: Use the `%_N.addr` alloca pointer directly (already exists for mutable locals)
- For immutable locals, may need to create an alloca
- `*x`: `load` from the pointer

**Step 3: Write native tests**

```kry
// borrow_basic.kry
// expect-exit: 42
fn read_ref(x: &i64) -> i64 {
    return *x
}

fn main() -> i64 {
    let val = 42
    return read_ref(&val)
}
```

```kry
// borrow_mut.kry
// expect-exit: 10
fn increment(x: &mut i64) {
    *x = *x + 1
}

fn main() -> i64 {
    let mut count = 0
    let mut i = 0
    while i < 10 {
        increment(&mut count)
        i = i + 1
    }
    return count
}
```

```kry
// borrow_function.kry
// expect-exit: 30
struct Point {
    x: i64
    y: i64
}

fn sum_point(p: &Point) -> i64 {
    return p.x + p.y
}

fn main() -> i64 {
    let p = Point { x: 10, y: 20 }
    return sum_point(&p)
}
```

**Step 4: Run native tests**

```bash
cd compiler && cargo test -p kryos-test-runner -- native 2>&1
```

**Step 5: Commit**

```bash
git add -A
git commit -m "feat(codegen): implement reference codegen on Cranelift and LLVM backends"
```

---

### Task 2.4: Wire comptime evaluation into pipeline

**Files:**
- Modify: `compiler/crates/kryos-mir/src/consteval.rs`
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`
- Create: `compiler/crates/kryos-test-runner/tests/native/comptime_basic.kry`
- Create: `compiler/crates/kryos-test-runner/tests/native/comptime_array.kry`

**Step 1: Verify comptime pass is called**

Read `pipeline.rs` line ~296 where `run_comptime_pass` is called. Verify it's actually invoked during normal compilation (not gated behind a flag).

**Step 2: Write comptime test**

```kry
// comptime_basic.kry
// expect-exit: 120
comptime {
    fn factorial(n: i64) -> i64 {
        if n <= 1 { return 1 }
        return n * factorial(n - 1)
    }
}

fn main() -> i64 {
    return factorial(5)
}
```

**Step 3: Write comptime constant test**

```kry
// comptime_array.kry
// expect-exit: 15
const SIZE: i64 = comptime { 3 * 5 }

fn main() -> i64 {
    return SIZE
}
```

**Step 4: Debug comptime evaluation path**

If tests fail, trace through:
1. How `comptime { expr }` is parsed (check kryos-parser)
2. How it's lowered in MIR (check lower.rs for comptime handling)
3. How `run_comptime_pass` in consteval.rs evaluates it
4. Whether the result replaces the comptime block with a constant

**Step 5: Run tests and commit**

```bash
cd compiler && cargo test -p kryos-test-runner -- native 2>&1
git add -A
git commit -m "feat(comptime): wire compile-time evaluation end-to-end"
```

---

### Task 2.5: Implement dynamic dispatch (dyn Trait)

**Files:**
- Modify: `compiler/crates/kryos-mir/src/ir.rs` (add vtable-related instructions)
- Modify: `compiler/crates/kryos-mir/src/lower.rs` (lower dyn Trait casts and calls)
- Modify: `compiler/crates/kryos-codegen-cranelift/src/codegen.rs` (vtable codegen)
- Modify: `compiler/crates/kryos-codegen-llvm/src/codegen.rs` (vtable codegen)
- Create: `compiler/crates/kryos-test-runner/tests/native/dyn_trait.kry`

**Step 1: Add vtable MIR instructions**

In `ir.rs`, add to the Instruction enum:
```rust
/// Create a trait object (fat pointer: data_ptr + vtable_ptr)
MakeTraitObject {
    dest: LocalId,
    data: Operand,        // pointer to concrete value
    vtable: String,       // vtable global name
},

/// Call through a trait object vtable
VtableCall {
    dest: Option<LocalId>,
    trait_obj: Operand,   // the fat pointer
    method_idx: u32,      // index into vtable
    args: Vec<Operand>,
},
```

**Step 2: Generate vtable layouts during MIR lowering**

When `impl Trait for Type` is encountered:
1. Create a vtable: array of function pointers, one per trait method (alphabetical order)
2. Store vtable definition in context
3. When a value is cast to `dyn Trait`, emit `MakeTraitObject`
4. When a method is called on `dyn Trait`, emit `VtableCall`

**Step 3: Codegen vtables**

Cranelift:
- Vtable is a heap-allocated array of function pointers
- `MakeTraitObject` creates a 2-slot value (data ptr, vtable ptr)
- `VtableCall` loads the function pointer from vtable[method_idx] and does an indirect call

LLVM:
- Vtable is a global constant struct of function pointers
- `MakeTraitObject` creates `insertvalue` of data and vtable pointers
- `VtableCall` uses `extractvalue` + `load` + indirect `call`

**Step 4: Write test**

```kry
// dyn_trait.kry
// expect-exit: 50
trait Shape {
    fn area(self) -> i64
}

struct Rect {
    w: i64
    h: i64
}

struct Square {
    side: i64
}

impl Shape for Rect {
    fn area(self) -> i64 {
        return self.w * self.h
    }
}

impl Shape for Square {
    fn area(self) -> i64 {
        return self.side * self.side
    }
}

fn print_area(s: dyn Shape) -> i64 {
    return s.area()
}

fn main() -> i64 {
    let r = Rect { w: 5, h: 6 }
    let s = Square { side: 4 }
    return print_area(r) + print_area(s)
}
```

**Step 5: Run tests and commit**

```bash
cd compiler && cargo test 2>&1
git add -A
git commit -m "feat: implement dynamic dispatch with vtables for dyn Trait"
```

---

### Task 2.6: Integrate constant folding pass into pipeline

**Files:**
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`
- Modify: `compiler/crates/kryos-mir/src/consteval.rs` (if needed)

**Step 1: Add a MIR optimization pass after comptime evaluation**

In pipeline.rs, after the `run_comptime_pass(&mut mir)` call (~line 296), add:

```rust
// Constant folding: replace constant expressions with their values.
kryos_mir::consteval::fold_constants(&mut mir);
```

**Step 2: Implement fold_constants if it doesn't exist**

This walks all functions, all blocks, all instructions. For each `Assign { value: RValue::BinOp { .. }, .. }` where both operands are constants, replace with the computed constant.

The existing `fold()` function in consteval.rs does exactly this — just need a wrapper that applies it across the whole module.

**Step 3: Gate behind release flag**

Only run optimization passes when building with `--release`:
```rust
if config.release {
    kryos_mir::consteval::fold_constants(&mut mir);
}
```

**Step 4: Run tests and commit**

```bash
cd compiler && cargo test 2>&1
git add -A
git commit -m "feat(pipeline): integrate constant folding pass for release builds"
```

---

### Task 2.7: Implement attribute enforcement

**Files:**
- Modify: `compiler/crates/kryos-capabilities/src/` (or create new pass)
- Create: `compiler/crates/kryos-test-runner/tests/e2e/attrs/`

**Step 1: @deprecated warning**

When a function annotated with `@deprecated` is called, emit a warning diagnostic. This is a simple AST-level check during type checking or a separate pass.

**Step 2: @test function marking**

Functions annotated with `@test` should be collected by the test runner. Verify `kryos test` discovers these.

**Step 3: @inline hint**

Store `@inline` annotation in MIR function metadata. Will be used by the inliner in Ring 3.

**Step 4: @pure verification**

Functions annotated with `@pure` must not:
- Call non-pure functions
- Access mutable state
- Perform I/O

Add a simple check: if `@pure` is present, scan function body for side-effecting operations.

**Step 5: Tests and commit**

```bash
cd compiler && cargo test 2>&1
git add -A
git commit -m "feat: enforce @deprecated, @test, @inline, @pure attributes"
```

---

## RING 3 — Performance

### Task 3.1: Dead code elimination pass

**Files:**
- Create: `compiler/crates/kryos-mir/src/optimize/mod.rs`
- Create: `compiler/crates/kryos-mir/src/optimize/dce.rs`
- Modify: `compiler/crates/kryos-mir/src/lib.rs` (add optimize module)
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`

**Implementation:**
1. Create `optimize/` module in kryos-mir
2. Implement `eliminate_dead_code(module: &mut MirModule)`:
   - Remove basic blocks with no predecessors (except entry)
   - Remove assignments whose LocalId is never read
   - Remove functions that are never called (entry point excluded)
3. Wire into pipeline after constant folding, gated behind `--release`

**Tests:** Write a program with dead branches after constant folding. Verify the optimized MIR is smaller (use `--emit-mir` flag to inspect).

---

### Task 3.2: Function inlining pass

**Files:**
- Create: `compiler/crates/kryos-mir/src/optimize/inline.rs`
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`

**Implementation:**
1. Implement `inline_functions(module: &mut MirModule, threshold: usize)`:
   - For each function call where the callee has fewer than `threshold` instructions (default 20)
   - And the callee is not recursive
   - And the callee is not marked `@noinline`
   - Replace the call with the inlined body (renaming locals to avoid conflicts)
2. Respect `@inline` attribute as force-inline (ignore threshold)
3. Run BEFORE constant folding (inlining exposes more foldable expressions)

**Pipeline order:** inline -> fold -> DCE

---

### Task 3.3: Tail call optimization

**Files:**
- Create: `compiler/crates/kryos-mir/src/optimize/tco.rs`
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`

**Implementation:**
1. Detect tail-recursive functions: the last instruction before `Return` is a `Call` to self
2. Replace with: assign new argument values to parameter locals, `Goto` back to entry block
3. This turns O(n) stack usage into O(1)

**Test:** `fibonacci_tco.kry` using tail-recursive style should not stack overflow on large inputs.

---

### Task 3.4: Loop-invariant code motion

**Files:**
- Create: `compiler/crates/kryos-mir/src/optimize/licm.rs`

**Implementation:**
1. Build dominance tree for each function
2. Identify loop headers (blocks that dominate their own predecessors)
3. For each instruction in a loop body, check if all operands are defined outside the loop
4. If so, move the instruction to the loop preheader

---

### Task 3.5: Strength reduction

**Files:**
- Create: `compiler/crates/kryos-mir/src/optimize/strength.rs`

**Implementation:**
- Replace `x * 2` with `x << 1`
- Replace `x * power_of_2` with `x << log2(n)`
- Replace `x / power_of_2` with `x >> log2(n)` for unsigned
- Replace `x % power_of_2` with `x & (n-1)` for unsigned

---

### Task 3.6: Pipeline integration and benchmarking

**Files:**
- Modify: `compiler/crates/kryos-driver/src/pipeline.rs`

**Implementation:**
1. Add optimization pass ordering: inline -> fold -> DCE -> LICM -> strength reduction
2. Gate all passes behind `--release` flag
3. Add `--emit-mir-opt` flag to dump optimized MIR
4. Re-run all benchmarks, compare debug vs release vs Rust vs Go
5. Update PERF_REPORT.md and BENCHMARK_REPORT.md

```bash
git add -A
git commit -m "feat: complete optimization pass pipeline (inline, DCE, TCO, LICM, strength reduction)"
```

---

## RING 4 — Ecosystem

### Task 4.1: Formatter completeness

- Verify `kryos fmt` handles ALL 10 Decl types (Const added in Ring 0)
- Format all 28 stdlib modules as a smoke test: `for f in compiler/stdlib/*.kry; do cargo run -- fmt "$f"; done`
- Fix any formatting bugs that surface
- Verify `kryos fmt --check` returns non-zero when formatting is needed

### Task 4.2: Doc generator completeness

- Verify `kryos doc` generates documentation for all public items
- Generate docs for all stdlib modules
- Add `DocKind::Constant` if not done in Ring 0
- Output: markdown files with function signatures, struct fields, enum variants, constants

### Task 4.3: LSP verification

- Launch `kryos lsp` and verify JSON-RPC handshake
- Test: diagnostics on save, completion for local variables, hover for type signatures, go-to-def for functions
- Create VS Code extension manifest: `editors/vscode/package.json` + `editors/vscode/language-configuration.json`
- Include TextMate grammar for `.kry` syntax highlighting
- Add snippet support for common patterns (fn, struct, enum, impl, match)

### Task 4.4: Package manager verification

- Test `kryos pkg init myproject` creates valid `kryos.toml` + `src/main.kry`
- Test `kryos pkg add github:example/lib@^1.0.0` updates kryos.toml
- Test `kryos pkg lock` generates deterministic kryos.lock
- Test dependency resolution with version conflicts

### Task 4.5: Test runner verification

- Verify `kryos test` discovers `@test` functions
- Verify `kryos test --filter <name>` filters correctly
- Verify pass/fail output is clear and professional

### Task 4.6: REPL verification

- Verify `kryos repl` starts, accepts expressions, shows results
- Test: let bindings, function definitions, expression evaluation
- Add type display for evaluated expressions

### Task 4.7: Error message audit

- Review all `Diagnostic` messages in the compiler
- Every error should: name what went wrong, show source location with span, suggest a fix when possible
- Follow Rust's error message quality bar
- Add color output (ANSI codes) for error messages in terminal

```bash
git add -A
git commit -m "feat: complete ecosystem toolchain — formatter, docs, LSP, pkg manager, test runner, REPL"
```

---

## RING 5 — Showcase

### Task 5.1: Demo programs

Create 5 production-quality demo programs in `examples/`:

1. **examples/http_server.kry** — Simple HTTP server with routing
2. **examples/mini_grep.kry** — File search CLI tool
3. **examples/pipeline.kry** — Concurrent data processing with channels
4. **examples/neural_net.kry** — Already exists, enhance with borrowing and comptime
5. **examples/web_scraper.kry** — Concurrent web scraper with actors

Each demo should:
- Compile and run without errors on both backends
- Demonstrate 3+ language features
- Include comments explaining what's happening
- Be under 200 lines

### Task 5.2: Benchmark suite

- Re-run fibonacci, matrix, sort, strings benchmarks with optimization passes
- Add new benchmarks: binary_trees.kry, nbody.kry, spectral_norm.kry
- Generate comparison table: Kryos debug / Kryos release / Rust debug / Rust release / Go
- Update BENCHMARK_REPORT.md and PERF_REPORT.md

### Task 5.3: "Why Kryos?" positioning document

Create `docs/WHY_KRYOS.md`:
- Clear positioning vs Rust (faster compilation, simpler borrowing, AI-native), Go (memory safety, no GC, capability security), Zig (higher-level abstractions, richer type system), Carbon (shipping now, not vaporware)
- Honest about limitations (v0.1.0, ecosystem is young)
- Target audiences: AI/ML engineers, systems programmers, security-conscious teams

### Task 5.4: Getting Started guide

Update `docs/01-getting-started.md`:
- 5-minute path: clone -> build compiler -> hello world -> first struct -> first test -> first build
- No assumed knowledge beyond "you've programmed before"
- Working code at every step (copy-paste friendly)

### Task 5.5: Investor-grade README

Rewrite `README.md`:
- Architecture diagram (ASCII art)
- Key metrics: 40k+ lines Rust, 21 crates, 28 stdlib modules, N tests passing, benchmark results
- Clear roadmap: completed / in progress / planned
- "Try it in 2 minutes" section
- Professional tone, no hype, let the engineering speak

### Task 5.6: GitHub Actions CI

Create `.github/workflows/ci.yml`:
- On every push: `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt -- --check`
- On release tags: build binaries for Windows, macOS, Linux
- Cache cargo registry and build artifacts

### Task 5.7: VS Code extension packaging

Create `editors/vscode/`:
- `package.json` — Extension manifest
- `syntaxes/kryos.tmLanguage.json` — TextMate grammar
- `language-configuration.json` — Comment style, brackets, auto-closing
- `snippets/kryos.json` — Code snippets
- README with install instructions

```bash
git add -A
git commit -m "feat: complete showcase — demos, benchmarks, docs, CI, VS Code extension"
```

---

## Execution Order Summary

| Task | Ring | Depends On | Est. Complexity |
|------|------|-----------|-----------------|
| 0.1 | Build | None | Low |
| 0.2 | Build | None | Low |
| 0.3 | Build | None | Medium |
| 0.4 | Build | 0.1-0.3 | Low |
| 1.1 | Correctness | Ring 0 | Medium |
| 1.2 | Correctness | Ring 0 | Medium |
| 1.3 | Correctness | Ring 0 | Medium |
| 1.4 | Correctness | 1.1-1.3 | Low |
| 2.1 | Completeness | Ring 1 | High |
| 2.2 | Completeness | 2.1 | High |
| 2.3 | Completeness | 2.2 | High |
| 2.4 | Completeness | Ring 1 | Medium |
| 2.5 | Completeness | Ring 1 | High |
| 2.6 | Completeness | Ring 1 | Low |
| 2.7 | Completeness | Ring 1 | Medium |
| 3.1-3.6 | Performance | Ring 2 | Medium each |
| 4.1-4.7 | Ecosystem | Ring 0 | Medium each |
| 5.1-5.7 | Showcase | Rings 1-4 | Medium each |

**Parallelization:** Tasks within the same ring can often be parallelized. Specifically:
- Ring 0: Tasks 0.1, 0.2, 0.3 are independent
- Ring 1: Tasks 1.1, 1.2, 1.3 are independent
- Ring 2: Tasks 2.4, 2.5, 2.6, 2.7 are independent of 2.1-2.3
- Ring 3 and Ring 4 are independent of each other
