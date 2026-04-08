# Kryos Self-Hosted Compiler — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a fully self-hosted Kryos compiler with native x86_64 code generation, its own linker, and zero external dependencies beyond the OS kernel.

**Architecture:** Extend the existing self-hosted frontend (lexer/parser/AST at 4,819 lines in `compiler/self-host/`) with a type checker, MIR lowering, optimizer, register allocator, x86_64 instruction encoder, ELF/COFF object file emitter, and static linker — all written in Kryos. The runtime is rewritten in Kryos using direct OS syscalls. Bootstrap chain: Rust compiler builds stage-0 once, then Kryos compiles itself forever.

**Tech Stack:** Kryos (self-hosted compiler), x86_64 machine code (native backend), ELF/COFF (object format), OS syscalls (runtime)

**MEMORY WARNING:** Debug builds of the Rust compiler consume ~48GB RAM. ALWAYS use `cargo build --release -j 4` and `cargo test --release -j 4`. Never bare `cargo build` or `cargo test`.

**Design Doc:** `docs/plans/2026-04-07-self-hosting-design.md`

---

## Phase 0: Language Gaps — Unblock Self-Hosting

Before writing the self-hosted compiler, the Rust-backed compiler needs these capabilities added so that the self-hosted code can use them.

### Task 0.1: Add byte buffer builtins to the runtime

**Files:**
- Modify: `crates/kryos-rt/src/builtins.rs` (add new extern "C" functions)
- Modify: `crates/kryos-codegen-cranelift/src/jit.rs` (register symbols in JIT)
- Modify: `crates/kryos-mir/src/lower.rs` (register return types)
- Test: `crates/kryos-test-runner/tests/e2e/builtins/`

**Context:** The self-hosted compiler must emit raw bytes (machine code, ELF headers, relocations). Currently there is no byte-level write capability. We need:

- `kryos_buf_new(capacity: i64) -> i64` — allocate a byte buffer, return handle
- `kryos_buf_write_byte(buf: i64, byte: i64)` — append one byte
- `kryos_buf_write_i16_le(buf: i64, val: i64)` — append 2 bytes little-endian
- `kryos_buf_write_i32_le(buf: i64, val: i64)` — append 4 bytes little-endian
- `kryos_buf_write_i64_le(buf: i64, val: i64)` — append 8 bytes little-endian
- `kryos_buf_write_bytes(buf: i64, src: i64, len: i64)` — append raw bytes from another buffer
- `kryos_buf_write_str(buf: i64, s: i64)` — append string bytes (no null terminator)
- `kryos_buf_write_zeros(buf: i64, count: i64)` — append N zero bytes (for padding/alignment)
- `kryos_buf_len(buf: i64) -> i64` — current length
- `kryos_buf_get_byte(buf: i64, offset: i64) -> i64` — read byte at offset
- `kryos_buf_set_byte(buf: i64, offset: i64, byte: i64)` — overwrite byte at offset
- `kryos_buf_patch_i32_le(buf: i64, offset: i64, val: i64)` — overwrite 4 bytes at offset (for backpatching jumps/relocations)
- `kryos_buf_patch_i64_le(buf: i64, offset: i64, val: i64)` — overwrite 8 bytes at offset
- `kryos_buf_write_to_file(buf: i64, path: i64) -> i64` — write entire buffer to file as raw bytes
- `kryos_buf_free(buf: i64)` — deallocate

**Step 1:** Implement `KryosBuf` struct in builtins.rs:

```rust
struct KryosBuf {
    data: Vec<u8>,
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_new(capacity: i64) -> i64 {
    let buf = Box::new(KryosBuf {
        data: Vec::with_capacity(capacity as usize),
    });
    Box::into_raw(buf) as i64
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_byte(buf: i64, byte: i64) {
    let buf = &mut *(buf as *mut KryosBuf);
    buf.data.push(byte as u8);
}

// ... etc for all functions
```

**Step 2:** Register all 15 functions in `jit.rs` symbol table and `lower.rs` return type map.

**Step 3:** Write e2e test:

```kryos
// crates/kryos-test-runner/tests/e2e/builtins/buf_basic_run.kry
// run-expect: ok
fn main() {
    let buf = buf_new(64)
    buf_write_byte(buf, 0x48)  // REX.W prefix
    buf_write_byte(buf, 0x89)  // MOV r/m64, r64
    buf_write_byte(buf, 0xC0)  // ModR/M: RAX <- RAX
    assert(buf_len(buf) == 3, "expected 3 bytes")
    assert(buf_get_byte(buf, 0) == 0x48, "expected REX.W")
    buf_free(buf)
    println("ok")
}
```

**Step 4:** Run: `cargo test --release -p kryos-test-runner -j 4 -- buf_basic`

**Step 5:** Commit: `feat(rt): add byte buffer builtins for native code emission`

---

### Task 0.2: Add exit() builtin

**Files:**
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-codegen-cranelift/src/jit.rs`
- Modify: `crates/kryos-mir/src/lower.rs`
- Test: `crates/kryos-test-runner/tests/e2e/builtins/`

**Step 1:** Add to builtins.rs:

```rust
#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_exit(code: i64) {
    std::process::exit(code as i32);
}
```

**Step 2:** Register in jit.rs and lower.rs (return type Void).

**Step 3:** Write e2e test:

```kryos
// exit_zero_run.kry
// run-expect: before exit
fn main() {
    println("before exit")
    exit(0)
    println("SHOULD NOT PRINT")
}
```

**Step 4:** Commit: `feat(rt): add exit() builtin`

---

### Task 0.3: Add map_keys(), map_has(), map_delete() to runtime

**Files:**
- Modify: `crates/kryos-rt/src/map.rs`
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-codegen-cranelift/src/jit.rs`
- Modify: `crates/kryos-mir/src/lower.rs`
- Test: `crates/kryos-test-runner/tests/e2e/builtins/`

**Context:** The self-hosted compiler needs hash maps for symbol tables, type registries, struct definitions, etc. Currently map create/insert/get work but keys/has/delete do not.

**Step 1:** Implement in map.rs/builtins.rs:

- `kryos_map_has(map: i64, key: i64) -> i64` — returns 1 if key exists, 0 otherwise
- `kryos_map_has_str(map: i64, key: i64) -> i64` — string key version
- `kryos_map_delete(map: i64, key: i64) -> i64` — remove key, return old value
- `kryos_map_delete_str(map: i64, key: i64) -> i64` — string key version
- `kryos_map_keys(map: i64) -> i64` — return KryosArray of keys
- `kryos_map_keys_str(map: i64) -> i64` — return KryosArray of string keys

**Step 2:** Register all 6 in jit.rs and lower.rs.

**Step 3:** Write e2e tests for each operation.

**Step 4:** Commit: `feat(rt): add map_keys, map_has, map_delete builtins`

---

### Task 0.4: Add args() builtin for CLI argument parsing

**Files:**
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-codegen-cranelift/src/jit.rs`
- Modify: `crates/kryos-mir/src/lower.rs`
- Test: `crates/kryos-test-runner/tests/e2e/builtins/`

**Context:** The self-hosted compiler needs to accept command-line arguments (input file path, flags). The main.kry already uses `std.process.args()` but this may not be wired through codegen.

**Step 1:** Implement:

```rust
#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_args() -> i64 {
    let args: Vec<String> = std::env::args().collect();
    let arr = kryos_array_new(8, args.len() as i64);
    for arg in args {
        let s = kryos_string_new(arg.as_ptr(), arg.len() as i64);
        kryos_array_push(arr as *mut KryosArray, s as i64);
    }
    arr as i64
}
```

**Step 2:** Register in jit.rs (return type Array), lower.rs.

**Step 3:** Test: simple program that prints its own arguments.

**Step 4:** Commit: `feat(rt): add args() builtin for CLI argument access`

---

## Phase 1: Type Checker in Kryos

### Task 1.1: Type checker data structures and symbol table

**Files:**
- Create: `compiler/self-host/types.kry`

**Context:** The type checker needs a symbol table (nested scopes), type representations, and error accumulation. Since Kryos doesn't have working generic enums through the type checker, we use the same integer-constant pattern as token.kry and ast.kry.

**Step 1:** Create `types.kry` with:

```kryos
// Type representation (mirrors MirType)
let TY_I8 = 1
let TY_I16 = 2
let TY_I32 = 3
let TY_I64 = 4
let TY_I128 = 5
let TY_U8 = 6
let TY_U16 = 7
let TY_U32 = 8
let TY_U64 = 9
let TY_U128 = 10
let TY_F32 = 11
let TY_F64 = 12
let TY_BOOL = 13
let TY_CHAR = 14
let TY_STR = 15
let TY_VOID = 16
let TY_PTR = 17
let TY_ARRAY = 18
let TY_STRUCT = 19
let TY_ENUM = 20
let TY_FUNCTION = 21
let TY_ANY = 22

struct TypeInfo {
    kind: i32
    name: str                  // struct/enum name, or "" for primitives
    element_type: [TypeInfo]   // for arrays, ptrs
    param_types: [TypeInfo]    // for functions
    ret_type: [TypeInfo]       // for functions
    field_names: [str]         // for structs
    field_types: [TypeInfo]    // for structs
    mutable: bool              // for references
}

// Symbol in the symbol table
struct Symbol {
    name: str
    ty: TypeInfo
    mutable: bool
    defined: bool              // true if has a value, false if just declared
}

// Scope for nested symbol resolution
struct Scope {
    symbols: [Symbol]          // linear scan is fine for scope sizes
    parent_idx: i32            // index into TypeChecker.scopes, -1 for global
}

// Main type checker state
struct TypeChecker {
    scopes: [Scope]
    current_scope: i32
    struct_defs: [StructDef]
    enum_defs: [EnumDef]
    trait_defs: [TraitDef]
    fn_sigs: [FnSig]
    errors: [str]
    warnings: [str]
}

struct StructDef {
    name: str
    fields: [StructField]
    generics: [str]
}

struct EnumDef {
    name: str
    variants: [EnumVariantDef]
    generics: [str]
}

struct EnumVariantDef {
    name: str
    field_types: [TypeInfo]
}

struct TraitDef {
    name: str
    methods: [FnSig]
}

struct FnSig {
    name: str
    params: [TypeInfo]
    param_names: [str]
    ret_type: TypeInfo
    generics: [str]
}
```

**Step 2:** Implement scope management functions:

```kryos
fn tc_new() -> TypeChecker
fn tc_push_scope(tc: TypeChecker)
fn tc_pop_scope(tc: TypeChecker)
fn tc_define(tc: TypeChecker, name: str, ty: TypeInfo, mutable: bool)
fn tc_lookup(tc: TypeChecker, name: str) -> Symbol   // walks scope chain
fn tc_error(tc: TypeChecker, msg: str)
```

**Step 3:** Implement type construction helpers:

```kryos
fn ty_i64() -> TypeInfo
fn ty_f64() -> TypeInfo
fn ty_bool() -> TypeInfo
fn ty_str() -> TypeInfo
fn ty_void() -> TypeInfo
fn ty_array(elem: TypeInfo) -> TypeInfo
fn ty_function(params: [TypeInfo], ret: TypeInfo) -> TypeInfo
fn ty_struct(name: str) -> TypeInfo
fn ty_equals(a: TypeInfo, b: TypeInfo) -> bool
fn ty_name(t: TypeInfo) -> str  // for error messages
```

**Step 4:** Verify compiles: `cargo run --release -j 4 -- check ../self-host/types.kry`

**Step 5:** Commit: `feat(self-host): add type checker data structures and symbol table`

---

### Task 1.2: Type checker — expression type inference

**Files:**
- Modify: `compiler/self-host/types.kry`

**Context:** This is the core of the type checker — given an expression AST node, determine its type.

**Step 1:** Implement `tc_check_expr(tc: TypeChecker, e: Expr) -> TypeInfo`:

- `EXPR_INT_LIT` → `ty_i64()`
- `EXPR_FLOAT_LIT` → `ty_f64()`
- `EXPR_STRING_LIT` → `ty_str()`
- `EXPR_BOOL_LIT` → `ty_bool()`
- `EXPR_NONE_LIT` → `ty_void()`
- `EXPR_IDENT` → lookup in symbol table, error if not found
- `EXPR_BINARY_OP` → check both sides, verify compatible types, return result type
- `EXPR_UNARY_OP` → check operand, verify valid operation
- `EXPR_FN_CALL` → look up function signature, check arg count/types, return ret_type
- `EXPR_METHOD_CALL` → resolve receiver type, find method in impl blocks
- `EXPR_FIELD_ACCESS` → resolve receiver as struct, find field type
- `EXPR_INDEX_ACCESS` → verify receiver is array, return element type
- `EXPR_ARRAY_LIT` → check all elements have same type, return array of that type
- `EXPR_STRUCT_LIT` → verify struct exists, check field names/types match
- `EXPR_LAMBDA` → infer param types from context, check body, return function type
- `EXPR_IF` → check condition is bool, check branches return same type
- `EXPR_MATCH` → check subject, verify pattern types, check all arms return same type
- `EXPR_CAST` → validate cast is legal (numeric to numeric, etc.)

**Step 2:** Implement binary op type rules:

```kryos
fn tc_check_binop(tc: TypeChecker, op: i32, left: TypeInfo, right: TypeInfo) -> TypeInfo
// Arithmetic (+, -, *, /, %, **): both numeric, same type, result = same type
// Comparison (==, !=, <, >, <=, >=): both same type, result = bool
// Logical (and, or): both bool, result = bool
// Bitwise (&, |, ^, <<, >>): both integer, result = left type
// String concat (+): if either is str, result = str
```

**Step 3:** Test with a small Kryos program that exercises each expression type.

**Step 4:** Commit: `feat(self-host): implement expression type inference`

---

### Task 1.3: Type checker — statements and declarations

**Files:**
- Modify: `compiler/self-host/types.kry`

**Context:** Walk declarations and statements, register types, validate bodies.

**Step 1:** Implement `tc_check_decl(tc: TypeChecker, d: Decl)`:

- `DECL_FUNCTION` → register signature, push scope, define params, check body, pop scope
- `DECL_STRUCT` → register struct definition with field types
- `DECL_ENUM` → register enum definition with variant types
- `DECL_TRAIT` → register trait with method signatures
- `DECL_IMPL` → verify target struct/enum exists, check each method
- `DECL_TYPE_ALIAS` → register alias
- `DECL_IMPORT` → resolve module, import symbols
- `DECL_EXTERN` → register extern function signatures

**Step 2:** Implement `tc_check_stmt(tc: TypeChecker, s: Stmt)`:

- `STMT_LET` → infer type from value (or use annotation), define symbol
- `STMT_ASSIGN` → check target is mutable, check value type matches
- `STMT_RETURN` → check value matches function return type
- `STMT_IF` → check condition is bool, check branches
- `STMT_FOR` → check iterable, define loop variable, check body
- `STMT_WHILE` → check condition is bool, check body
- `STMT_TRY_CATCH` → check try body, define catch variable, check catch body
- `STMT_THROW` → check expression is str
- `STMT_EXPR` → check the expression

**Step 3:** Implement `tc_check_module(tc: TypeChecker, module: Module)`:

```kryos
fn tc_check_module(tc: TypeChecker, module: Module) {
    // Pass 1: register all declarations (struct defs, fn sigs, etc.)
    for i in range(0, len(module.declarations)) {
        tc_register_decl(tc, module.declarations[i])
    }
    // Pass 2: type-check all bodies
    for i in range(0, len(module.declarations)) {
        tc_check_decl(tc, module.declarations[i])
    }
}
```

**Step 4:** Test: run type checker on demo.kry through the self-hosted pipeline, verify no false errors.

**Step 5:** Commit: `feat(self-host): implement statement and declaration type checking`

---

### Task 1.4: Type checker integration into main.kry

**Files:**
- Modify: `compiler/self-host/main.kry`

**Step 1:** Add Phase 3 to main.kry after parsing:

```kryos
// Phase 3: Type checking
println("--- Type Checking ---")
let tc = tc_new()
tc_register_builtins(tc)  // register println, to_string, len, push, etc.
tc_check_module(tc, module)
if len(tc.errors) > 0 {
    println("Type errors: " + to_string(len(tc.errors)))
    for i in range(0, len(tc.errors)) {
        println("  error: " + tc.errors[i])
    }
    exit(1)
}
println("Type check passed")
```

**Step 2:** Implement `tc_register_builtins(tc)` — register all 83+ runtime function signatures.

**Step 3:** Test: `cargo run --release -j 4 -- run ../self-host/main.kry ../examples/demo.kry`

Expected: "Type check passed"

**Step 4:** Commit: `feat(self-host): integrate type checker into compiler pipeline`

---

## Phase 2: MIR Lowering in Kryos

### Task 2.1: MIR data structures

**Files:**
- Create: `compiler/self-host/mir.kry`

**Context:** Mirror the Rust MIR format using integer constants and flat structs (same pattern as ast.kry).

**Step 1:** Define MIR types, instructions, terminators as constants + structs:

```kryos
// MIR instruction kinds
let INST_ASSIGN = 1
let INST_DROP = 2
let INST_NOP = 3
let INST_STORE_DEREF = 4
let INST_SPAWN = 5
let INST_SEND = 6
let INST_RECEIVE = 7

// RValue kinds (28 variants)
let RV_USE = 1
let RV_BINOP = 2
let RV_UNOP = 3
let RV_CALL = 4
let RV_CALL_INDIRECT = 5
let RV_CONST_INT = 6
let RV_CONST_FLOAT = 7
let RV_CONST_BOOL = 8
let RV_CONST_STRING = 9
let RV_CONST_NONE = 10
let RV_ARRAY = 11
let RV_STRUCT = 12
let RV_FIELD = 13
let RV_INDEX = 14
let RV_CAST = 15
let RV_ENUM_VARIANT = 16
let RV_ENUM_TAG = 17
let RV_ENUM_PAYLOAD = 18
let RV_CLOSURE = 19
let RV_MAP = 20
let RV_STRING_CONCAT = 21
let RV_RANGE = 22
let RV_ADDR_OF = 23
let RV_DEREF = 24
let RV_COMPTIME = 25

// Terminator kinds
let TERM_RETURN = 1
let TERM_GOTO = 2
let TERM_BRANCH = 3
let TERM_SWITCH = 4
let TERM_UNREACHABLE = 5

// Structs
struct MirLocal { id: i32, name: str, ty: TypeInfo, mutable: bool }
struct MirParam { local_id: i32, ty: TypeInfo }
struct Operand { is_local: bool, local_id: i32, const_kind: i32, int_val: i64, float_val: f64, bool_val: bool, str_val: str }
struct RValue { kind: i32, op: i32, left: Operand, right: Operand, func_name: str, args: [Operand], field_name: str, object: Operand, index: Operand, elements: [Operand], struct_name: str, field_names: [str], field_operands: [Operand], cast_ty: TypeInfo, enum_name: str, variant_idx: i32, field_idx: i32, captures: [Operand], int_val: i64, float_val: f64, bool_val: bool, str_val: str }
struct Instruction { kind: i32, dest: i32, value: RValue, ptr: Operand, store_val: Operand }
struct Terminator { kind: i32, ret_val: Operand, has_ret: bool, goto_block: i32, cond: Operand, then_block: i32, else_block: i32, switch_val: Operand, targets: [i64], target_blocks: [i32], default_block: i32 }
struct BasicBlock { id: i32, instructions: [Instruction], terminator: Terminator }
struct MirFunction { name: str, params: [MirParam], ret_ty: TypeInfo, blocks: [BasicBlock], locals: [MirLocal], next_local: i32, next_block: i32 }
struct MirModule { functions: [MirFunction], struct_defs: [StructDef], enum_defs: [EnumDef] }
```

**Step 2:** Implement helper constructors:

```kryos
fn operand_local(id: i32) -> Operand
fn operand_int(val: i64) -> Operand
fn operand_str(val: str) -> Operand
fn operand_bool(val: bool) -> Operand
fn rv_use(op: Operand) -> RValue
fn rv_const_int(val: i64) -> RValue
fn rv_binop(op: i32, left: Operand, right: Operand) -> RValue
fn rv_call(name: str, args: [Operand]) -> RValue
fn term_return(val: Operand) -> Terminator
fn term_goto(block: i32) -> Terminator
fn term_branch(cond: Operand, then_bb: i32, else_bb: i32) -> Terminator
```

**Step 3:** Verify compiles: `cargo run --release -j 4 -- check ../self-host/mir.kry`

**Step 4:** Commit: `feat(self-host): add MIR data structures`

---

### Task 2.2: MIR lowering — expressions and statements

**Files:**
- Create: `compiler/self-host/lower.kry`

**Context:** Transform typed AST into MIR basic blocks. This is the core of compilation.

**Step 1:** Implement lowering context:

```kryos
struct LowerCtx {
    func: MirFunction
    current_block: i32
    // For break/continue
    loop_break_block: i32
    loop_continue_block: i32
}

fn ctx_alloc_local(ctx: LowerCtx, name: str, ty: TypeInfo, mutable: bool) -> i32
fn ctx_alloc_temp(ctx: LowerCtx, ty: TypeInfo) -> i32
fn ctx_alloc_block(ctx: LowerCtx) -> i32
fn ctx_emit(ctx: LowerCtx, inst: Instruction)
fn ctx_finish_block(ctx: LowerCtx, term: Terminator)
```

**Step 2:** Implement `lower_expr(ctx: LowerCtx, e: Expr) -> Operand`:

Walks the expression tree, emitting instructions and returning the operand holding the result. Key cases:

- Literals → `RV_CONST_*`, store in temp
- Identifiers → `RV_USE(local_id)` from symbol lookup
- Binary ops → lower left, lower right, emit `INST_ASSIGN(temp, RV_BINOP(op, left, right))`
- Function calls → lower each arg, emit `INST_ASSIGN(temp, RV_CALL(name, args))`
- Field access → emit `INST_ASSIGN(temp, RV_FIELD(object, field_name))`
- If expression → allocate result temp, emit branches, lower both sides into the temp
- Match → emit switch/branch chain (same as Rust compiler's lower_match)

**Step 3:** Implement `lower_stmt(ctx: LowerCtx, s: Stmt)`:

- `STMT_LET` → alloc local, lower value, emit assign
- `STMT_ASSIGN` → lower target (get local id), lower value, emit assign
- `STMT_RETURN` → lower value, emit `TERM_RETURN`
- `STMT_IF` → alloc then/else/merge blocks, lower condition, emit `TERM_BRANCH`, lower each body
- `STMT_WHILE` → alloc cond/body/exit blocks, emit loop structure
- `STMT_FOR` → desugar to while loop over range/iterator
- `STMT_TRY_CATCH` → emit try block with landing pad, catch block
- `STMT_EXPR` → lower expression, discard result

**Step 4:** Implement `lower_function(decl: Decl, tc: TypeChecker) -> MirFunction`:

```kryos
fn lower_function(decl: Decl, tc: TypeChecker) -> MirFunction {
    let func = mir_function_new(decl.name)
    let ctx = lower_ctx_new(func)
    // Register parameters
    for i in range(0, len(decl.params)) {
        let ty = tc_resolve_type_expr(tc, decl.params[i].ty[0])
        ctx_alloc_local(ctx, decl.params[i].name, ty, false)
    }
    // Lower body
    for i in range(0, len(decl.fn_body)) {
        lower_stmt(ctx, decl.fn_body[i])
    }
    // Ensure terminal block has a return
    if !block_has_terminator(ctx) {
        ctx_finish_block(ctx, term_return_void())
    }
    return ctx.func
}
```

**Step 5:** Implement `lower_module(module: Module, tc: TypeChecker) -> MirModule`.

**Step 6:** Test: lower demo.kry, print MIR text representation, verify it looks sane.

**Step 7:** Commit: `feat(self-host): implement MIR lowering for expressions and statements`

---

### Task 2.3: MIR lowering — string operations, builtins, and special forms

**Files:**
- Modify: `compiler/self-host/lower.kry`

**Context:** Handle string concatenation (emit `kryos_string_concat` calls), builtin function calls (emit direct runtime calls), closures (capture analysis), and error handling (try/catch → landing pads).

**Step 1:** String concat: when `BINOP_ADD` has string operands, emit `RV_CALL("kryos_string_concat", [left, right])` instead of `RV_BINOP`.

**Step 2:** Builtin dispatch: maintain a map of builtin names → runtime function names:

```kryos
fn is_builtin(name: str) -> bool
fn builtin_runtime_name(name: str) -> str
// println → kryos_println_str
// to_string → kryos_i64_to_string (or f64/bool variant based on arg type)
// len → kryos_builtin_len
// push → kryos_builtin_push
// pop → kryos_builtin_pop
// file_read → kryos_builtin_file_read
// ... etc
```

**Step 3:** Closures: analyze free variables, emit `RV_CLOSURE(func_name, captures)`.

**Step 4:** Test: lower fibonacci_showcase.kry and http_server.kry, verify MIR output.

**Step 5:** Commit: `feat(self-host): handle strings, builtins, closures in MIR lowering`

---

## Phase 3: MIR Optimizer

### Task 3.1: Optimization passes

**Files:**
- Create: `compiler/self-host/optimize.kry`

**Context:** Operate on MirModule, transform in-place. Same 5 passes as the Rust compiler.

**Step 1:** Implement each pass as a function `fn pass_name(module: MirModule)`:

1. **Constant folding** — if both operands of a BinOp are constants, replace with computed constant
2. **Dead code elimination** — remove blocks with no predecessors (except entry), remove unused assigns
3. **Function inlining** — for functions < 20 instructions, replace call with inlined body (copy locals, remap block IDs)
4. **Tail call optimization** — if last instruction before return is a self-call, replace with goto to entry block
5. **Strength reduction** — `x * 2` → `x << 1`, `x * 1` → `x`, `x + 0` → `x`, `x / 1` → `x`

**Step 2:** Implement `fn optimize(module: MirModule)` that runs all passes in order.

**Step 3:** Test: optimize a program with known constant expressions, verify they're folded.

**Step 4:** Commit: `feat(self-host): implement MIR optimization passes`

---

## Phase 4: Register Allocator

### Task 4.1: Liveness analysis

**Files:**
- Create: `compiler/self-host/regalloc.kry`

**Context:** Compute live ranges for each MIR local — which instructions it's alive across.

**Step 1:** Define data structures:

```kryos
struct LiveRange {
    local_id: i32
    start: i32     // instruction index (global, across all blocks)
    end_pos: i32   // last use instruction index
    reg: i32       // assigned register (-1 = unassigned)
    stack_slot: i32 // stack offset (-1 = in register)
    spilled: bool
}

// x86_64 register IDs
let REG_RAX = 0
let REG_RCX = 1
let REG_RDX = 2
let REG_RBX = 3
let REG_RSP = 4
let REG_RBP = 5
let REG_RSI = 6
let REG_RDI = 7
let REG_R8 = 8
let REG_R9 = 9
let REG_R10 = 10
let REG_R11 = 11
let REG_R12 = 12
let REG_R13 = 13
let REG_R14 = 14
let REG_R15 = 15

// Callee-saved (must preserve across calls): RBX, R12-R15, RBP
// Caller-saved (scratch): RAX, RCX, RDX, RSI, RDI, R8-R11
// Reserved: RSP (stack pointer), RBP (frame pointer)
// Available for allocation: RAX, RCX, RDX, RBX, RSI, RDI, R8-R15 (14 registers)
// After reserving RSP/RBP: 12 general-purpose registers available
```

**Step 2:** Implement liveness analysis:

```kryos
fn compute_live_ranges(func: MirFunction) -> [LiveRange]
// Walk instructions in reverse order
// For each use of a local: extend its live range start backward
// For each definition of a local: set its live range end
```

**Step 3:** Commit: `feat(self-host): implement liveness analysis for register allocation`

---

### Task 4.2: Linear scan register allocator

**Files:**
- Modify: `compiler/self-host/regalloc.kry`

**Context:** Assign physical registers to live ranges, spilling to stack when needed.

**Step 1:** Implement linear scan:

```kryos
fn allocate_registers(func: MirFunction) -> [LiveRange] {
    let ranges = compute_live_ranges(func)
    sort_by_start(ranges)
    let mut active: [LiveRange] = []   // currently in-register ranges
    let mut free_regs = [REG_RAX, REG_RCX, REG_RDX, REG_RBX, REG_RSI, REG_RDI,
                         REG_R8, REG_R9, REG_R10, REG_R11, REG_R12, REG_R13,
                         REG_R14, REG_R15]
    
    for i in range(0, len(ranges)) {
        let r = ranges[i]
        // Expire old ranges that ended before this one starts
        expire_old(active, free_regs, r.start)
        
        if len(free_regs) > 0 {
            // Assign a free register
            r.reg = pop(free_regs)
            push(active, r)
        } else {
            // Spill: pick the range that ends latest
            spill_at_interval(active, free_regs, r)
        }
    }
    return ranges
}
```

**Step 2:** Handle calling convention constraints:

- Function arguments: first 6 in RDI, RSI, RDX, RCX, R8, R9 (SysV) or RCX, RDX, R8, R9 (Windows)
- Return value: always in RAX
- Before function calls: save caller-saved registers that are live across the call
- Callee-saved registers: generate save/restore in function prologue/epilogue

**Step 3:** Test: allocate registers for a simple function, verify no conflicts.

**Step 4:** Commit: `feat(self-host): implement linear scan register allocator`

---

## Phase 5: x86_64 Instruction Encoder

### Task 5.1: Core instruction encoding

**Files:**
- Create: `compiler/self-host/x86.kry`

**Context:** Emit raw x86_64 machine code bytes into a buffer. Each function encodes one instruction class.

**Step 1:** Implement encoding helpers:

```kryos
fn rex(w: bool, r: i32, x: i32, b: i32) -> i32
// REX prefix: 0100WRXB
// W=1 for 64-bit operand, R=high bit of ModR/M reg, X=SIB index, B=ModR/M r/m or SIB base

fn modrm(mod_bits: i32, reg: i32, rm: i32) -> i32
// ModR/M byte: [mod:2][reg:3][r/m:3]
// mod=11 for reg-reg, mod=00 for [reg], mod=01 for [reg+disp8], mod=10 for [reg+disp32]

fn reg_encoding(reg_id: i32) -> i32
// Maps our REG_* constants to x86_64 encoding (lower 3 bits)
// RAX=0, RCX=1, RDX=2, RBX=3, RSP=4, RBP=5, RSI=6, RDI=7
// R8-R15 = 0-7 with REX.B set

fn needs_rex_b(reg_id: i32) -> bool
// True for R8-R15
```

**Step 2:** Implement core instruction emitters:

```kryos
// Data movement
fn emit_mov_reg_reg(buf: i64, dst: i32, src: i32)        // MOV r64, r64
fn emit_mov_reg_imm64(buf: i64, dst: i32, imm: i64)      // MOV r64, imm64 (movabs)
fn emit_mov_reg_imm32(buf: i64, dst: i32, imm: i32)      // MOV r32, imm32 (zero-extends)
fn emit_mov_reg_mem(buf: i64, dst: i32, base: i32, offset: i32)  // MOV r64, [base+offset]
fn emit_mov_mem_reg(buf: i64, base: i32, offset: i32, src: i32)  // MOV [base+offset], r64
fn emit_lea(buf: i64, dst: i32, base: i32, offset: i32)  // LEA r64, [base+offset]

// Arithmetic
fn emit_add_reg_reg(buf: i64, dst: i32, src: i32)        // ADD r64, r64
fn emit_add_reg_imm32(buf: i64, dst: i32, imm: i32)      // ADD r64, imm32
fn emit_sub_reg_reg(buf: i64, dst: i32, src: i32)        // SUB r64, r64
fn emit_sub_reg_imm32(buf: i64, dst: i32, imm: i32)      // SUB r64, imm32
fn emit_imul_reg_reg(buf: i64, dst: i32, src: i32)       // IMUL r64, r64
fn emit_idiv_reg(buf: i64, divisor: i32)                   // IDIV r64 (RDX:RAX / divisor)
fn emit_neg_reg(buf: i64, reg: i32)                        // NEG r64
fn emit_cqo(buf: i64)                                     // CQO (sign-extend RAX into RDX:RAX)

// Bitwise
fn emit_and_reg_reg(buf: i64, dst: i32, src: i32)
fn emit_or_reg_reg(buf: i64, dst: i32, src: i32)
fn emit_xor_reg_reg(buf: i64, dst: i32, src: i32)
fn emit_shl_reg_cl(buf: i64, dst: i32)                    // SHL r64, CL
fn emit_shr_reg_cl(buf: i64, dst: i32)                    // SHR r64, CL
fn emit_not_reg(buf: i64, reg: i32)                        // NOT r64

// Comparison and branches
fn emit_cmp_reg_reg(buf: i64, a: i32, b: i32)            // CMP r64, r64
fn emit_cmp_reg_imm32(buf: i64, reg: i32, imm: i32)      // CMP r64, imm32
fn emit_test_reg_reg(buf: i64, a: i32, b: i32)           // TEST r64, r64
fn emit_jmp_rel32(buf: i64, offset: i32)                   // JMP rel32
fn emit_jcc_rel32(buf: i64, cc: i32, offset: i32)        // Jcc rel32 (conditional)
// Condition codes: CC_E=0x84, CC_NE=0x85, CC_L=0x8C, CC_GE=0x8D, CC_LE=0x8E, CC_G=0x8F, CC_B=0x82, CC_A=0x87

// Stack and function call
fn emit_push_reg(buf: i64, reg: i32)                       // PUSH r64
fn emit_pop_reg(buf: i64, reg: i32)                        // POP r64
fn emit_call_rel32(buf: i64, offset: i32)                  // CALL rel32
fn emit_ret(buf: i64)                                      // RET
fn emit_nop(buf: i64)                                      // NOP

// SSE2 floating point
fn emit_movsd_xmm_xmm(buf: i64, dst: i32, src: i32)     // MOVSD xmm, xmm
fn emit_addsd(buf: i64, dst: i32, src: i32)               // ADDSD xmm, xmm
fn emit_subsd(buf: i64, dst: i32, src: i32)               // SUBSD xmm, xmm
fn emit_mulsd(buf: i64, dst: i32, src: i32)               // MULSD xmm, xmm
fn emit_divsd(buf: i64, dst: i32, src: i32)               // DIVSD xmm, xmm
fn emit_ucomisd(buf: i64, a: i32, b: i32)                 // UCOMISD xmm, xmm
fn emit_cvtsi2sd(buf: i64, dst_xmm: i32, src_gpr: i32)  // CVTSI2SD xmm, r64
fn emit_cvtsd2si(buf: i64, dst_gpr: i32, src_xmm: i32)  // CVTSD2SI r64, xmm
```

**Step 3:** Test each instruction by emitting known byte sequences and comparing:

```kryos
// Test: MOV RAX, RCX should be 48 89 C8
fn test_mov_rax_rcx() {
    let buf = buf_new(16)
    emit_mov_reg_reg(buf, REG_RAX, REG_RCX)
    assert(buf_len(buf) == 3, "expected 3 bytes")
    assert(buf_get_byte(buf, 0) == 0x48, "REX.W")
    assert(buf_get_byte(buf, 1) == 0x89, "MOV opcode")
    assert(buf_get_byte(buf, 2) == 0xC8, "ModR/M")
    buf_free(buf)
}
```

**Step 4:** Commit: `feat(self-host): implement x86_64 instruction encoder`

---

### Task 5.2: Code generation — MIR to machine code

**Files:**
- Create: `compiler/self-host/codegen.kry`

**Context:** Walk MIR functions, emit x86_64 machine code using the encoder and register allocation.

**Step 1:** Implement function codegen:

```kryos
struct CodegenCtx {
    buf: i64                    // byte buffer handle
    alloc: [LiveRange]          // register allocation
    func: MirFunction
    block_offsets: [i32]        // byte offset of each block start in buf
    fixups: [Fixup]             // forward jumps to backpatch
    relocations: [Relocation]   // external symbol references
}

struct Fixup {
    buf_offset: i32    // where the rel32 placeholder is
    target_block: i32  // which block it should jump to
}

struct Relocation {
    buf_offset: i32    // where the rel32 placeholder is
    symbol: str        // external symbol name
    kind: i32          // relocation type
}
```

**Step 2:** Implement `fn codegen_function(func: MirFunction, alloc: [LiveRange]) -> CodegenCtx`:

- Emit prologue: `push rbp; mov rbp, rsp; sub rsp, frame_size`
- For each basic block:
  - Record block offset
  - For each instruction: emit the corresponding x86_64
  - For the terminator: emit branch/jump/return
- Emit epilogue (for return): `add rsp, frame_size; pop rbp; ret`
- Backpatch forward jumps using block_offsets

**Step 3:** Implement instruction translation:

```kryos
fn codegen_instruction(ctx: CodegenCtx, inst: Instruction) {
    if inst.kind == INST_ASSIGN {
        codegen_rvalue(ctx, inst.dest, inst.value)
        return
    }
    // ... other instruction kinds
}

fn codegen_rvalue(ctx: CodegenCtx, dest: i32, rv: RValue) {
    if rv.kind == RV_CONST_INT {
        let reg = get_reg(ctx.alloc, dest)
        emit_mov_reg_imm64(ctx.buf, reg, rv.int_val)
        return
    }
    if rv.kind == RV_BINOP {
        let left_reg = get_reg(ctx.alloc, rv.left.local_id)
        let right_reg = get_reg(ctx.alloc, rv.right.local_id)
        let dest_reg = get_reg(ctx.alloc, dest)
        // Move left to dest if not already there
        if dest_reg != left_reg {
            emit_mov_reg_reg(ctx.buf, dest_reg, left_reg)
        }
        if rv.op == BINOP_ADD { emit_add_reg_reg(ctx.buf, dest_reg, right_reg) }
        if rv.op == BINOP_SUB { emit_sub_reg_reg(ctx.buf, dest_reg, right_reg) }
        if rv.op == BINOP_MUL { emit_imul_reg_reg(ctx.buf, dest_reg, right_reg) }
        // ... etc
        return
    }
    if rv.kind == RV_CALL {
        codegen_call(ctx, dest, rv.func_name, rv.args)
        return
    }
    // ... etc
}
```

**Step 4:** Implement `codegen_call` — move args to ABI registers, emit CALL with relocation, move result from RAX.

**Step 5:** Implement `codegen_terminator`:

```kryos
fn codegen_terminator(ctx: CodegenCtx, term: Terminator) {
    if term.kind == TERM_RETURN {
        if term.has_ret {
            let ret_reg = get_reg(ctx.alloc, term.ret_val.local_id)
            if ret_reg != REG_RAX {
                emit_mov_reg_reg(ctx.buf, REG_RAX, ret_reg)
            }
        }
        // Epilogue
        emit_mov_reg_reg(ctx.buf, REG_RSP, REG_RBP)
        emit_pop_reg(ctx.buf, REG_RBP)
        emit_ret(ctx.buf)
        return
    }
    if term.kind == TERM_GOTO {
        add_fixup(ctx, term.goto_block)
        emit_jmp_rel32(ctx.buf, 0)  // placeholder, backpatched later
        return
    }
    if term.kind == TERM_BRANCH {
        let cond_reg = get_reg(ctx.alloc, term.cond.local_id)
        emit_test_reg_reg(ctx.buf, cond_reg, cond_reg)
        add_fixup(ctx, term.then_block)
        emit_jcc_rel32(ctx.buf, CC_NE, 0)  // jump if true
        add_fixup(ctx, term.else_block)
        emit_jmp_rel32(ctx.buf, 0)  // fall through to else
        return
    }
}
```

**Step 6:** Test: compile a trivial function `fn add(a: i64, b: i64) -> i64 { return a + b }` to machine code, verify bytes.

**Step 7:** Commit: `feat(self-host): implement MIR-to-x86_64 code generation`

---

## Phase 6: Object File Emitter

### Task 6.1: ELF object file writer

**Files:**
- Create: `compiler/self-host/elf.kry`

**Context:** Write compiled machine code into ELF .o files (Linux). ELF is the simplest object format and best documented.

**Step 1:** Define ELF constants and header structures:

```kryos
// ELF magic and header constants
let ELFMAG = 0x7F  // followed by 'E', 'L', 'F'
let ELFCLASS64 = 2
let ELFDATA2LSB = 1
let ET_REL = 1      // relocatable object
let EM_X86_64 = 62
let SHT_NULL = 0
let SHT_PROGBITS = 1
let SHT_SYMTAB = 2
let SHT_STRTAB = 3
let SHT_RELA = 4
let SHF_ALLOC = 0x2
let SHF_EXECINSTR = 0x4
let SHF_WRITE = 0x1
let STB_GLOBAL = 1
let STT_FUNC = 2
let STT_OBJECT = 1
let R_X86_64_PC32 = 2
let R_X86_64_PLT32 = 4
let R_X86_64_64 = 1
let R_X86_64_32S = 11
```

**Step 2:** Implement ELF writer:

```kryos
fn write_elf_object(path: str, functions: [CodegenCtx], module: MirModule) {
    let buf = buf_new(65536)
    
    // 1. Build .text section (all function code concatenated)
    // 2. Build .data section (string constants, global data)
    // 3. Build .symtab (symbol table — one entry per function)
    // 4. Build .strtab (string table — function names)
    // 5. Build .rela.text (relocations — external function calls)
    // 6. Build .shstrtab (section name string table)
    // 7. Write ELF header (64 bytes)
    // 8. Write section data
    // 9. Write section headers
    
    buf_write_to_file(buf, path)
    buf_free(buf)
}
```

**Step 3:** Test: compile a simple program to .o, verify with `objdump -d output.o`.

**Step 4:** Commit: `feat(self-host): implement ELF object file emitter`

---

### Task 6.2: COFF object file writer (Windows)

**Files:**
- Create: `compiler/self-host/coff.kry`

**Context:** Same as ELF but for Windows COFF format. This is needed for the compiler to work on Windows.

**Step 1:** Implement COFF writer (similar structure to ELF but different header format, different relocation types).

**Step 2:** Test: compile to .obj, verify with `dumpbin /disasm output.obj`.

**Step 3:** Commit: `feat(self-host): implement COFF object file emitter`

---

## Phase 7: Linker

### Task 7.1: ELF static linker

**Files:**
- Create: `compiler/self-host/linker.kry`

**Context:** Read ELF .o files, resolve symbols, apply relocations, write ELF executable.

**Step 1:** Implement ELF reader:

```kryos
struct ElfObject {
    sections: [Section]
    symbols: [ElfSymbol]
    relocations: [ElfReloc]
}

struct Section {
    name: str
    data: i64        // byte buffer handle
    offset: i32      // offset in final executable
    size: i32
    flags: i32
}

struct ElfSymbol {
    name: str
    section: i32
    offset: i32
    size: i32
    binding: i32     // local/global
    sym_type: i32    // func/object
    resolved_addr: i64
}

struct ElfReloc {
    offset: i32
    symbol: str
    rel_type: i32
    addend: i64
}

fn read_elf_object(path: str) -> ElfObject
```

**Step 2:** Implement linker:

```kryos
fn link_executable(output_path: str, objects: [str]) {
    // 1. Read all object files
    // 2. Merge .text sections (concatenate, record offsets)
    // 3. Merge .data, .rodata, .bss sections
    // 4. Build global symbol table (resolve duplicates, detect undefined)
    // 5. Apply relocations (patch machine code with resolved addresses)
    // 6. Write ELF executable header
    // 7. Write program headers (PT_LOAD for text, data)
    // 8. Write section data at correct file offsets
    // 9. Set entry point to _start (which calls main)
    // 10. Mark as executable: file permissions
}
```

**Step 3:** Implement `_start` stub that calls `main` and invokes `exit` syscall:

```kryos
fn emit_start_stub(buf: i64) {
    // _start:
    //   call main
    //   mov rdi, rax    ; exit code = main return value
    //   mov rax, 60     ; SYS_exit
    //   syscall
    emit_call_rel32(buf, 0)    // placeholder, patched to main
    emit_mov_reg_reg(buf, REG_RDI, REG_RAX)
    emit_mov_reg_imm64(buf, REG_RAX, 60)
    emit_syscall(buf)
}
```

**Step 4:** Test: link the simple add function with a main that calls it, verify executable runs.

**Step 5:** Commit: `feat(self-host): implement ELF static linker`

---

### Task 7.2: PE/COFF linker (Windows)

**Files:**
- Modify: `compiler/self-host/linker.kry`

**Context:** Windows equivalent — read .obj files, write .exe with PE headers.

**Step 1:** Implement PE executable writer (similar to ELF but with PE/COFF headers, import tables for kernel32.dll).

**Step 2:** Windows entry point stub calls `main`, then `ExitProcess(retval)`.

**Step 3:** Test: link on Windows, verify .exe runs.

**Step 4:** Commit: `feat(self-host): implement PE/COFF linker for Windows`

---

## Phase 8: Runtime in Kryos

### Task 8.1: OS syscall layer

**Files:**
- Create: `compiler/self-host/runtime/sys_linux.kry`
- Create: `compiler/self-host/runtime/sys_windows.kry`

**Context:** Thin wrappers around OS syscalls for memory, I/O, process control.

**Step 1:** Linux syscall wrappers (using inline assembly or a `syscall` builtin):

```kryos
// These will use the x86_64 syscall instruction directly
// RAX = syscall number, RDI = arg1, RSI = arg2, RDX = arg3

fn sys_write(fd: i64, buf: i64, len: i64) -> i64  // SYS_write = 1
fn sys_read(fd: i64, buf: i64, len: i64) -> i64   // SYS_read = 0
fn sys_open(path: i64, flags: i64) -> i64          // SYS_open = 2
fn sys_close(fd: i64) -> i64                        // SYS_close = 3
fn sys_mmap(addr: i64, len: i64, prot: i64, flags: i64, fd: i64, off: i64) -> i64  // SYS_mmap = 9
fn sys_munmap(addr: i64, len: i64) -> i64           // SYS_munmap = 11
fn sys_exit(code: i64)                               // SYS_exit = 60
fn sys_brk(addr: i64) -> i64                         // SYS_brk = 12
```

**Step 2:** Windows equivalents via kernel32.dll imports (WriteFile, ReadFile, CreateFileA, VirtualAlloc, VirtualFree, ExitProcess).

**Step 3:** Commit: `feat(self-host): add OS syscall layer for runtime`

---

### Task 8.2: Memory allocator

**Files:**
- Create: `compiler/self-host/runtime/alloc.kry`

**Context:** Simple bump allocator for initial bootstrap, upgraded to free-list allocator later.

**Step 1:** Implement basic allocator using mmap/VirtualAlloc:

```kryos
fn kryos_alloc(size: i64) -> i64     // allocate, return pointer
fn kryos_dealloc(ptr: i64)            // free
fn kryos_realloc(ptr: i64, new_size: i64) -> i64
```

**Step 2:** Start with a simple free-list allocator (linked list of free blocks with size headers).

**Step 3:** Commit: `feat(self-host): implement memory allocator in Kryos`

---

### Task 8.3: String, array, map runtime in Kryos

**Files:**
- Create: `compiler/self-host/runtime/string.kry`
- Create: `compiler/self-host/runtime/array.kry`
- Create: `compiler/self-host/runtime/map.kry`
- Create: `compiler/self-host/runtime/io.kry`

**Context:** Rewrite the Rust runtime data structures in pure Kryos.

**Step 1:** String: heap-allocated, length-prefixed (same layout as KryosString in Rust):

```kryos
// KryosString layout: [length: i64][data: bytes...]
fn string_new(data: i64, len: i64) -> i64
fn string_concat(a: i64, b: i64) -> i64
fn string_len(s: i64) -> i64
fn string_eq(a: i64, b: i64) -> i64
fn string_slice(s: i64, start: i64, end_pos: i64) -> i64
fn string_char_at(s: i64, idx: i64) -> i64
fn string_free(s: i64)
```

**Step 2:** Array: dynamic, doubling capacity:

```kryos
// KryosArray layout: [length: i64][capacity: i64][elem_size: i64][data: elements...]
fn array_new(elem_size: i64, cap: i64) -> i64
fn array_push(arr: i64, val: i64)
fn array_get(arr: i64, idx: i64) -> i64
fn array_set(arr: i64, idx: i64, val: i64)
fn array_len(arr: i64) -> i64
fn array_free(arr: i64)
```

**Step 3:** Map: open-addressing hash map:

```kryos
fn map_new() -> i64
fn map_insert(m: i64, key: i64, value: i64)
fn map_get(m: i64, key: i64) -> i64
fn map_has(m: i64, key: i64) -> i64
fn map_delete(m: i64, key: i64) -> i64
fn map_keys(m: i64) -> i64
fn map_len(m: i64) -> i64
fn map_free(m: i64)
```

**Step 4:** I/O: file operations using syscalls:

```kryos
fn file_read(path: i64) -> i64    // returns string handle
fn file_write(path: i64, content: i64) -> i64
fn println_str(s: i64)
fn print_str(s: i64)
fn eprintln_str(s: i64)
```

**Step 5:** Commit: `feat(self-host): implement string, array, map, I/O runtime in Kryos`

---

## Phase 9: Bootstrap

### Task 9.1: Integration — wire all components into main.kry

**Files:**
- Modify: `compiler/self-host/main.kry`

**Step 1:** Update main.kry to run the full pipeline:

```kryos
fn main() {
    let args = args()
    if len(args) < 2 {
        println("Usage: kryos <input.kry> [-o output]")
        exit(1)
    }
    
    let input_file = args[1]
    let output_file = if len(args) > 3 and args[2] == "-o" { args[3] } else { "a.out" }
    
    // Phase 1: Lex
    let source = file_read(input_file)
    let tokens = tokenize(source)
    
    // Phase 2: Parse
    let p = parser_new(tokens)
    let module = parse_module(p)
    if len(p.errors) > 0 { report_errors(p.errors); exit(1) }
    
    // Phase 3: Type check
    let tc = tc_new()
    tc_register_builtins(tc)
    tc_check_module(tc, module)
    if len(tc.errors) > 0 { report_errors(tc.errors); exit(1) }
    
    // Phase 4: Lower to MIR
    let mir = lower_module(module, tc)
    
    // Phase 5: Optimize
    optimize(mir)
    
    // Phase 6: Register allocate + codegen each function
    let mut code_sections: [CodegenCtx] = []
    for i in range(0, len(mir.functions)) {
        let alloc = allocate_registers(mir.functions[i])
        let ctx = codegen_function(mir.functions[i], alloc)
        push(code_sections, ctx)
    }
    
    // Phase 7: Emit object file
    let obj_path = output_file + ".o"
    write_elf_object(obj_path, code_sections, mir)
    
    // Phase 8: Link
    link_executable(output_file, [obj_path, "libkryos_rt.o"])
    
    println("Compiled: " + input_file + " -> " + output_file)
}
```

**Step 2:** Test: compile demo.kry with the self-hosted compiler (running under the Rust-backed compiler), run the output, verify correct behavior.

**Step 3:** Commit: `feat(self-host): wire full compilation pipeline`

---

### Task 9.2: Bootstrap verification

**Files:**
- Create: `scripts/bootstrap.sh`

**Step 1:** Build stage-0 (using Rust compiler):

```bash
cd compiler
cargo build --release -j 4
# Stage 0: Rust-compiled kryos binary
cp target/release/kryos ../stage0
```

**Step 2:** Build stage-1 (stage-0 compiles self-hosted source):

```bash
./stage0 run self-host/main.kry self-host/main.kry -o stage1
```

**Step 3:** Build stage-2 (stage-1 compiles self-hosted source):

```bash
./stage1 self-host/main.kry -o stage2
```

**Step 4:** Verify bootstrap:

```bash
diff stage1 stage2
# Must be identical — if not, there's a codegen bug
```

**Step 5:** Commit: `feat: bootstrap verification — Kryos compiles itself`

---

## Phase 10: Polish — Series A Readiness

### Task 10.1: Error messages with source context

**Files:**
- Modify: `compiler/self-host/types.kry` (error formatting)

**Step 1:** Errors include file, line, column, source snippet, caret, suggestion:

```
error: type mismatch
  --> main.kry:14:5
   |
14 |     let x: i64 = "hello"
   |                  ^^^^^^^ expected i64, found str
```

**Step 2:** Commit: `feat(self-host): rich error messages with source context`

---

### Task 10.2: Pre-built binaries and install script

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `scripts/install.sh`

**Step 1:** GitHub Actions release workflow: on tag push, build for Linux x86_64, macOS arm64/x86_64, Windows x86_64. Upload as release assets.

**Step 2:** Install script:

```bash
#!/bin/sh
set -e
REPO="FrostbyteDevTeam/kryos-lang"
PLATFORM=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
# ... detect platform, download binary, install to ~/.kryos/bin, add to PATH
```

**Step 3:** Commit: `feat: one-command install script and release workflow`

---

### Task 10.3: Tutorial and documentation

**Files:**
- Create: `docs/tutorial/` (5 chapters)
- Create: `docs/reference/` (language reference)

**Step 1:** Tutorial: "Build a CSV Analyzer in Kryos" — walks through structs, file I/O, string processing, error handling, formatted output. Each snippet verified by CI.

**Step 2:** Language reference: comprehensive but concise — types, operators, control flow, ownership, traits, generics, error handling, concurrency.

**Step 3:** Commit: `docs: add tutorial and language reference`

---

### Task 10.4: Benchmarks

**Files:**
- Modify: `benchmarks/`
- Add: `scripts/bench.sh`

**Step 1:** Benchmark suite using the self-hosted compiler:
- Fibonacci (recursive, iterative, TCO)
- Matrix multiplication
- String processing
- Hash map operations
- Compiler self-compile time

**Step 2:** Compare against equivalent C, Go, Rust programs. Document results.

**Step 3:** `kryos bench` command runs the full suite and prints a table.

**Step 4:** Commit: `feat: reproducible benchmark suite`
