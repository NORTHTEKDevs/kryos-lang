# Foreign Function Interface (FFI)

Kryos can call into Python and C/C++ libraries directly. This is how you leverage the existing ecosystem -- use numpy for linear algebra, call system libraries for cryptography, integrate with any Python package or C shared library without leaving Kryos.

FFI access is gated by the capability system. You must declare `ffi` in your `kryos.toml` capabilities:

```toml
[capabilities]
allowed = ["compute", "ffi"]
```

Without this, any FFI call will be rejected at runtime.

## Python FFI

Python FFI is available in the Community tier under the `ffi:python` capability. Since the Kryos interpreter is implemented in Python, calling Python code is seamless -- any installed Python package is accessible.

### Importing a Python Module

```
@capabilities(ffi)

let math = py_import("math")
let result = py_call(math, "sqrt", 16.0)
println(to_string(result))  // 4.0
```

`py_import` takes a fully-qualified Python module name and returns a module object. The import is cached -- importing the same module twice returns the same object.

### Calling Functions

```
@capabilities(ffi)

let np = py_import("numpy")

// Call a function on the module
let arr = py_call(np, "array", [1.0, 2.0, 3.0, 4.0])

// Call a method on the returned object
let mean_val = py_call(arr, "mean")
println(to_string(mean_val))
```

`py_call` takes a module or object, a function/method name, and any number of arguments. Arguments are marshalled automatically between Kryos and Python types:

| Kryos type | Python type |
|-----------|-------------|
| `i32`, `i64` | `int` |
| `f32`, `f64` | `float` |
| `str` | `str` |
| `bool` | `bool` |
| `[T]` (array) | `list` |

### Accessing Attributes

```
@capabilities(ffi)

let sys = py_import("sys")
let path = py_attr(sys, "path")

let math = py_import("math")
let pi = py_attr(math, "pi")
println(to_string(pi))  // 3.141592653589793
```

`py_attr` retrieves an attribute from a Python module or object. This is how you access constants, class attributes, or any non-callable property.

### Practical Example: Using NumPy

```
@capabilities(ffi)

let np = py_import("numpy")

// Create arrays
let a = py_call(np, "array", [[1.0, 2.0], [3.0, 4.0]])
let b = py_call(np, "array", [[5.0, 6.0], [7.0, 8.0]])

// Matrix multiplication
let result = py_call(np, "matmul", a, b)
println(to_string(result))

// Statistical operations
let mean = py_call(np, "mean", a)
let std = py_call(np, "std", a)
println("Mean: " + to_string(mean))
println("Std:  " + to_string(std))
```

### Practical Example: Using Pandas

```
@capabilities(ffi)

let pd = py_import("pandas")

let df = py_call(pd, "read_csv", "sales_data.csv")
let summary = py_call(df, "describe")
println(to_string(summary))

// Filter rows
let filtered = py_call(df, "query", "revenue > 10000")
let count = py_attr(filtered, "shape")
println("High-revenue rows: " + to_string(count))
```

### Error Handling

If a Python module is not installed or a function call fails, Kryos raises an `FFIError` with a descriptive message:

```
FFIError: failed to import Python module 'nonexistent': No module named 'nonexistent'
FFIError: attribute 'missing_fn' not found on <module 'math'>
FFIError: error calling sqrt: math domain error
```

If you call `py_call` on something that is not callable, you get:

```
FFIError: 'pi' on <module 'math'> is not callable
```

## C FFI (Native FFI)

C FFI is available in the Pro tier under the `ffi:native` capability. It lets you load shared libraries (`.dll` on Windows, `.so` on Linux, `.dylib` on macOS) and call their exported functions with full type marshalling.

### Loading a Library

```
@capabilities(ffi)

let lib = c_load("./libmath.so")
```

`c_load` takes a path to a shared library and returns a library handle. The handle is cached -- loading the same path twice returns the same handle.

### Calling a C Function

```
@capabilities(ffi)

let lib = c_load("./libmath.so")

// c_call(library, function_name, arg_types, return_type, ...args)
let result = c_call(lib, "add", ["i32", "i32"], "i32", 3, 4)
println(to_string(result))  // 7
```

`c_call` requires you to declare the argument types and return type explicitly. This is necessary because C functions have no runtime type information -- Kryos needs to know how to marshall values across the FFI boundary.

### Type Marshalling

Kryos maps its types to C types through `ctypes`:

| Kryos type | C type | Size |
|-----------|--------|------|
| `i8` | `int8_t` | 1 byte |
| `i16` | `int16_t` | 2 bytes |
| `i32` | `int` | 4 bytes |
| `i64` | `long` / `long long` | 8 bytes |
| `u8` | `uint8_t` | 1 byte |
| `u16` | `uint16_t` | 2 bytes |
| `u32` | `unsigned int` | 4 bytes |
| `u64` | `unsigned long` / `unsigned long long` | 8 bytes |
| `f32` | `float` | 4 bytes |
| `f64` | `double` | 8 bytes |
| `bool` | `_Bool` | 1 byte |
| `str` | `char*` | pointer |
| `void` | `void` | 0 |
| `ptr` | `void*` | pointer |

Note: `i64` and `u64` map differently on Windows vs. Unix. On Windows they use `long long` / `unsigned long long` (8 bytes), while on Unix they use `long` / `unsigned long`. Kryos handles this automatically.

### Fixed-Size Arrays

You can pass fixed-size arrays to C functions using the `[T; N]` notation in the type list:

```
@capabilities(ffi)

let lib = c_load("./libvec.so")

// Pass a fixed array of 3 i32 values
let result = c_call(lib, "sum_array", ["[i32; 3]"], "i32", [1, 2, 3])
```

### String Handling

Strings are automatically marshalled:

- **Kryos to C**: Kryos `str` values are encoded to UTF-8 bytes before passing to C as `char*`
- **C to Kryos**: `char*` return values are decoded from UTF-8 back to Kryos strings

```
@capabilities(ffi)

let lib = c_load("./libgreet.so")
let msg = c_call(lib, "greet", ["str"], "str", "World")
println(msg)  // "Hello, World!"
```

### Practical Example: Calling System Libraries

```
@capabilities(ffi)

// On Linux, call libc directly
let libc = c_load("libc.so.6")

// Get process ID
let pid = c_call(libc, "getpid", [], "i32")
println("PID: " + to_string(pid))

// Get current time
let time_val = c_call(libc, "time", ["ptr"], "i64", 0)
println("Unix timestamp: " + to_string(time_val))
```

### Practical Example: Using a Crypto Library

```
@capabilities(ffi)

let sodium = c_load("libsodium.so")

// Initialize the library
let init_result = c_call(sodium, "sodium_init", [], "i32")
if init_result < 0 {
    println("Failed to initialize libsodium")
}

// Generate random bytes
let buf_size = 32
let random_buf = c_call(sodium, "randombytes_buf", ["ptr", "u32"], "void", 0, buf_size)
```

## FFI Registry

Behind the scenes, Kryos maintains a global FFI registry that caches all loaded modules and libraries. This means:

- Importing `numpy` twice does not re-import it
- Loading `libsodium.so` twice does not reload the library
- You can check what is loaded with the registry's introspection

The registry tracks two categories:
- **Python modules**: keyed by module name (e.g., `"numpy"`, `"math"`)
- **Native libraries**: keyed by file path (e.g., `"./libmath.so"`)

## Safety Considerations

FFI is inherently unsafe. You are calling into code that Kryos cannot verify, type-check, or memory-manage. A few things to keep in mind:

**Python FFI** is relatively safe because Python has its own garbage collector and type system. The main risk is runtime exceptions from bad arguments.

**C FFI** is dangerous. You can:
- Crash the process with a segfault if you pass wrong types
- Corrupt memory if you pass wrong array sizes
- Leak memory if the C function allocates and you do not free
- Introduce undefined behavior if you get the return type wrong

That is why FFI requires an explicit capability declaration. The capability system makes FFI usage visible in your `kryos.toml`, so code reviewers and auditors can quickly identify which packages interact with foreign code.

### Best Practices

1. **Minimize the FFI surface.** Wrap foreign calls in a Kryos module that exposes a clean, type-safe API. Do not scatter `c_call` throughout your codebase.

2. **Validate inputs before crossing the boundary.** Check array lengths, null pointers, and string encoding before passing them to C.

3. **Declare foreign dependencies in `kryos.toml`.** This documents what your project needs and helps `kryos install` report the right instructions.

```toml
[dependencies.python]
numpy = "1.26"

[dependencies.c]
libsodium = "1.0.18"

[capabilities]
allowed = ["compute", "ffi"]
```

4. **Test FFI code in isolation.** Write focused tests for your FFI wrappers so failures are obvious and localized.

5. **Prefer the Python FFI for prototyping.** It is safer and easier. Move to C FFI only when you need the performance.
