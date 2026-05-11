# Unsafe Code Audit

This document catalogs every category of `unsafe` block used in the Kryos
runtime (`kryos-rt`), code generator (`kryos-codegen-*`), and native standard
library (`kryos-stdlib-native`). Its purpose is twofold:

1.  **Reviewability** — a human auditor can read this document, then read each
    `unsafe { ... }` block in the source and verify it matches one of the
    documented patterns and obeys its stated invariants.
2.  **Discipline** — new `unsafe` code must either fit one of these patterns
    (and reference it in a `// SAFETY:` comment) or extend this document.

The Kryos compiled-program ABI uses **opaque `i64` handles** for every
heap-allocated runtime type (strings, arrays, maps, channels, tensors, Arc'd
boxes, etc.). This is the source of most `unsafe` in the runtime: every entry
point from generated code takes `i64` and reconstructs a pointer.

Total `unsafe` count (May 2026): **~310 blocks across 24 files**.

---

## 1. FFI Handle Reconstruction

**Pattern.**

```rust
let s = handle as *const crate::string::KryosString;
unsafe { (*s).len }
```

**Where.** `builtins.rs`, `string.rs`, `map.rs`, `tensor.rs`, `channel.rs`,
`array.rs`, every `extern "C"` entry point from codegen.

**Invariants.**

*   The caller (codegen-emitted code) guarantees the handle was produced by the
    matching constructor in this crate, e.g.:
    *   `*const KryosString` ← `kryos_string_new` / `kryos_string_concat` / ...
    *   `*const KryosArray` ← `kryos_array_new` / array literal codegen.
    *   `*const MapHeader` ← `kryos_map_new`.
    *   `*const Tensor` ← `kryos_tensor_new` / shape ops.
*   `0` is a sentinel for "null handle" — all entry points must early-return on
    `handle == 0` before dereferencing.
*   Handles are reference-counted via the `Arc` discipline in `arc.rs`. A
    handle held by generated code carries one logical strong ref; the runtime
    must not drop until codegen emits `kryos_*_release`.

**Audit status.** Universally followed. Every entry point checks `handle != 0`
before deref. Type confusion is impossible from well-typed Kryos source because
the type checker ensures handle types match.

**Risk.** Low. Type confusion would require either (a) raw `transmute` in
Kryos source (not currently supported by the language) or (b) a codegen bug
that emits the wrong release/use call — these are caught by `cargo test`.

---

## 2. Slice Construction from Raw Parts

**Pattern.**

```rust
let slice = unsafe { std::slice::from_raw_parts(data, len) };
let text  = std::str::from_utf8(slice).unwrap_or("");
```

**Where.** Most string handling. ~60 occurrences.

**Invariants.**

*   `data` and `len` are loaded from a `KryosString` whose handle was
    validated (see pattern 1). `KryosString` invariants:
    *   `data` is non-null when `len > 0`. We check `!data.is_null() && len > 0`
        at every call site before constructing the slice.
    *   `data` points to `len` bytes of valid heap allocation owned by the
        string's `Arc`-tracked allocation; lifetime is bounded by the call
        because we never store the returned `&str` across a refcount drop.
    *   Bytes are not necessarily valid UTF-8; we always use
        `str::from_utf8(...).unwrap_or("")` (or `.unwrap()` only when the
        producer guaranteed UTF-8, e.g. names freshly built by `to_string`).

**Audit status.** Consistent. The helper `unsafe fn bytes_to_str` in
`builtins.rs` encapsulates this pattern for one-shot reads.

**Risk.** Low. Use-after-free would require dropping the string between the
slice construction and the read — generated code is single-threaded per
string handle (no aliasing without atomic refcount bump via `kryos_string_retain`).

---

## 3. Allocator (`alloc` / `dealloc`)

**Pattern.**

```rust
let layout = Layout::from_size_align(size, align).unwrap();
let ptr = unsafe { alloc::alloc::alloc(layout) };
// ... use ptr ...
unsafe { alloc::alloc::dealloc(ptr, layout) };
```

**Where.** `arc.rs` (Arc box), `string.rs` (string buffer), `array.rs` (array
backing), `map.rs` (hash table), `tensor.rs` (tensor data).

**Invariants.**

*   Every `alloc` is paired with exactly one `dealloc` using the **same
    `Layout`**. The Layout is reconstructed from header fields (capacity,
    element size) which never change post-allocation.
*   `Layout::from_size_align` is only called with valid `align` (always a
    power of two from `std::mem::align_of::<T>()`).
*   Out-of-memory: we return null and let the caller panic, per the runtime's
    "abort on OOM" policy (we are a systems language; OOM is unrecoverable).

**Audit status.** Followed. Pairing is enforced by the `Drop` impl on
`KryosArc<T>` / `KryosString` / etc.; raw allocs that don't use these wrappers
are only used in two spots (string concat fast-path, tensor reshape) and both
have inline release.

**Risk.** Medium. The realloc paths in `string.rs` (concat) and `array.rs`
(push) are the most subtle: they allocate a new buffer, memcpy, then dealloc
the old buffer with the *old* layout. The `// SAFETY:` notes there explicitly
state "old_layout uses old_capacity, not new_capacity".

---

## 4. Atomic Reference Counting

**Pattern.**

```rust
let old = unsafe { (*header).refcount.fetch_add(1, Ordering::Relaxed) };
// ...
let prev = unsafe { (*header).refcount.fetch_sub(1, Ordering::Release) };
if prev == 1 {
    std::sync::atomic::fence(Ordering::Acquire);
    unsafe { drop_in_place(header); }
}
```

**Where.** `arc.rs` and inlined in `string.rs`, `array.rs`, etc.

**Invariants.** Standard Arc memory ordering:

*   Clone uses **Relaxed** (consistent with Rust's `std::sync::Arc`).
*   Drop uses **Release** on decrement, **Acquire** fence before destruction.
    This synchronizes the "last writer" with the destructor.
*   Refcount of 0 is the destruction sentinel; we never re-clone from a 0
    refcount because we always destroy before the last reference dies.

**Audit status.** Modeled after the official Rust standard library's `Arc`.
Documented in `arc.rs` with multi-line SAFETY notes.

**Risk.** Low. Identical ordering pattern to Rust std; no novel concurrency.

---

## 5. Signal Handler (`stack_guard.rs`)

**Pattern.** Raw `sigaction` + `sigaltstack` syscalls to install a SIGSEGV
handler that prints "stack overflow" and `_exit`s.

**Where.** `kryos-rt/src/stack_guard.rs`.

**Invariants.**

*   The handler is **async-signal-safe**: it only calls `write(2)` and
    `_exit(2)` — both POSIX async-signal-safe.
*   We allocate a 64 KB sigaltstack so the handler runs on its own stack
    (otherwise stack overflow would re-fault inside the handler).
*   `install()` is `Once`-guarded so calling it twice is harmless.
*   We do **not** call back into Rust code from the handler — no allocation,
    no Mutex, no thread-locals.

**Audit status.** Documented inline. Async-signal-safe by inspection.

**Risk.** Low for the handler itself. Note: if the user's program installs
its own SIGSEGV handler later, ours will be overridden — this is a deliberate
trade-off (don't fight user code).

---

## 6. Threading Primitives (`channel.rs`, `actor.rs`, `spawn.rs`)

**Pattern.** Mutex/Condvar wrapped in a heap allocation that's shared by
sender and receiver halves of a channel; raw pointer used as `i64` handle.

**Where.** `channel.rs` (11 unsafe), `actor.rs` (few), `spawn.rs` (few).

**Invariants.**

*   The channel header `ChannelHeader { refcount, mutex<VecDeque<i64>>, ... }`
    is Arc-counted; sender and receiver each hold one logical refcount.
*   Send/recv hold the mutex for the duration of the queue mutation — never
    across an `await` or `.recv()` blocking call (we use `Condvar` instead).
*   Drop of the last endpoint sets a "closed" flag under the mutex and signals
    the condvar, so blocked receivers wake up and return `Err`.

**Audit status.** Reviewed. The closed-flag handshake matches `crossbeam`'s
discipline.

**Risk.** Low–Medium. Known limitations:

*   No bounded channels yet (all are unbounded MPMC).
*   No fairness guarantees.

---

## 7. C Library / Syscall FFI (`stdlib-native/*.rs`)

**Pattern.** `extern "C"` declarations for libc functions (open, read, write,
fork, exec, sqlite3_*, ...) and direct calls.

**Where.** `stdlib-native/process.rs` (15), `stdlib-native/string.rs` (13),
`stdlib-native/sqlite.rs` (12), plus `fs.rs`, `net.rs`.

**Invariants.**

*   Every libc call uses POSIX-documented argument types; CStr nul-termination
    is enforced by passing through `CString::new(...).unwrap()` (or `Err`).
*   File descriptors are wrapped in a thin Drop-closing handle so leaks are
    caught.
*   `process.rs` fork/exec follows the "only async-signal-safe fns between
    fork and exec" rule — no Rust allocation or stdlib calls in the child.

**Audit status.** Reviewed. The fork-safety constraint is the only one that
requires care; documented inline.

**Risk.** Medium. The SQLite binding is the largest single FFI surface; we
rely on rusqlite-style assumptions (statement outlives bound parameters).

---

## 8. Cranelift / LLVM JIT Memory

**Pattern.** Cranelift's `JITModule::finalize_definitions()` requires `unsafe`
because it mprotects memory as executable.

**Where.** `kryos-codegen-cranelift/src/codegen.rs` (small number of calls).

**Invariants.** Standard Cranelift discipline:

*   Finalize only after all definitions are added.
*   `get_finalized_function(id)` returns a function pointer that's valid for
    the lifetime of the JIT module.

**Audit status.** Boilerplate, follows Cranelift docs.

**Risk.** Low. If misused, Cranelift would surface the error.

---

## Block-by-Block Coverage Goals

This audit aims for **every `unsafe { ... }` block** in the runtime to either:

1.  Be obvious (e.g. one-line `unsafe { *(handle as *const i64) }` in a
    function whose entire docstring documents the invariant), **or**
2.  Carry a `// SAFETY:` comment naming one of the patterns above.

Current coverage (May 2026):

| File                                 | Unsafe blocks | Has SAFETY notes |
| ------------------------------------ | ------------- | ---------------- |
| `kryos-rt/arc.rs`                    | 9             | Yes (good)       |
| `kryos-rt/stack_guard.rs`            | 3             | Yes (good)       |
| `kryos-rt/channel.rs`                | 11            | Partial          |
| `kryos-rt/string.rs`                 | 24            | Partial          |
| `kryos-rt/map.rs`                    | 18            | Partial          |
| `kryos-rt/tensor.rs`                 | 44            | Partial          |
| `kryos-rt/builtins.rs`               | 78            | Sparse           |
| `kryos-stdlib-native/process.rs`     | 15            | Partial          |
| `kryos-stdlib-native/string.rs`      | 13            | Partial          |
| `kryos-stdlib-native/sqlite.rs`      | 12            | Partial          |

**Plan for v0.4.0:** add `// SAFETY: pattern N — <one-line invariant>` to every
naked unsafe block in `builtins.rs`, `tensor.rs`, `map.rs`, and `string.rs`.
Done incrementally; this document is the contract.

## Out of Scope

*   **Memory-safe Kryos source code.** Kryos source has no `unsafe` keyword
    yet. When it is added, it will follow Rust's "unsafe is a promise, not a
    permission" model and have its own audit document.
*   **JIT-emitted code.** Cranelift/LLVM emit machine code; we trust those
    backends. Bugs there would be backend bugs, filed upstream.

---

*Last updated: May 2026, against commit at HEAD of `master`.*
