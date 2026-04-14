# Kryos Developer Adoption Design — 2026-04-13

## Goal

Get Kryos to VC / public launch / developer adoption quality: zero memory safety bugs, complete standard library, excellent developer experience, and a working self-hosting bootstrap chain.

## Approved Approach

Four parallel workstreams, each independently mergeable:

---

## Workstream 1 — String Safety

**Problem:** `kryos_array_get` returns a raw pointer into the array's heap buffer. When the caller stores this into a named local and then the array is dropped, the local becomes a dangling pointer. Any subsequent use (or second free by array destructor) causes heap corruption / STATUS_HEAP_CORRUPTION.

**Fix:**
- In the Cranelift codegen (`crates/kryos-codegen-cranelift/src/`), wherever `RValue::Index` is lowered for an `Array(Str)` element, emit a call to `kryos_string_clone(ptr)` instead of returning the raw pointer.
- In `Instruction::Assign` for `String`-typed locals inside a loop, emit `kryos_string_free(old_ptr)` before the overwrite to prevent accumulating leaks.

---

## Workstream 2 — Developer Experience

**Problem:** Compilation errors abort the entire parse, leaving users with no recovery. The REPL cannot handle multi-line input, `:type <expr>` only says "type-checks" rather than showing the actual type, and diagnostics have no "did you mean?" hints.

**Fixes:**
- Error recovery: introduce an `Unknown` sentinel type so the type checker continues after a type error, accumulating all errors before aborting.
- REPL multi-line: detect unclosed `{`, `(`, `[` at end of input and prompt with `....` continuation lines.
- `:type <expr>`: wire through the type table so the actual inferred type string is printed.
- "did you mean?": on undefined variable/function, compute Levenshtein distance against names in scope and suggest the closest match in the diagnostic.

---

## Workstream 3 — Language-Complete Standard Library

**Target:** Every common operation a developer reaches for in a systems/scripting language.

**String utilities:** `split(s, delim) -> [str]`, `trim(s) -> str`, `trim_start`, `trim_end`, `contains(s, sub) -> bool`, `starts_with(s, prefix) -> bool`, `ends_with(s, suffix) -> bool`, `replace(s, old, new) -> str`, `to_lower(s) -> str`, `to_upper(s) -> str` (already exists), `index_of(s, sub) -> i64`, `join(arr, sep) -> str`

**File I/O:** `read_file(path) -> str`, `write_file(path, content)`, `append_file(path, content)`, `file_exists(path) -> bool`

**Collections:** `sort(arr)` (in-place, numeric or lexicographic), `reverse(arr)` (in-place)

**Math:** `pow(base, exp) -> f64`, `log(x) -> f64`, `log2(x) -> f64`, `min(a, b)`, `max(a, b)` (already partially exists - verify/complete)

**Environment:** `env_get(key) -> str`, `args() -> [str]`, `exit(code)`

**Networking:** `http_get(url) -> str` (blocking, no TLS required for v1)

All implemented in `crates/kryos-stdlib-native/src/` as Rust `extern "C"` functions, registered in the runtime symbol table, and declared in the standard prelude.

---

## Workstream 4 — Stage-2 Bootstrap

**Problem:** `bootstrap.sh` stage-1 binary (Rust-compiled Kryos) compiles stage-2 successfully, but stage-2 binary segfaults when trying to compile stage-3. The fault is in the self-hosted x86 emitter or ELF/COFF writer.

**Fix strategy:**
1. Binary-search the regression: run stage-1 on a minimal `.kry` file (just `fn main() {}`). If that works, incrementally add self-host source until it segfaults.
2. Instrument `self-host/x86.kry` emit functions with bounds-checks on buffer writes.
3. Verify `self-host/coff.kry` section header offsets are computed correctly for Windows (the primary dev platform).
4. Once stage-2 produces matching SHA-256 to stage-3, bootstrap is verified.

---

## Success Criteria

- All 925+ existing tests pass (`cargo test --release -j 4`)
- All 13 example programs run to completion with correct output, zero crashes
- `bootstrap.sh --verbose` prints "BOOTSTRAP VERIFIED" with matching SHA-256
- REPL handles multi-line, `:type`, `:reset`, Ctrl+C cleanly
- Standard library covers every function in the above list
- No memory leaks or heap corruption under Valgrind / Dr. Memory on any example
- Code reviewer: 10/10, Production certifier: 10/10
