# Kryos v0.3.0 — Complete Language Design

> **Status:** Approved 2026-04-04
> **Goal:** Make every parsed language feature work end-to-end through MIR lowering and both codegen backends (Cranelift AOT + LLVM IR text), fix all known bugs, implement missing runtime infrastructure, and reach production-quality zero-defect status.

## Current State (v0.2.0)

### Architecture

```
Source (.kry)
  → Lexer (kryos-lexer, 80+ tokens)
  → Parser (kryos-parser, recursive descent + Pratt)
  → AST (kryos-ast, 60+ node types)
  → Type Checker (kryos-types, HM inference + unification)
  → Ownership Analysis (kryos-ownership, move semantics + ARC)
  → Capability Checker (kryos-capabilities, compile-time security)
  → MIR Lowering (kryos-mir, CFG with basic blocks)
  → Codegen:
      ├─ Cranelift AOT (kryos-codegen-cranelift, object files)
      ├─ Cranelift JIT (kryos-codegen-cranelift, in-memory execution)
      └─ LLVM IR Text (kryos-codegen-llvm, .ll files → llc/clang)
  → Linker (kryos-linker, platform-aware linking)
```

Supporting crates: kryos-cli, kryos-driver, kryos-fmt, kryos-doc, kryos-lsp, kryos-test-runner, kryos-package, kryos-bindgen, kryos-rt, kryos-stdlib-native, kryos-errors.

**21 crates. 390 tests. All passing.**

### What Works End-to-End

| Feature | Parse | Type Check | MIR | Cranelift | LLVM |
|---------|-------|------------|-----|-----------|------|
| Functions (params, return, calls) | Y | Y | Y | Y | Y |
| Integer arithmetic | Y | Y | Y | Y | Y |
| Float arithmetic | Y | Y | Y | Y | Y |
| Boolean logic | Y | Y | Y | Y | Y |
| String literals (C strings) | Y | Y | Y | Y | Y |
| Let bindings (immutable) | Y | Y | Y | Y | Y |
| Let mut (mutable) | Y | Y | Y | Y | **BUG** |
| If/elif/else | Y | Y | Y | Y | Y |
| While loops | Y | Y | Y | Y | Y |
| For-range loops | Y | Y | Y | Y | Y |
| Structs (create) | Y | Y | Y | Y | Y |
| Struct field access | Y | Y | Y | Y | **BUG** |
| Match (integers) | Y | Y | Y | Y | Y |
| Break/continue | Y | Y | Y | Y | Y |
| Multi-file imports | Y | Y | Y | Y | Y |
| Casts | Y | Y | Y | Y | Y |
| Unary ops | Y | Y | Y | Y | Y |
| Arrays/tuples (insertvalue) | Y | - | Y | Y | Y |
| ARC shared refs | Y | Y | Y | stub | stub |

### Known Bugs (5)

1. **LLVM SSA Bug** — `codegen.rs:395-410`: Mutable variables emit `%_N = add type val, 0` which defines the same SSA name twice when reassigned across basic blocks (loops). Fix: alloca/store/load for all mutable locals.

2. **LLVM Field Access** — `codegen.rs:526-538`: Hardcoded `extractvalue ... 0` ignores actual field index. Fix: carry struct_defs into LLVM codegen, resolve field name → index.

3. **len() Stub** — Cranelift `codegen.rs:273-291`: Returns 0. Non-range for-loops produce infinite loops. Fix: implement real len() with array metadata.

4. **to_string() Stub** — Cranelift `codegen.rs:301-326`: Returns input unchanged. Fix: implement integer-to-string conversion with snprintf.

5. **eprintln → puts** — Cranelift `codegen.rs:243-245`: Routes to stdout. Fix: use fprintf(stderr, ...).

### Parsed But Not Lowered (16 features)

| Feature | AST Node | MIR Status |
|---------|----------|------------|
| Enum declarations | `Decl::Enum` | Skipped |
| Enum pattern matching | `Pattern::Enum` | Not wired |
| Impl blocks (methods) | `Decl::Impl` | Skipped |
| Trait declarations | `Decl::Trait` | Skipped |
| Generics | `GenericParam` | No monomorphization |
| Type aliases | `Decl::TypeAlias` | Skipped |
| Extern/FFI | `Decl::Extern` | Skipped |
| Actor declarations | `Decl::Actor` | Nop |
| Try/catch | `Stmt::TryCatch` | Nop |
| Throw | `Stmt::Throw` | Nop |
| Spawn | `Stmt::Spawn` | Nop |
| Select | `Stmt::Select` | Nop |
| Lambdas/closures | `Expr::Lambda` | Falls to `<closure>` |
| Interpolated strings | `Expr::InterpolatedString` | Not lowered |
| Pipe expressions | `Expr::PipeExpr` | Not lowered |
| Map literals | `Expr::MapLiteral` | Not lowered |

### Missing Infrastructure

- **String type**: Currently C char pointers. Need heap-allocated strings with length field.
- **Heap allocation**: ARC stubs only. No malloc/free wiring for arrays, strings, maps.
- **Result/Option**: Not special types in the type system. Need enum + generic support first.
- **Method dispatch**: No name mangling (`Type::method` → `Type__method`).
- **Trait dispatch**: No vtable generation or monomorphization.
- **Generic monomorphization**: Type checker has generics but MIR doesn't specialize.
- **Closure capture**: No environment struct allocation.
- **Array runtime**: Need real len, push, pop, index-with-bounds-check.
- **String runtime**: Need concat, slice, find, split, etc. as compiled builtins.

---

## v0.3.0 Design

### Phase 1: Fix All Bugs + Housekeeping

**LLVM SSA Fix**: For every mutable local, emit `alloca` in the entry block, `store` on assignment, `load` on use. Immutable locals keep direct SSA names. This is the standard LLVM approach (mem2reg optimizes it later).

**LLVM Field Access Fix**: Pass `MirModule::struct_defs` into `LlvmCodegen`. On `RValue::Field`, look up the struct type from the object operand, find the field index, emit `extractvalue` with the correct index.

**len() Fix**: Arrays need metadata. Introduce a runtime struct: `{ i64 len, ptr data }`. The `len()` builtin reads the first field. For-loop desugaring already calls len() — it just needs a real implementation.

**to_string() Fix**: Call `snprintf` to convert i64 to string, allocate buffer, return pointer.

**eprintln Fix**: Declare `fprintf` + `stderr` global, route eprintln through it.

### Phase 2: Enums + Full Pattern Matching

**Representation**: Tagged union. MIR gets new types:
- `MirType::Enum(String)` — reference by name
- Enum metadata in `MirModule::enum_defs`: maps enum name → list of variants with field types

**Memory layout**: `{ i64 tag, [max_payload_size x i8] payload }`. Tag is variant index (0, 1, 2...). Payload is the largest variant's fields.

**MIR lowering**:
- `Decl::Enum` → populate `enum_defs`
- Enum variant construction → `RValue::EnumVariant { name, variant, fields }`
- `Pattern::Enum` in match → extract tag, compare, extract payload fields

**Codegen**: Both backends get enum-aware struct layout, tag extraction (GEP/extractvalue), payload casting.

### Phase 3: Methods (Impl Blocks)

**Strategy**: Desugar methods to free functions with name mangling: `impl Point { fn distance(self) }` → `fn Point__distance(self: Point)`. The `self` parameter is always the first argument.

**MIR lowering**:
- `Decl::Impl` → iterate methods, lower each as `fn TypeName__methodName(self, ...)`
- `Expr::MethodCall { object, method, args }` → `RValue::Call { func: "TypeName__methodName", args: [object, ...args] }`
- Need to resolve object type to find the impl target for name mangling

**Both backends**: No special handling needed — methods are just functions.

### Phase 4: Traits + Generics

**Traits**: Interface declarations. Two dispatch strategies:
1. **Monomorphization** (Rust-style): Duplicate function bodies for each concrete type. Faster, larger binary.
2. **Vtable** (Go-style): Runtime dispatch through function pointer table. Smaller binary, slight overhead.

**Decision**: Monomorphization first (simpler, matches Rust semantics). Vtable later if binary size matters.

**Generic monomorphization**:
- During MIR lowering, when a generic function is called with concrete type args, create a specialized copy: `fn max<T>(a: T, b: T)` called as `max(1, 2)` → `fn max__i64(a: i64, b: i64)`
- Track instantiations to avoid duplicates
- Trait bounds become compile-time checks: "does this type have an impl for this trait?"

**MIR changes**:
- `MirModule::trait_defs`: maps trait name → list of method signatures
- `MirModule::impl_map`: maps (type, trait) → list of monomorphized function names
- Generic functions stored as templates, instantiated on demand

### Phase 5: Heap Allocation + String Type + Runtime Builtins

**Heap allocation**: Wire `malloc`/`free` through both backends as imported C functions. Every heap object gets: `{ i64 refcount, i64 size, [data] }` header (unifies with ARC).

**String type**: `KryosString = { i64 len, i64 cap, ptr data }`. Null-terminated for C interop but length-tracked for O(1) len().

**Array type**: `KryosArray = { i64 len, i64 cap, i64 elem_size, ptr data }`. Bounds-checked index access.

**Runtime builtins** (compiled into every binary):
- `kryos_string_new(ptr, len) → KryosString*`
- `kryos_string_concat(a, b) → KryosString*`
- `kryos_string_len(s) → i64`
- `kryos_string_eq(a, b) → bool`
- `kryos_array_new(elem_size, cap) → KryosArray*`
- `kryos_array_push(arr, val)`
- `kryos_array_get(arr, idx) → val`
- `kryos_array_len(arr) → i64`

These are defined in `kryos-rt` as Rust functions with `#[no_mangle] extern "C"` and compiled to a static library that gets linked in.

### Phase 6: Error Handling (Result/Option)

With enums + generics working:

```kryos
enum Option<T> {
    Some(T),
    None
}

enum Result<T, E> {
    Ok(T),
    Err(E)
}
```

These are regular enums, defined in a `core` prelude module auto-imported into every file.

**Try/catch lowering**: `try { ... } catch e { ... }` desugars to:
- Wrap the try block's result in `Result::Ok`
- On `throw expr`, produce `Result::Err(expr)`
- Catch block receives the error value

**? operator** (future): Desugar `expr?` to match + early return on Err.

### Phase 7: Closures/Lambdas + Pipe Expressions

**Closure representation**: `{ ptr fn_ptr, ptr env }`. The environment is a heap-allocated struct containing captured variables.

**MIR lowering**:
- `Expr::Lambda` → create a new MIR function (anonymous name), analyze captured variables, allocate environment struct, store captures, create closure pair

**Pipe expressions**: `a |> f` desugars to `f(a)`. `a |> f(b)` desugars to `f(a, b)`. Pure syntactic sugar — lowered before MIR.

### Phase 8: Type Aliases + Extern/FFI

**Type aliases**: `type Name = TypeExpr` → register in type resolver, expand during type checking. No MIR changes needed.

**Extern/FFI**: `extern "C" { fn puts(s: *u8) -> i32; }` → declare the function in both backends as an imported symbol with the specified calling convention. Wire through MIR as external function declarations.

### Phase 9: Interpolated Strings + Map Literals

**Interpolated strings**: `"hello {name}, you are {age}"` → lower to a series of `to_string()` calls and `string_concat()` calls.

**Map literals**: `{ "key": value }` → new MIR type `MirType::Map(Box<MirType>, Box<MirType>)`, runtime hash map implementation in `kryos-rt`.

### Phase 10: Actors/Spawn/Channels/Select

**Actor model**: Requires thread spawning and message passing. Implementation:
- `spawn expr` → `kryos_spawn(fn_ptr, env)` using OS threads
- Channels → `kryos_channel_new()`, `kryos_channel_send()`, `kryos_channel_recv()`
- Select → poll multiple channels

These are already defined in `kryos-rt` as Rust FFI functions. Wire them through MIR lowering.

### Phase 11: Comptime Blocks

**Strategy**: Evaluate `comptime { ... }` during compilation by interpreting the block. The result replaces the comptime expression in the AST before MIR lowering. Requires a subset interpreter that can evaluate pure expressions at compile time.

### Phase 12: Full Audit + Certification

- Run `code-reviewer` across entire compiler
- Run `production-certifier`
- Fix every issue flagged
- Verify all 390+ tests still pass
- Add integration tests for every new feature
- Benchmark performance regression
- Verify both backends produce correct output for the full test suite
