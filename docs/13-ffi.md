# Foreign Function Interface (FFI)

Kryos can call into C libraries and system functions directly using `extern` blocks. This is how you leverage the existing native ecosystem -- call system libraries, use crypto routines, integrate with any C-ABI shared library.

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
    puts("hello from Kryos")
    let root = sqrt(144.0)    // 12.0
    let positive = abs(-42)   // 42
}
```

Each function inside the extern block is a declaration only -- no body. The linker resolves the symbol at build time against system libraries or any libraries you link with.

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

## Linking

When you build with `kryos build`, extern symbols are resolved at link time. By default, the linker searches the system C library (libc). To link against additional libraries, use `-l` flags in your `kryos.toml`:

```toml
[build]
link = ["-lm", "-lsodium"]
```

Or pass them on the command line:

```bash
kryos build main.kry -- -lm -lsodium
```

## Practical Example: Math Library

```
extern "C" {
    fn sin(x: f64) -> f64
    fn cos(x: f64) -> f64
    fn pow(base: f64, exp: f64) -> f64
}

fn main() {
    let angle = 3.14159 / 4.0
    println(to_string(sin(angle)))   // ~0.707
    println(to_string(cos(angle)))   // ~0.707
    println(to_string(pow(2.0, 10.0))) // 1024.0
}
```

## Practical Example: System Calls

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

## Safety Considerations

FFI is inherently unsafe. You are calling into code that Kryos cannot verify, type-check, or memory-manage. A few things to keep in mind:

- **Wrong types crash the process.** If you declare `fn sqrt(x: i32) -> i32` but the actual C function expects `double`, you get undefined behavior -- a segfault, garbage values, or worse.
- **String lifetime matters.** Kryos strings passed to C are valid for the duration of the call. Do not store the `char*` pointer on the C side beyond the call.
- **Memory is your responsibility.** If a C function allocates memory, you must call the corresponding free function. Kryos does not track foreign allocations.

That is why FFI requires an explicit capability declaration. The capability system makes FFI usage visible in `kryos.toml`, so code reviewers can identify which packages interact with foreign code.

### Best Practices

1. **Minimize the FFI surface.** Wrap foreign calls in a Kryos module that exposes a clean, type-safe API. Do not scatter `extern` declarations throughout your codebase.

2. **Validate inputs before crossing the boundary.** Check array lengths and string encoding before passing them to C.

3. **Declare link dependencies in `kryos.toml`.** This documents what your project needs.

```toml
[build]
link = ["-lsodium"]

[capabilities]
allowed = ["compute", "ffi"]
```

4. **Test FFI code in isolation.** Write focused tests for your FFI wrappers so failures are obvious and localized.
