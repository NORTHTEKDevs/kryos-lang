# Developer Adoption Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Get Kryos to VC / public launch / developer adoption quality: zero memory safety bugs, complete standard library, excellent developer experience, and a working self-hosting bootstrap chain.

**Architecture:** Four independent workstreams (WS1-WS4) each targeting a specific quality gap. WS1 fixes a heap corruption bug in Cranelift codegen. WS2 improves REPL ergonomics. WS3 adds 7 missing stdlib functions. WS4 diagnoses and fixes the stage-2 bootstrap segfault.

**Tech Stack:** Rust 2021, Cranelift 0.116, kryos-rt (C ABI builtins), kryos-codegen-cranelift, kryos-cli REPL, kryos-types.

**Build constraint:** ALWAYS `cargo build --release -j 4` and `cargo test --release -j 4`. Debug builds OOM at 48GB.

---

## WS1 — String Safety

### Task 1: Add `kryos_string_clone` after array-get for Str elements

**Files:**
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs` (around line 3055)
- Test: `tests/string_array_safety.kry` (new example file)

**Step 1: Write the failing test**

Create `examples/string_array_safety.kry`:
```
fn main() {
    let words = ["hello", "world", "kryos"]
    let w = words[0]
    println(w)
    println(words[1])
    println(w)
}
```
Expected output:
```
hello
world
hello
```

**Step 2: Run to confirm baseline**

```bash
cargo run --release -j 4 -- run examples/string_array_safety.kry
```
Expected (before fix): may crash or print garbage on third line due to dangling pointer.

**Step 3: Locate the fix site in codegen.rs**

Open `crates/kryos-codegen-cranelift/src/codegen.rs` around line 3055. Find the `RValue::Index` arm:
```rust
RValue::Index { object, index } => {
    let ptr = translate_operand(object, ...)?;
    let idx_raw = translate_operand(index, ...)?;
    let idx = ...sextend...;
    let get_ref = ensure_func_ref_with_args("kryos_array_get", ...)?;
    let call = builder.ins().call(get_ref, &[ptr, idx]);
    Ok(Some(builder.inst_results(call)[0]))
}
```

**Step 4: Apply the fix**

After the `kryos_array_get` call, check if the element type is `MirType::Str`. If so, clone the returned pointer using `kryos_string_clone`. The element type is available from the MIR node's type annotation. Pattern: look at how `RValue::BinaryOp` / `RValue::Call` handle string returns by calling `kryos_string_clone` — the same helper `ensure_func_ref_with_args("kryos_string_clone", ...)` pattern is used there.

Replace the `RValue::Index` arm with:
```rust
RValue::Index { object, index } => {
    let ptr = translate_operand(object, builder, translator, module)?;
    let idx_raw = translate_operand(index, builder, translator, module)?;
    let idx = if idx_ty.is_int() && idx_ty.bits() < 64 {
        builder.ins().sextend(types::I64, idx_raw)
    } else {
        idx_raw
    };
    let get_ref = ensure_func_ref_with_args("kryos_array_get", builder, translator, module, 2)?;
    let call = builder.ins().call(get_ref, &[ptr, idx]);
    let raw = builder.inst_results(call)[0];
    // If the array element type is Str, clone the pointer so the caller owns it.
    if matches!(elem_ty, MirType::Str) {
        let clone_ref = ensure_func_ref_with_args("kryos_string_clone", builder, translator, module, 1)?;
        let clone_call = builder.ins().call(clone_ref, &[raw]);
        Ok(Some(builder.inst_results(clone_call)[0]))
    } else {
        Ok(Some(raw))
    }
}
```

Note: `elem_ty` must be obtained from the MIR node. Look at how the surrounding match arms access type information (likely via `node.ty` or a parameter passed into the translation function). If `elem_ty` is not in scope, extract it from `object`'s `MirType` as the inner type: `if let MirType::Array(elem, _) = obj_mir_ty { elem }`.

**Step 5: Build and run**

```bash
cargo build --release -j 4
cargo run --release -j 4 -- run examples/string_array_safety.kry
```
Expected: `hello / world / hello` with no crash.

**Step 6: Run full test suite**

```bash
cargo test --release -j 4
```
Expected: 925+ tests pass.

**Step 7: Commit**

```bash
git add crates/kryos-codegen-cranelift/src/codegen.rs examples/string_array_safety.kry
git commit -m "fix: clone string element on array-get to prevent dangling pointer"
```

---

### Task 2: Fix string leak on Assign overwrite in loops

**Files:**
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs` (Instruction::Assign handling)

**Step 1: Write the failing test**

Create `examples/string_loop_assign.kry`:
```
fn main() {
    let arr = ["a", "b", "c", "d", "e"]
    let result = ""
    let i = 0
    while i < 5 {
        result = arr[i]
        i = i + 1
    }
    println(result)
}
```
Expected: `e`

**Step 2: Run baseline**

```bash
cargo run --release -j 4 -- run examples/string_loop_assign.kry
```
Verify it produces `e` (may already work). Primary concern is memory leak under Dr. Memory, not crash.

**Step 3: Locate Instruction::Assign in codegen.rs**

Search for `Instruction::Assign` (or `Instruction::Store` / assignment emission). Find where a string-typed local is overwritten. Before emitting the store of the new value, check if the slot already holds a non-null string pointer, and if so emit a `kryos_string_free` call on the old value first.

Pattern already exists in codegen for other places that free before overwrite. Find with: `grep -n "kryos_string_free" crates/kryos-codegen-cranelift/src/codegen.rs`

**Step 4: Apply the fix**

At the `Instruction::Assign` site for `MirType::Str` locals, before storing the new pointer, load the existing slot value and emit:
```rust
if matches!(local_ty, MirType::Str) {
    // Load current value from stack slot
    let old_val = builder.ins().stack_load(types::I64, slot, 0);
    // Emit free if non-null (wrap in conditional or just call — runtime handles null)
    let free_ref = ensure_func_ref_with_args("kryos_string_free", builder, translator, module, 1)?;
    builder.ins().call(free_ref, &[old_val]);
}
// Then store the new value
builder.ins().stack_store(new_val, slot, 0);
```

**Step 5: Build and test**

```bash
cargo build --release -j 4
cargo run --release -j 4 -- run examples/string_loop_assign.kry
cargo test --release -j 4
```
Expected: `e`, all tests pass.

**Step 6: Commit**

```bash
git add crates/kryos-codegen-cranelift/src/codegen.rs examples/string_loop_assign.kry
git commit -m "fix: free old string value before overwrite in loop assigns"
```

---

## WS2 — Developer Experience

### Task 3: REPL multi-line input

**Files:**
- Modify: `crates/kryos-cli/src/commands/repl.rs`

**Step 1: Write the failing test (manual)**

Start the REPL:
```bash
cargo run --release -j 4 -- repl
```
Type:
```
fn add(a: i64, b: i64) -> i64 {
```
Expected: prompt changes to `....` and waits for continuation.
Actual: currently treats as complete input and errors.

**Step 2: Implement unclosed-bracket detection**

Add a helper function to `repl.rs`:
```rust
/// Returns true if `input` has more open brackets/parens/braces than closed ones.
fn has_unclosed_delimiters(input: &str) -> bool {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut prev = '\0';
    for ch in input.chars() {
        if in_str {
            if ch == '"' && prev != '\\' { in_str = false; }
        } else {
            match ch {
                '"' => in_str = true,
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        prev = ch;
    }
    depth > 0
}
```

**Step 3: Wire it into the read loop**

In the main `loop` in `execute()`, after reading a line, accumulate it into a `pending: String`. If `has_unclosed_delimiters(&pending)`, print `....` and continue accumulating. Once closed, process `pending` as the full input.

Replace the single `reader.read_line(&mut line)` section with:
```rust
let mut pending = String::new();
loop {
    let prompt = if pending.is_empty() { "kryos> " } else { "....   " };
    print!("{prompt}");
    stdout.flush().map_err(|e| e.to_string())?;
    line.clear();
    let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
    if n == 0 { eprintln!(); return Ok(()); }
    pending.push_str(&line);
    if !has_unclosed_delimiters(pending.trim()) { break; }
}
let trimmed = pending.trim().to_string();
// Use `trimmed` everywhere below instead of `trimmed` from `line.trim()`
```

**Step 4: Build and test manually**

```bash
cargo build --release -j 4
cargo run --release -j 4 -- repl
```
Type a multi-line function definition, verify `....` prompt appears, verify function executes after closing `}`.

**Step 5: Run full suite**

```bash
cargo test --release -j 4
```

**Step 6: Commit**

```bash
git add crates/kryos-cli/src/commands/repl.rs
git commit -m "feat: REPL multi-line input with continuation prompt for unclosed delimiters"
```

---

### Task 4: `:type <expr>` shows actual type string

**Files:**
- Modify: `crates/kryos-cli/src/commands/repl.rs`
- Modify: `crates/kryos-driver/src/lib.rs` (expose type inference result)

**Step 1: Understand the current flow**

In `repl.rs`, the `:type <expr>` handler calls `kryos_driver::check_source(&wrapper, "<repl>")` which returns diagnostics. The type table is not exposed. We need `kryos_driver` to return the inferred type of `__result__`.

**Step 2: Extend `kryos_driver::check_source`**

In `crates/kryos-driver/src/lib.rs`, find `check_source`. Add an overload or extend the return type to include a `HashMap<String, String>` of variable-name-to-type-string. Specifically, after type-checking succeeds, look up `__result__` in the type environment and return its type as a display string.

Alternative (simpler): Add a new function `infer_type_of(source: &str, file: &str, var_name: &str) -> Option<String>` that runs the type checker and returns the type of the named variable.

```rust
pub fn infer_type_of(source: &str, file: &str, var_name: &str) -> Option<String> {
    let (ast, parse_diags) = kryos_parser::parse_source(source, file);
    if parse_diags.iter().any(|d| d.is_error()) { return None; }
    let (typed, _type_diags, type_env) = kryos_types::check(&ast);
    // Look up var_name in the type environment
    type_env.lookup(var_name).map(|ty| ty.display())
}
```

The exact API depends on what `kryos_types::check` returns. Read `crates/kryos-driver/src/lib.rs` and `crates/kryos-types/src/lib.rs` to find the actual return type of the type checker and how to access the inferred type map.

**Step 3: Update the `:type` handler in repl.rs**

Replace:
```rust
println!("expression `{expr}` type-checks successfully");
```
With:
```rust
match kryos_driver::infer_type_of(&wrapper, "<repl>", "__result__") {
    Some(ty) => println!("{expr} : {ty}"),
    None => println!("expression `{expr}` type-checks successfully"),
}
```

**Step 4: Build and test manually**

```bash
cargo build --release -j 4
cargo run --release -j 4 -- repl
```
Type `:type 1 + 1` → should print `1 + 1 : i64`
Type `:type "hello"` → should print `"hello" : str`

**Step 5: Run full suite**

```bash
cargo test --release -j 4
```

**Step 6: Commit**

```bash
git add crates/kryos-cli/src/commands/repl.rs crates/kryos-driver/src/lib.rs
git commit -m "feat: :type command shows actual inferred type instead of just confirming type-check"
```

---

## WS3 — Standard Library Completions

### Task 5: `index_of(s, sub) -> i64`

**Files:**
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs`
- Test: `examples/stdlib_strings.kry`

**Step 1: Add Rust implementation to builtins.rs**

Find the section with `kryos_builtin_contains` in `builtins.rs`. Add after it:
```rust
#[unsafe(no_mangle)]
pub extern "C" fn kryos_builtin_index_of(s_ptr: *mut KryosString, sub_ptr: *mut KryosString) -> i64 {
    if s_ptr.is_null() || sub_ptr.is_null() { return -1; }
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts((*s_ptr).data, (*s_ptr).len as usize)) };
    let sub = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts((*sub_ptr).data, (*sub_ptr).len as usize)) };
    match s.find(sub) {
        Some(idx) => idx as i64,
        None => -1,
    }
}
```

**Step 2: Wire in codegen.rs dispatch table**

Find the dispatch table (around line 2687) in `codegen.rs`. Add entry for `index_of`:
```rust
"index_of" => {
    let a = args[0];
    let b = args[1];
    let f = ensure_func_ref_with_args("kryos_builtin_index_of", builder, translator, module, 2)?;
    let call = builder.ins().call(f, &[a, b]);
    Ok(Some(builder.inst_results(call)[0]))
}
```

**Step 3: Write test**

Create `examples/stdlib_strings.kry`:
```
fn main() {
    let s = "hello world"
    println(to_string(index_of(s, "world")))
    println(to_string(index_of(s, "xyz")))
    println(trim_start("  hello  "))
    println(trim_end("  hello  "))
}
```
Expected:
```
6
-1
hello  
  hello
```

**Step 4: Build and run**

```bash
cargo build --release -j 4
cargo run --release -j 4 -- run examples/stdlib_strings.kry
```

**Step 5: Commit**

```bash
git add crates/kryos-rt/src/builtins.rs crates/kryos-codegen-cranelift/src/codegen.rs examples/stdlib_strings.kry
git commit -m "feat: add index_of stdlib function"
```

---

### Task 6: `trim_start(s)` and `trim_end(s)`

**Files:**
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs`

**Step 1: Add Rust implementations**

In `builtins.rs`, near `kryos_builtin_trim`, add:
```rust
#[unsafe(no_mangle)]
pub extern "C" fn kryos_builtin_trim_start(s_ptr: *mut KryosString) -> *mut KryosString {
    if s_ptr.is_null() { return s_ptr; }
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts((*s_ptr).data, (*s_ptr).len as usize)) };
    let trimmed = s.trim_start();
    kryos_string_alloc_from_bytes(trimmed.as_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn kryos_builtin_trim_end(s_ptr: *mut KryosString) -> *mut KryosString {
    if s_ptr.is_null() { return s_ptr; }
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts((*s_ptr).data, (*s_ptr).len as usize)) };
    let trimmed = s.trim_end();
    kryos_string_alloc_from_bytes(trimmed.as_bytes())
}
```

(Use whatever internal helper `kryos_builtin_trim` uses to allocate the output string — look at its implementation for the exact pattern.)

**Step 2: Wire in codegen dispatch table**

Add entries for `trim_start` and `trim_end` following the same pattern as `trim`.

**Step 3: Run test from Task 5**

```bash
cargo run --release -j 4 -- run examples/stdlib_strings.kry
```
Verify `trim_start` / `trim_end` lines now print correctly.

**Step 4: Commit**

```bash
git add crates/kryos-rt/src/builtins.rs crates/kryos-codegen-cranelift/src/codegen.rs
git commit -m "feat: add trim_start and trim_end stdlib functions"
```

---

### Task 7: `sort(arr)` and `reverse(arr)` (in-place)

**Files:**
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs`
- Test: `examples/stdlib_collections.kry`

**Step 1: Understand KryosArray layout**

Find `KryosArray` struct definition in `crates/kryos-rt/src/array.rs`. It should have: `len: i64`, `cap: i64`, `data: *mut i64` (or similar). For numeric arrays the elements are `i64` or `f64`. For string arrays they are `*mut KryosString` (as i64 pointers).

**Step 2: Add `sort` implementation**

```rust
#[unsafe(no_mangle)]
pub extern "C" fn kryos_builtin_sort(arr_ptr: *mut KryosArray) {
    if arr_ptr.is_null() { return; }
    unsafe {
        let len = (*arr_ptr).len as usize;
        let data = (*arr_ptr).data as *mut i64;
        let slice = std::slice::from_raw_parts_mut(data, len);
        slice.sort_unstable();
    }
}
```

For float arrays, sorting by i64 bits won't work correctly. Check MIR to see if sort receives element type info; if not, implement a separate `kryos_builtin_sort_f64` and dispatch based on array element type in codegen.

**Step 3: Add `reverse` implementation**

```rust
#[unsafe(no_mangle)]
pub extern "C" fn kryos_builtin_reverse(arr_ptr: *mut KryosArray) {
    if arr_ptr.is_null() { return; }
    unsafe {
        let len = (*arr_ptr).len as usize;
        let data = (*arr_ptr).data as *mut i64;
        let slice = std::slice::from_raw_parts_mut(data, len);
        slice.reverse();
    }
}
```

**Step 4: Wire in codegen**

In the dispatch table, `sort` and `reverse` take one argument (the array) and return void (no result). Check how other void builtins like `push` are handled and follow the same pattern.

**Step 5: Write test**

Create `examples/stdlib_collections.kry`:
```
fn main() {
    let nums = [3, 1, 4, 1, 5, 9, 2, 6]
    sort(nums)
    println(to_string(nums[0]))
    println(to_string(nums[7]))

    reverse(nums)
    println(to_string(nums[0]))

    let words = ["banana", "apple", "cherry"]
    sort(words)
    println(words[0])
}
```
Expected:
```
1
9
9
apple
```

**Step 6: Build and run**

```bash
cargo build --release -j 4
cargo run --release -j 4 -- run examples/stdlib_collections.kry
```

**Step 7: Run full suite**

```bash
cargo test --release -j 4
```

**Step 8: Commit**

```bash
git add crates/kryos-rt/src/builtins.rs crates/kryos-codegen-cranelift/src/codegen.rs examples/stdlib_collections.kry
git commit -m "feat: add sort and reverse stdlib functions"
```

---

### Task 8: `append_file(path, content)`

**Files:**
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-rt/src/fs.rs`
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs`

**Step 1: Add low-level function to fs.rs**

In `fs.rs`, after `kryos_fs_write`, add:
```rust
#[unsafe(no_mangle)]
pub extern "C" fn kryos_fs_append(path_ptr: *mut KryosString, content_ptr: *mut KryosString) {
    if path_ptr.is_null() || content_ptr.is_null() { return; }
    unsafe {
        let path = std::str::from_utf8_unchecked(std::slice::from_raw_parts((*path_ptr).data, (*path_ptr).len as usize));
        let content = std::slice::from_raw_parts((*content_ptr).data, (*content_ptr).len as usize);
        let _ = std::fs::OpenOptions::new().append(true).create(true).open(path)
            .and_then(|mut f| { use std::io::Write; f.write_all(content) });
    }
}
```

**Step 2: Add `kryos_builtin_append_file` to builtins.rs**

Near `kryos_builtin_write_file`, add:
```rust
#[unsafe(no_mangle)]
pub extern "C" fn kryos_builtin_append_file(path_ptr: *mut KryosString, content_ptr: *mut KryosString) {
    kryos_fs_append(path_ptr, content_ptr);
}
```

**Step 3: Wire in codegen dispatch table**

Add `append_file` following the same pattern as `write_file`.

**Step 4: Write test**

Create `examples/stdlib_fileio.kry`:
```
fn main() {
    write_file("test_output.txt", "line1\n")
    append_file("test_output.txt", "line2\n")
    let content = read_file("test_output.txt")
    println(content)
    println(to_string(file_exists("test_output.txt")))
}
```
Expected:
```
line1
line2

true
```

**Step 5: Build and run**

```bash
cargo build --release -j 4
cargo run --release -j 4 -- run examples/stdlib_fileio.kry
```

**Step 6: Run full suite**

```bash
cargo test --release -j 4
```

**Step 7: Commit**

```bash
git add crates/kryos-rt/src/builtins.rs crates/kryos-rt/src/fs.rs crates/kryos-codegen-cranelift/src/codegen.rs examples/stdlib_fileio.kry
git commit -m "feat: add append_file stdlib function"
```

---

### Task 9: `http_get(url) -> str` (blocking, no TLS required)

**Files:**
- Modify: `crates/kryos-rt/Cargo.toml` (add ureq or minreq dependency)
- Modify: `crates/kryos-rt/src/builtins.rs`
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs`
- Modify: `Cargo.toml` (workspace dep)

**Step 1: Add HTTP crate to workspace**

In root `Cargo.toml`, under `[workspace.dependencies]`, add:
```toml
ureq = { version = "2", features = ["native-tls"] }
```

In `crates/kryos-rt/Cargo.toml`, add:
```toml
ureq = { workspace = true }
```

**Step 2: Add `kryos_builtin_http_get` to builtins.rs**

```rust
#[unsafe(no_mangle)]
pub extern "C" fn kryos_builtin_http_get(url_ptr: *mut KryosString) -> *mut KryosString {
    if url_ptr.is_null() { return kryos_string_alloc_from_bytes(b""); }
    let url = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts((*url_ptr).data, (*url_ptr).len as usize))
    };
    let body = ureq::get(url)
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .unwrap_or_default();
    kryos_string_alloc_from_bytes(body.as_bytes())
}
```

**Step 3: Wire in codegen dispatch table**

Add `http_get` following the same pattern as string-returning functions (1 arg, returns string pointer).

**Step 4: Write test (network test — run manually, skip in CI)**

Add to `examples/stdlib_fileio.kry` or create `examples/stdlib_http.kry`:
```
fn main() {
    let response = http_get("http://httpbin.org/get")
    println(contains(response, "url"))
}
```
Expected: `true`

**Step 5: Build**

```bash
cargo build --release -j 4
```

**Step 6: Run full suite**

```bash
cargo test --release -j 4
```

**Step 7: Commit**

```bash
git add crates/kryos-rt/Cargo.toml crates/kryos-rt/src/builtins.rs crates/kryos-codegen-cranelift/src/codegen.rs Cargo.toml
git commit -m "feat: add http_get stdlib function using ureq"
```

---

## WS4 — Stage-2 Bootstrap

### Task 10: Binary-search stage-2 segfault

**Files:**
- Read: `self-host/bootstrap.sh`
- Read: `self-host/x86.kry`
- Read: `self-host/codegen.kry`

**Step 1: Run stage-1 on minimal file**

```bash
cd self-host
./bootstrap.sh --stage 1
```

If that produces a `stage1` binary, run it on a minimal file:
```bash
echo 'fn main() {}' > minimal.kry
./stage1 minimal.kry
```
If segfault: the bug is in stage-1 output, not stage-2. If it works, proceed.

**Step 2: Run stage-1 to compile stage-2**

```bash
./stage1 kryos.kry -o stage2
./stage2 minimal.kry
```
If segfault on `./stage2 minimal.kry`: the bug is in the code stage-1 generates for the self-host compiler.

**Step 3: Binary-search source files**

Progressively add self-host source files until segfault appears. Start with just `main.kry`, then add `lexer.kry`, `parser.kry`, `types.kry`, `x86.kry`, `coff.kry` one at a time. The file that causes the segfault contains the bug.

**Step 4: Instrument the offending file**

In `x86.kry` or whichever file causes the fault, add bounds-check assertions before buffer writes:
```
fn emit_byte(buf: [i64], pos: i64, byte: i64) {
    if pos >= len(buf) {
        println("BOUNDS OVERFLOW at pos=" + to_string(pos) + " len=" + to_string(len(buf)))
        exit(1)
    }
    buf[pos] = byte
}
```

**Step 5: Run with instrumentation**

```bash
./stage1 kryos.kry -o stage2_debug
./stage2_debug minimal.kry
```
The bounds-check println will identify the exact location of the overflow.

**Step 6: Fix the root cause**

Typical causes:
- Buffer allocated too small (capacity not accounting for COFF/ELF header size)
- Section header offsets computed incorrectly for Windows COFF (alignment requirements)
- String table not null-terminated

Fix the specific computation that overflows.

**Step 7: Verify bootstrap**

```bash
./bootstrap.sh --verbose
```
Expected: `BOOTSTRAP VERIFIED` with matching SHA-256 for stage-2 and stage-3.

**Step 8: Commit**

```bash
git add self-host/
git commit -m "fix: resolve stage-2 bootstrap segfault in self-hosted code emitter"
```

---

## WS3 Supplement — Declare new stdlib in prelude

### Task 11: Register new functions in standard prelude

**Files:**
- Find and modify the standard prelude declaration file (likely `crates/kryos-parser/src/prelude.rs` or `crates/kryos-types/src/prelude.rs`)

**Step 1: Find the prelude**

```bash
grep -r "read_file\|write_file" crates/ --include="*.rs" -l
```
This will identify where stdlib functions are declared for the type checker.

**Step 2: Add declarations**

Add function signatures for all new functions: `index_of`, `trim_start`, `trim_end`, `sort`, `reverse`, `append_file`, `http_get`.

For example:
```rust
("index_of", Type::Fn(vec![Type::Str, Type::Str], Box::new(Type::I64))),
("trim_start", Type::Fn(vec![Type::Str], Box::new(Type::Str))),
("trim_end", Type::Fn(vec![Type::Str], Box::new(Type::Str))),
("sort", Type::Fn(vec![Type::Array(Box::new(Type::Unknown))], Box::new(Type::Void))),
("reverse", Type::Fn(vec![Type::Array(Box::new(Type::Unknown))], Box::new(Type::Void))),
("append_file", Type::Fn(vec![Type::Str, Type::Str], Box::new(Type::Void))),
("http_get", Type::Fn(vec![Type::Str], Box::new(Type::Str))),
```

**Step 3: Build and run full suite**

```bash
cargo build --release -j 4
cargo test --release -j 4
```

**Step 4: Commit**

```bash
git add crates/
git commit -m "feat: declare new stdlib functions in type checker prelude"
```

---

## Final Verification

### Task 12: Run all examples and check for regressions

**Step 1: Run all 13+ examples**

```bash
for f in examples/*.kry; do
    echo "=== $f ==="
    cargo run --release -j 4 -- run "$f"
done
```
Expected: All complete without crash or unexpected output.

**Step 2: Run full test suite**

```bash
cargo test --release -j 4 2>&1 | tail -5
```
Expected: 925+ tests, 0 failures.

**Step 3: Test REPL**

```bash
cargo run --release -j 4 -- repl
```
Manual checks:
- Multi-line function definition with `....` prompt
- `:type 1 + 2` shows `i64`
- `:reset` clears state
- Ctrl+C exits cleanly

**Step 4: Invoke code-reviewer**

Use the `code-reviewer` skill to audit WS1-WS3 changes for correctness, safety, and edge cases. Target: 10/10.

**Step 5: Invoke production-certifier**

Use the `production-certifier` skill for final production readiness gate. Target: 10/10.

**Step 6: Final commit tag**

```bash
git tag v0.2.1-developer-adoption
```

---

**Plan complete and saved to `docs/plans/2026-04-13-developer-adoption.md`. Two execution options:**

**1. Subagent-Driven (this session)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open a new session with executing-plans, batch execution with checkpoints

**Which approach?**
