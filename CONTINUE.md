# Kryos Compiler — Continue Autonomous Fix Pass

**Directive:** "I want perfection, and I don't care how long you need to work autonomously to get there." Fix ALL remaining issues for public release / VC readiness. No new user messages needed — just work until it's done.

**CRITICAL BUILD CONSTRAINT:** Always use `cargo build --release -j 4` and `cargo test --release -j 4`. Debug builds consume 48GB RAM and will hang.

## Project Location
- Compiler: `C:\Users\Krist\projects\active\kryos-lang\compiler`
- 21 Rust crates in `compiler/crates/` (~50k lines)
- 28 stdlib modules in `compiler/stdlib/`
- 15 self-host files in `compiler/self-host/`
- Dual backends: Cranelift (`kryos-codegen-cranelift`) + LLVM (`kryos-codegen-llvm`)
- ALL changes must be made in BOTH backends unless only one is affected

## What's Already Done (v0.2.3, commit 2be86b3)
- 925+ tests pass, 0 clippy warnings, self-host compiles
- Array element drop (per-element free loop)
- Closure capture cloning (clone heap values at capture time)
- Channel send / Spawn / ActorSend cloning (clone before ownership transfer)
- MirType::Shared fully handled (drop, struct fields, enum variants, array elements, exception cleanup, all clone paths)
- LLVM closure env uses kryos_arc_alloc (ARC-managed)
- @copy struct deep-copy retains Function/Shared fields

## Remaining Issues To Fix (priority order)

### 1. Closure capture memory leak (CRITICAL)
Closures capture heap values (Str, Array, Map, Function, Shared) by cloning them into the ARC env buffer. When the closure is dropped via `kryos_arc_release`, the ARC frees the env buffer but does NOT free the individual cloned captures inside it. Each captured string, array, map, etc. leaks.

**Fix:** Register a drop function with `kryos_arc_alloc` that iterates the env buffer and frees each captured value by type. The ARC system already supports drop functions (`kryos_arc_alloc(size, align)` — check `crates/kryos-rt/src/arc.rs`). The codegen needs to generate a per-closure dropper thunk that knows the capture layout and types, then pass it as the drop_fn when allocating the env.

Both backends need this. The capture types are available at codegen time (look at the MakeClosure/closure capture code in both backends).

### 2. Array element drop recursion into struct/enum fields (IMPORTANT)
Currently array element drop uses direct free calls (e.g., `free` for structs, `kryos_array_free` for nested arrays) to avoid infinite recursion on self-referential types like `struct Foo { children: [Foo] }`. This means struct fields inside array elements don't get their sub-fields freed (strings inside structs inside arrays leak).

**Fix:** Use a visited-set or depth limit instead of blocking recursion entirely. Or generate named drop helper functions per struct type (like Rust's Drop impl) that can be called by pointer — avoids the recursion issue while still cleaning up nested fields.

### 3. No `Self` type in traits (LANGUAGE FEATURE)
Users can't write `fn equals(other: Self) -> Bool` in trait definitions. This blocks idiomatic API design.

**Fix:** In the parser/type-checker, resolve `Self` to the implementing type when inside a trait impl block. The MIR lowering should substitute the concrete type.

### 4. No `::` associated functions (LANGUAGE FEATURE)
Can't write `MyStruct::new()` — there's no static method dispatch syntax.

**Fix:** Parse `Identifier :: Identifier (args)` as a call to a namespaced function. In the type checker, resolve `Type::method` to a function named `Type_method` or similar mangling. Codegen emits a normal function call.

### 5. Closures in structs/collections not freed (MEMORY)
Only closures in local variables get Drop instructions. Closures stored as struct fields or array elements aren't dropped when the container is dropped.

**Fix:** The struct drop and array element drop code already handles `MirType::Function` — verify this actually works end-to-end with a test. If struct field drop for Function types calls `kryos_arc_release`, this may already be fixed. Write a test to confirm.

### 6. Self-host uses concatenation instead of `use` imports
The 15 self-host files are concatenated together instead of using the module system. This makes the self-host look like it doesn't use its own features.

**Fix:** Split the concatenated self-host into proper modules with `use` statements. The module system supports file-based resolution, selective imports, and transitive deps — use them.

### 7. REPL state persistence
REPL doesn't persist variables/functions between lines.

### 8. Debug build memory usage
Debug builds consume ~48GB RAM. This is a known issue but should be investigated — likely a data structure that clones excessively or an optimization pass with exponential behavior.

### 9. `@test` annotation — function-level test runner
`@test` is parsed but there's no `kryos test` command that discovers and runs `@test`-annotated functions.

### 10. `@pure` attribute — optimization
Parsed but never used. Pure functions could enable CSE, hoisting, memoization.

## Verification Checklist
After each fix:
1. `cargo build --release -j 4` — must succeed
2. `cargo clippy --release -j 4 -- -D warnings` — 0 warnings
3. `cargo test --release -j 4` — all tests pass (ignore transient LNK1105)
4. `./target/release/kryos build self-host/main.kry` — compiles with same or fewer warnings
5. Commit with descriptive message

## Architecture Notes
- All elements stored as 8-byte i64 values (pointers for heap types)
- KryosArray layout: `{ len: i64 @0, cap: i64 @8, elem_size: i64 @16, data: *mut u8 @24 }`
- ARC: `kryos_arc_alloc(size, align)` returns user-data pointer. Header before it has ref_count + drop_fn.
- `kryos_arc_retain(ptr)` increments, `kryos_arc_release(ptr)` decrements and calls drop_fn at 0.
- Closure env: ARC-allocated buffer. Offset 0 = thunk function pointer. Offsets 8, 16, ... = captured values.
- MIR types that own heap memory: Str, Array, Map (Struct named "Map"), Function, Shared, Struct, Enum
- Clone operations: Str→kryos_string_clone, Array→kryos_array_clone, Map→kryos_map_clone, Function/Shared→kryos_arc_retain
- Free operations: Str→kryos_string_free, Array→kryos_array_free, Map→kryos_map_free, Function/Shared→kryos_arc_release, Struct→free (after field cleanup), Enum→free (after variant cleanup)
