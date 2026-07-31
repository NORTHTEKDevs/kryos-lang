# Foreign Function Interface (FFI)

> **Implementation Status (corrected -- verified against this commit, not
> the earlier draft of this page):** `extern` blocks (with optional ABI
> string, defaulting to `"C"`) parse, type-check, and compile through both
> backends -- but **calling an ARBITRARY real C-library function through one
> is not reliably supported today**, despite what earlier versions of this
> page (and its own examples) claimed. What actually works:
>
> - The `kryos_*`-prefixed runtime symbols that back the documented stdlib
>   builtins genuinely link and run -- but you reach them by calling the
>   ordinary Kryos builtin/stdlib function, **not** by hand-declaring your
>   own `extern` block against a `kryos_*` name (a hand-declared `kryos_*`
>   extern with a `str`/heap-typed signature **crashes** -- it calls the raw
>   symbol without the marshalling the real builtin path applies; see
>   CLAUDE.md gotcha #22). This is why "the runtime provides 100+ FFI
>   functions" is true of the *stdlib surface*, not of what you can safely
>   `extern`-declare yourself.
> - An extern name that happens to COLLIDE with a Kryos builtin (`sin`,
>   `cos`, `pow`, `sqrt`, `abs`, ...) gets intercepted by the builtin
>   fast-path regardless of your `extern` declaration -- it "works," but it
>   is calling the KRYOS builtin, not your declared foreign function. Do not
>   read a working `extern "C" { fn sqrt(x: f64) -> f64 }` example as proof
>   that arbitrary C-library FFI links; it proves the opposite (name
>   collision, not linking).
> - A genuinely foreign, non-`kryos_*`, non-builtin-colliding C symbol is
>   inconsistent: some fail the AOT build outright with "use of undefined
>   value" (`getpid`, `strlen`, and even libc's own `sqlite3_libversion_number`
>   analog all reproduce this on this platform), and at least one
>   (`puts`) BUILDS AND RUNS but silently does not produce the call's
>   effect at all -- `puts("hello from Kryos")` exits 0 and prints nothing,
>   which is worse than a link failure because nothing signals the problem.
>   **Do not rely on calling your own C functions via `extern` until real
>   FFI emission lands** (documented in CLAUDE.md gotcha #22 as "the
>   extern's param/symbol info isn't threaded to codegen").
> - Custom link flags in `kryos.toml` (`[build] link = [...]`) are **not
>   implemented** -- the "Linking" section below describing them is
>   aspirational, not current behavior.
> - `kryos bindgen <header.h>` **is implemented** and works (generates real
>   `extern "C" { ... }` declarations from a header) -- an earlier draft of
>   this page said the opposite; that was wrong in the other direction.

Kryos can DECLARE calls into C libraries and system functions using `extern`
blocks, and the declarations type-check and pass through codegen -- but
reliably calling into an arbitrary real C library is not there yet (see
above). Treat the rest of this page's worked examples as illustrating the
INTENDED shape of the feature, verified per-example below, not as a
guarantee they produce correct output for a library of your choosing.

FFI access is gated by the capability system. Declare `ffi` in your `kryos.toml` capabilities:

```toml
[capabilities]
allowed = ["compute", "ffi"]
```

Without this, extern declarations will be rejected.

## Extern Blocks

Declare foreign functions inside an `extern "C"` block. This tells the compiler that these functions follow the C calling convention and will be provided at link time:

```
extern "C" {
    fn puts(s: str) -> i32
    fn sqrt(x: f64) -> f64
    fn abs(x: i32) -> i32
}

fn main() {
    puts("hello from Kryos")   // VERIFIED: builds, exits 0, prints NOTHING -- silently wrong, not a real puts call
    let root = sqrt(144.0)     // VERIFIED: works, but only because `sqrt` collides with the Kryos builtin
    let positive = abs(-42)    // VERIFIED: FAILS to build ("defined with type 'i64' but expected 'i32'")
}
```

Each function inside the extern block is a declaration only -- no body. In
principle the linker resolves the symbol at build time against system
libraries or any libraries you link with; in practice, see the status note
at the top of this page before assuming any specific symbol will link
correctly, let alone marshal its arguments/return correctly.

## Type Marshalling

Kryos types map to C types across the FFI boundary:

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
| `str` | `char*` | pointer |

All values cross the boundary as their native representation. Strings are passed as null-terminated UTF-8 `char*` pointers.

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

## Practical Example: Math Library (works, but NOT because of real FFI linking)

```
extern "C" {
    fn sin(x: f64) -> f64
    fn cos(x: f64) -> f64
    fn pow(base: f64, exp: f64) -> f64
}

fn main() {
    let angle = 3.14159 / 4.0
    println(to_string(sin(angle)))   // VERIFIED: prints ~0.707
    println(to_string(cos(angle)))   // VERIFIED: prints ~0.707
    println(to_string(pow(2.0, 10.0))) // VERIFIED: prints 1024
}
```

This example genuinely builds and runs -- but read the status note at the
top of this page first: `sin`/`cos`/`pow` are also Kryos ambient builtins,
so this is calling the BUILTIN under a name that happens to match your
`extern` declaration, not proof that `extern "C"` reliably reaches an
arbitrary C library function. Renaming the extern block to a genuinely
foreign symbol is not guaranteed to produce the same result -- see the next
example.

## Practical Example: System Calls (FAILS to link today)

```
extern "C" {
    fn getpid() -> i32
    fn getenv(name: str) -> str
}

fn main() {
    let pid = getpid()          // VERIFIED: AOT build fails -- "use of undefined value '@getpid'"
    println("PID: " + to_string(pid))
}
```

This does not build today. It is left here, marked as broken, because it is
exactly the shape of code a reasonable person would try first -- better to
show it failing with the real error than to omit it and let someone hit the
same wall with no warning.

## Safety Considerations

FFI is inherently unsafe. You are calling into code that Kryos cannot verify, type-check, or memory-manage. A few things to keep in mind:

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
