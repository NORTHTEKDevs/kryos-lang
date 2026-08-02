# Foreign Function Interface (FFI)

> **Implementation Status (corrected again -- this page previously said
> unsupported externs "parse, type-check, and compile through both
> backends," which was true but was itself the bug: they compiled clean and
> then failed unpredictably at link time, at codegen time, or -- worst case
> -- silently produced no effect at runtime. As of this compiler, the
> unsupported shapes below are REJECTED AT CHECK TIME (`error[E0508]`,
> `kryos explain E0508`) instead of being allowed to compile.** What this
> means concretely:
>
> - **Arbitrary C-library FFI (any extern name that does not start with
>   `kryos_`) is rejected at `kryos check`/`run`/`build`.** It was never
>   reliably supported -- the extern's param/symbol info was never threaded
>   to codegen, so `extern "C" { fn getpid() -> i32 }` failed AOT codegen
>   with "use of undefined value '@getpid'", `extern "C" { fn abs(x: i32)
>   -> i32 }` failed AOT with a confusing type mismatch (and only "worked"
>   on `kryos run` via an unrelated builtin-name collision), and `extern "C"
>   { fn puts(s: str) -> i32 }` built, ran, and printed NOTHING -- a silent
>   wrong answer with no diagnostic at all. All three are now a clear
>   compile-time error instead of a downstream surprise: `error[E0508]:
>   extern function \`getpid\` is not a \`kryos_*\` runtime symbol --
>   arbitrary C-library FFI is not implemented by this compiler`.
> - The `kryos_*`-prefixed runtime symbols that back the documented stdlib
>   builtins genuinely link and run -- but you reach them by calling the
>   ordinary Kryos builtin/stdlib function, **not** by hand-declaring your
>   own `extern` block against a `kryos_*` name. **A hand-declared `kryos_*`
>   extern with a `str`/array/map-typed signature is now ALSO rejected at
>   check time** (same E0508), because it used to compile clean and
>   SEGFAULT both backends at runtime: the real native symbol expects raw
>   pointer/length pairs (e.g. `kryos_env_get(key_ptr: i64, key_len: i64,
>   val_buf: i64, val_buf_len: i64) -> i64`, per `std::os`), not a Kryos
>   `str` handle, and the hand-declared version called the raw symbol
>   without that marshalling. Use the documented builtin (`env_get(...)`)
>   instead. A small allowlist of names the compiler is verified to marshal
>   correctly (`kryos_builtin_to_upper`/`to_lower`,
>   `kryos_ffi_dlopen`/`dlsym`/`cstr`/`strlen`/`string_from_ptr`) is exempt,
>   since the stdlib itself hand-declares exactly those with `str` types.
>   An i64/i32/f64/ptr-only `kryos_*` extern (matching the real symbol's raw
>   ABI, as `compiler/stdlib/os.kry` demonstrates) is unaffected and still
>   works.
> - Custom link flags in `kryos.toml` (`[build] link = [...]`) are **not
>   implemented** -- the "Linking" section below describing them is
>   aspirational, not current behavior.
> - `kryos bindgen <header.h>` **is implemented** and works (generates real
>   `extern "C" { ... }` declarations from a header) -- those declarations
>   are still subject to the same E0508 rejection above once you try to
>   compile against them, since bindgen only generates the declaration, not
>   real linking support.

Kryos can DECLARE calls into C libraries and system functions using `extern`
blocks -- but **the compiler now rejects, at check time, both of the
unsupported shapes documented above** rather than letting them compile and
fail (or silently misbehave) later. Treat the rest of this page's worked
examples as illustrating the shape of the feature and its FAILURE MODE
(each is now a compile-time E0508, not a runtime surprise) -- not as
working code to copy.

FFI access is gated by the capability system. Declare `ffi` in your `kryos.toml` capabilities:

```toml
[capabilities]
allowed = ["compute", "ffi"]
```

Without this, extern declarations will be rejected.

## Extern Blocks

Declare foreign functions inside an `extern "C"` block. This tells the compiler that these functions follow the C calling convention and will be provided at link time -- **but any name that does not start with `kryos_` is rejected at check time (E0508), before it ever reaches the linker:**

```
extern "C" {
    fn puts(s: str) -> i32
    fn sqrt(x: f64) -> f64
    fn abs(x: i32) -> i32
}

fn main() {
    puts("hello from Kryos")
    let root = sqrt(144.0)
    let positive = abs(-42)
}
```

VERIFIED: `kryos check`/`run`/`build` all reject this with three `E0508`
errors, one per declared name -- `error[E0508]: extern function \`puts\` is
not a \`kryos_*\` runtime symbol -- arbitrary C-library FFI is not
implemented by this compiler` (and identically for `sqrt`/`abs`). This
replaces the previous behavior, where the block compiled clean and each
call misbehaved differently at runtime: `puts` printed nothing at all,
`sqrt` silently called the Kryos builtin instead of libm, and `abs` failed
AOT codegen with a confusing type mismatch. None of that ambiguity is
reachable anymore -- the extern block itself is rejected.

Each function inside the extern block is a declaration only -- no body. In
principle the linker resolves the symbol at build time against system
libraries or any libraries you link with; in practice, no non-`kryos_*`
extern reaches the linker at all today (see the status note at the top of
this page).

## Type Marshalling

An `extern` signature is restricted to the types below (E0508 rejects
anything else -- `str`, arrays, maps, structs/enums/tuples, `fn` -- outside
the small compiler-verified allowlist noted in the status note above,
because Kryos's own representation for those types is a heap handle, not
the C bit pattern the row implies):

| Kryos type | C type | Size |
|-----------|--------|------|
| `i8` | `int8_t` | 1 byte |
| `i16` | `int16_t` | 2 bytes |
| `i32` | `int` / `int32_t` | 4 bytes |
| `i64` | `long long` / `int64_t` | 8 bytes |
| `u8` | `uint8_t` | 1 byte |
| `u16` | `uint16_t` | 2 bytes |
| `u32` | `uint32_t` | 4 bytes |
| `u64` | `uint64_t` | 8 bytes |
| `f32` | `float` | 4 bytes |
| `f64` | `double` | 8 bytes |
| `bool` | `_Bool` | 1 byte |
| `ptr` / `*T` | `void*` / `T*` | pointer |

`str` is NOT in this table on purpose: a Kryos `str` is a heap handle
(pointer to a length-prefixed, refcounted `KryosString`), not a bare
`char*`. To pass string data across a raw extern boundary, convert
explicitly with the `str_to_ptr(s) -> i64` / `len(s) -> i64` builtins on the
way in and `buf_to_str(ptr, len) -> str` on the way out (see
`compiler/stdlib/os.kry`'s `_env_or_empty` for the canonical pattern) --
declaring the extern parameter itself as `str` is rejected (E0508) because
it skips that conversion and reads/writes through the wrong pointer shape.

## Linking (ASPIRATIONAL -- `[build] link` is not implemented)

The intended design: extern symbols would be resolved at link time, with the
linker searching the system C library (libc) by default and `-l` flags in
`kryos.toml` pulling in additional libraries:

```toml
[build]
link = ["-lm", "-lsodium"]   # NOT IMPLEMENTED -- this key has no effect today
```

Command-line `-- -lm -lsodium`-style passthrough is likewise not available
today. There is currently no supported way to link an additional system or
third-party C library from a `kryos build` invocation.

## Practical Example: Math Library (rejected -- use the builtins directly)

```
extern "C" {
    fn sin(x: f64) -> f64
    fn cos(x: f64) -> f64
    fn pow(base: f64, exp: f64) -> f64
}

fn main() {
    let angle = 3.14159 / 4.0
    println(to_string(sin(angle)))
    println(to_string(cos(angle)))
    println(to_string(pow(2.0, 10.0)))
}
```

VERIFIED: rejected with `E0508` on all three declarations. This used to
"work" -- but only because `sin`/`cos`/`pow` are also Kryos ambient
builtins, so it was silently calling the BUILTIN under a name that
happened to match the `extern` declaration, not real FFI. Since `sin`,
`cos`, `pow`, and `sqrt` are already ambient Kryos builtins (see "Builtins
available everywhere" in CLAUDE.md), there is no need for an `extern`
block at all -- call them directly: `sin(angle)`, no import, no capability.

## Practical Example: System Calls (rejected, not a link failure anymore)

```
extern "C" {
    fn getpid() -> i32
    fn getenv(name: str) -> str
}

fn main() {
    let pid = getpid()
    println("PID: " + to_string(pid))
}
```

VERIFIED: rejected with `error[E0508]: extern function \`getpid\` is not a
\`kryos_*\` runtime symbol...` at `kryos check`, before any codegen or
linking is attempted. This used to compile clean and fail deep in the AOT
backend with a cryptic `use of undefined value '@getpid'` -- kept here,
still marked as broken, because it is exactly the shape of code a
reasonable person would try first; the point now is that the failure is
immediate and names the real limitation instead of surfacing as a linker
error three stages later.

## Safety Considerations

The points below describe FFI risk in the general/abstract sense (useful if
real C-library linking lands later); in THIS compiler today, most of them
are moot in practice because the unsupported shapes that would trigger them
are rejected at check time (E0508) before you can ever run them. FFI is
inherently unsafe. You are calling into code that Kryos cannot verify, type-check, or memory-manage. A few things to keep in mind:

- **Wrong types crash the process.** If you declare `fn sqrt(x: i32) -> i32` but the actual C function expects `double`, you get undefined behavior -- a segfault, garbage values, or worse.
- **String lifetime matters.** Kryos strings passed to C are valid for the duration of the call. Do not store the `char*` pointer on the C side beyond the call.
- **Memory is your responsibility.** If a C function allocates memory, you must call the corresponding free function. Kryos does not track foreign allocations.

That is why FFI requires an explicit capability declaration. The capability system makes FFI usage visible in `kryos.toml`, so code reviewers can identify which packages interact with foreign code.

### Best Practices

1. **Minimize the FFI surface.** Wrap foreign calls in a Kryos module that exposes a clean, type-safe API. Do not scatter `extern` declarations throughout your codebase.

2. **Validate inputs before crossing the boundary.** Check array lengths and string encoding before passing them to C.

3. ~~Declare link dependencies in `kryos.toml`.~~ **Not available today** --
   `[build] link` is not implemented (see the status note at the top of this
   page). Until it lands, there is no supported way to pull in an additional
   C library from `kryos build`; genuine third-party FFI is not usable yet
   regardless of how it's declared.

```toml
[capabilities]
allowed = ["compute", "ffi"]
```

4. **Test FFI code in isolation.** Write focused tests for your FFI wrappers so failures are obvious and localized.
