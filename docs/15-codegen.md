# Compilation Pipeline

Kryos is a compiled language. Every program goes through the same pipeline: lexing, parsing, type checking, ownership analysis, capability checking, MIR lowering, code generation, and linking. There is no interpreter. Everything compiles to native code.

Debug builds use the **Cranelift** backend for fast compilation. Release builds (`--release`) use the **LLVM** backend for optimized native binaries.

## The Pipeline

Source code goes through eight stages to become a native binary:

```
.kry source
    |
    v
  Lexer           -> token stream
    |
    v
  Parser          -> AST (abstract syntax tree)
    |
    v
  Type Checker    -> type-annotated AST (inference + checking)
    |
    v
  Ownership       -> verified ownership/borrowing
    |
    v
  Capabilities    -> capability audit (deny-by-default)
    |
    v
  MIR Lowering    -> MIR (SSA basic blocks + terminators)
    |
    v
  Codegen         -> Cranelift (debug) or LLVM IR (release)
    |
    v
  Linker          -> native binary (executable)
```

The entire pipeline is implemented in Rust across 21 crates. No external tooling is needed for debug builds. Release builds shell out to the LLVM toolchain (`llc` and `clang`).

## Building a Program

```bash
kryos build hello.kry
```

This runs the full pipeline and produces an executable. By default, the output is named after the source file (e.g., `hello` or `hello.exe` on Windows). Use `-o <path>` to override.

### Debug builds (default -- Cranelift)

```bash
kryos build hello.kry
```

Cranelift generates native code directly -- no external tools required. Compilation is fast but the generated code is not heavily optimized. This is the default for development.

### Release builds (LLVM)

```bash
kryos build hello.kry --release
```

The LLVM backend generates optimized LLVM IR, then invokes `llc` and `clang` to produce the final binary. This path produces faster executables but takes longer to compile. Requires LLVM installed on your system.

If `llc` or `clang` are not found on your PATH, the build fails with a descriptive message:

```
llc not found -- install LLVM to compile with --release
clang not found -- install LLVM/Clang to link the binary
```

### Inspecting intermediate output

```bash
kryos build hello.kry --emit-mir      # dump MIR (mid-level IR)
kryos build hello.kry --emit-llvm     # dump LLVM IR text (release mode)
kryos build hello.kry --verbose        # print each pipeline stage
```

The `--emit-mir` flag prints the MIR for every function -- useful for understanding how the compiler lowers your code before codegen. The `--emit-llvm` flag prints the LLVM IR text (only meaningful with `--release`).

## MIR: The Mid-Level IR

After type checking, ownership analysis, and capability checking, the AST is lowered to MIR (Mid-level Intermediate Representation). MIR is an SSA-style IR organized into basic blocks with explicit terminators.

Each function in MIR consists of:
- **Basic blocks** -- labeled sequences of statements
- **Statements** -- assignments, stores, calls
- **Terminators** -- branches, returns, switches

MIR serves as the single point of truth for both backends. Cranelift and LLVM codegen both consume MIR, not the AST.

## How the Codegen Works

### The Uniform Slot Model

All values in the codegen use **8-byte (i64) uniform slots**. This is a deliberate simplification: every local variable, function parameter, struct field, and enum field occupies exactly one i64-sized slot.

| Kryos type | Codegen representation |
|------------|----------------------|
| `i8`, `i16`, `i32`, `i64` | i64 (zero-extended or sign-extended to 64 bits) |
| `u8`, `u16`, `u32`, `u64` | i64 |
| `i128`, `u128` | i64 (truncated -- full 128-bit support is planned) |
| `f32`, `f64` | i64 (bit-cast via reinterpret) |
| `bool` | i64 (0 = false, 1 = true) |
| `str` | i64 (opaque handle to `kryos_string` runtime) |
| `char` | i64 |
| `void` | void |
| Arrays | i64 (pointer to heap-allocated runtime array) |
| Maps | i64 (pointer to heap-allocated runtime map) |
| Structs | i64 (pointer to heap-allocated uniform slot array) |
| Enums | i64 (pointer to heap-allocated [tag, field0, field1, ...]) |
| Closures | i64 (function pointer or pointer to closure environment) |

This uniform model means the codegen does not need to track sizes or alignments for most operations. Aggregate types (strings, arrays, maps, structs, enums) are always accessed through pointers, with the actual data managed by runtime functions.

### Functions

Each `fn` declaration becomes a native function. Parameters and locals are allocated as i64 slots. The compiler wraps `fn main()` as the program entry point.

```
fn add(a: i32, b: i32) -> i32 {
    return a + b
}
```

In the LLVM backend, this generates:

```llvm
define i64 @add(i64 %a, i64 %b) {
entry:
  %0 = add i64 %a, %b
  ret i64 %0
}
```

Note: all parameters and return values are i64, regardless of the declared Kryos type.

### Higher-Order Functions

Functions can be passed as arguments. When a function name like `double` is used as a value (not called), it compiles to a `func_addr` instruction that produces the function's native address as an i64 pointer.

```
fn double(x: i32) -> i32 { return x * 2 }
fn apply(f: fn(i32) -> i32, x: i32) -> i32 { return f(x) }
fn main() { println(apply(double, 5)) }
```

In the MIR, `double` used as a value becomes a closure with no captures:

```
_1 = closure(double, [])      // get function pointer
_0 = call apply(_1, 5)        // pass it to apply
```

Inside `apply`, the indirect call compiles to `call_indirect`:

```
_2 = call_indirect _0(_1)     // call through function pointer
```

The codegen uses Cranelift's `call_indirect` instruction with an all-i64 signature. Arguments are widened to i64 before the call to match the uniform slot model.

### Integer Widening

The type checker allows implicit widening between integer types of the same sign family. Signed integers widen: i8 -> i16 -> i32 -> i64 -> i128. Unsigned integers widen: u8 -> u16 -> u32 -> u64 -> u128.

This means `let x: i64 = 42` works without an explicit cast -- the i32 literal `42` is automatically widened to i64. The widening has zero runtime cost because the codegen already stores all values as i64 slots.

### Entry Point

Every Kryos program must define `fn main()`. The compiler wraps it as the process entry point:

```llvm
define i32 @main() {
entry:
  call void @kryos_main()
  call void @kryos_spawn_wait_all()
  ret i32 0
}
```

Your `fn main()` is compiled as `@kryos_main` and called from the real `@main`. The `kryos_spawn_wait_all()` call ensures any threads launched by `spawn` complete before the process exits.

### Strings

Strings are opaque handles managed by the `kryos_string` runtime. String literals are interned at compile time. All string operations go through runtime functions:

- `kryos_string_new(data, len)` -- create a KryosString from raw bytes
- `kryos_string_concat(a, b)` -- concatenate two strings, returns a new handle
- `kryos_string_eq(a, b)` -- compare two strings for equality, returns 1 or 0
- `kryos_string_len(s)` -- return the byte length
- `kryos_string_free(s)` -- free the string's heap allocation

String literals compile to a `kryos_string_new` call that wraps the raw data section pointer into a proper KryosString handle. Every string value in the codegen is a KryosString handle pointer -- never a raw C string.

String concatenation with `+` compiles to a chain of `kryos_string_concat()` calls:

```
let greeting = "Hello, " + name + "!"
```

Becomes:

```
tmp0 = kryos_string_concat("Hello, ", name)
greeting = kryos_string_concat(tmp0, "!")
```

### Arrays

Arrays are heap-allocated and managed by runtime functions:

- `kryos_array_new()` -- create an empty array
- `kryos_array_push(arr, val)` -- append a value
- `kryos_array_get(arr, idx)` -- get a value by index
- `kryos_array_set(arr, idx, val)` -- set a value by index
- `kryos_array_len(arr)` -- return the length

Array literals compile to `kryos_array_new()` followed by a sequence of `kryos_array_push()` calls:

```
let xs = [1, 2, 3]
```

Becomes:

```
xs = kryos_array_new()
kryos_array_push(xs, 1)
kryos_array_push(xs, 2)
kryos_array_push(xs, 3)
```

### Maps

Map literals compile to `kryos_map_new()` followed by insert calls. The codegen detects whether keys are strings or integers and routes to the correct runtime function:

**Integer-key maps:**

```
let m = { 1: "a", 2: "b" }
```

Becomes:

```
m = kryos_map_new()
kryos_map_insert(m, 1, "a")
kryos_map_insert(m, 2, "b")
```

**String-key maps:**

```
let m = { "name": "Kryos", "version": 1 }
```

Becomes:

```
m = kryos_map_new()
kryos_map_insert_str(m, "name", "Kryos")
kryos_map_insert_str(m, "version", 1)
```

String-key maps use content-based hashing (FNV-1a) through `kryos_string_hash` so that two strings with the same content always map to the same bucket, regardless of whether they are the same pointer. Lookups use `kryos_map_get_str`, which also hashes by content and compares strings byte-by-byte for equality.

Integer-key maps use the raw i64 value as the hash key directly.

### Structs

Structs are heap-allocated as arrays of uniform 8-byte slots. Each field occupies exactly one i64 slot, accessed by index:

```
struct Point {
    x: f64,
    y: f64
}
```

A `Point` is allocated as a 2-slot array: `[slot0: i64, slot1: i64]`. Field `x` is at index 0, field `y` is at index 1. Field access compiles to a pointer offset and load.

Methods in `impl` blocks are compiled as regular functions with a mangled name: `TypeName_methodName`. The `self` parameter is a pointer to the struct's slot array.

```
impl Point {
    fn get_x(self) -> f64 {
        return self.x
    }
}
```

Compiles as `Point_get_x(self_ptr)` where `self_ptr` points to the slot array.

### Enums

Enums use a uniform slot layout: `[tag: i64, field0: i64, field1: i64, ...]`. The tag is always at index 0. Payload fields follow in declaration order, with the allocation sized to the largest variant.

```
enum Shape {
    Circle(f64),
    Rect(f64, f64)
}
```

`Shape` is allocated as 3 slots: `[tag, field0, field1]`. `Circle(r)` sets tag=0, field0=r. `Rect(w, h)` sets tag=1, field0=w, field1=h.

The `match` expression on enums compiles to a switch on the tag value, with each arm extracting fields by slot index.

### Lambda Expressions and Inner Functions

Lambdas use `fn(params) -> RetType { body }` syntax. Inner named functions (`fn name(params) -> RetType { body }` inside a function body) are desugared to lambdas at parse time.

**No captures** -- the lambda compiles to a plain function pointer stored as an i64:

```
let f = fn(x: i32) -> i32 { return x * 2 }
```

Generates a top-level function (`__lambda_0`) and stores its pointer in `f`. Anonymous functions without a return type annotation default to returning i64.

**With captures** -- the compiler detects free variables in the lambda body. Captured variables become extra parameters prepended to the function signature:

```
fn make_adder(x: i64) -> i64 {
    fn adder(y: i64) -> i64 {
        return x + y
    }
    return adder(10)
}
```

The inner function `adder` captures `x` from the enclosing scope. The compiler:
1. Generates `__lambda_0(x, y)` with `x` as an extra parameter
2. At the call site, emits `call __lambda_0(x_local, 10)` -- a direct call with captures prepended

The MIR for the call to `adder(10)`:

```
_1 = closure(__lambda_0, [_0])   // _0 is the captured x
_2 = call __lambda_0(_0, 10)     // direct call with capture prepended
```

When the compiler can resolve the closure target at compile time, it emits a direct call instead of an indirect call. This avoids the overhead of closure environment allocation.

### String Interpolation

String interpolation (`"hello {name}"`) compiles to a series of string concatenation calls. The lexer tokenizes interpolated strings into alternating literal parts and expression parts. The MIR lowers these to `StringConcat` instructions that build the result left to right.

```
let name = "world"
println("hello {name}")
```

The MIR for the interpolation:

```
_1 = "world"
_2 = ""                          // start with empty string
_3 = string_concat(_2, "hello ") // append literal part
_4 = string_concat(_3, _1)      // append expression value (name)
```

Each expression inside `{}` is evaluated, converted to a string if needed (using `kryos_i64_to_string`, `kryos_f64_to_string`, etc.), and concatenated into the result. The conversion is automatic -- you can interpolate integers, floats, bools, and strings without calling `to_string()`.

To include a literal `{` or `}` in a string, use the escape sequences `\{` and `\}`.

### Printing

`println`, `print`, and `eprintln` accept any type. The codegen determines the operand type and routes to the correct conversion function before calling the string-based print function:

| Operand type | Conversion | Print call |
|-------------|-----------|-----------|
| `str` | none (already a KryosString handle) | `kryos_println_str(handle)` |
| `bool` | `kryos_bool_to_string(val)` | `kryos_println_str(result)` |
| `f64` | `kryos_f64_to_string(val)` | `kryos_println_str(result)` |
| `i32`, `i64`, other ints | `kryos_i64_to_string(val)` | `kryos_println_str(result)` |

This means you can write `println(42)`, `println(true)`, or `println(3.14)` without calling `to_string()` first. The codegen handles the conversion automatically.

### String Comparison

The `==` and `!=` operators work on strings. When the codegen detects that either operand is a string type, it routes to `kryos_string_eq()` instead of integer comparison:

```
let a = "hello"
let b = "hello"
if a == b {
    println("match")
}
```

`kryos_string_eq` compares both the length and byte content of the two KryosString handles. Returns 1 for equal, 0 for not equal. The `!=` operator inverts the result.

### String Memory Management

String locals receive automatic cleanup. When a string local goes out of scope, the codegen emits a `kryos_string_free` call to release the heap allocation. This is driven by `Instruction::Drop` in MIR -- the ownership system determines when each local's lifetime ends, and the codegen translates that into a free call for string-typed locals.

### Ranges

Range expressions compile to a 24-byte stack allocation with three i64 fields:

```
[start: i64, end: i64, inclusive: i64]
```

`range(0, 10)` produces `[0, 10, 0]` (exclusive). `range_inclusive(0, 10)` produces `[0, 10, 1]`.

Both `for x in range(start, end)` and `for x in start..end` compile to the same counter-based loop. The MIR lowering detects range expressions and routes them to the optimized counter-loop path:

```
for.init:
  counter = start
for.cond:
  cmp = counter < end
  branch cmp -> for.body, for.end
for.body:
  x = counter
  ; body
  branch -> for.inc
for.inc:
  counter = counter + 1
  branch -> for.cond
for.end:
```

### Control Flow

**`if`/`elif`/`else`** compiles to conditional branches. Note: the keyword is `elif`, not `else if`.

**`while` loops** use a condition block, body block, and end block.

**`break` and `continue`** compile to unconditional branches to the loop's end block or condition block, respectively.

**`match` expressions** -- match arms are expressions (`pattern => expr`). Simple matches compile to a chain of comparisons. Enum matches compile to a switch on the tag. Match is an expression -- it produces a value.

### Try/Catch

`try`/`catch`/`throw` compiles to Result-enum-based control flow -- no stack unwinding. The compiler desugars try/catch into a switch on a `Result` enum with `Ok` (tag 0) and `Err` (tag 1) variants.

```
try {
    throw 99
    println("unreachable")
} catch e {
    println("caught")
}
```

The MIR for this:

```
// try block body
_1 = 99
_0 = enum_variant(Result, 1, [_1])    // Result::Err(99)
goto check_bb                          // skip remaining stmts

check_bb:
  _2 = enum_tag(_0)                    // extract tag
  switch _2: 0 -> ok_bb, default -> err_bb

ok_bb:
  _3 = enum_payload(_0, 0, 0)         // extract Ok value
  goto merge_bb

err_bb:
  e = enum_payload(_0, 1, 0)          // extract Err value → binds to `e`
  call println("caught")
  goto merge_bb

merge_bb:
  // execution continues
```

When the try block completes without a throw, the last expression is wrapped in `Result::Ok`. When `throw` executes, it stores `Result::Err(value)` in the result local and jumps directly to the tag-check block, skipping any remaining statements in the try block.

Nested try/catch is supported. Each level maintains its own result local and check block. A throw inside a catch block propagates to the next outer try/catch.

### Generics (Monomorphization)

Generic functions are compiled by monomorphization -- each unique combination of type arguments produces a specialized copy. Since all types use i64 slots, monomorphization primarily affects type-checking rather than codegen in the current implementation.

### Boolean Operators

Boolean operators use keywords: `and`, `or`, `not` (not `&&`, `||`, `!`). They compile to short-circuit evaluation -- `and` skips the right operand if the left is false, `or` skips if the left is true.

### Spawn (Concurrency)

`spawn` compiles to a thread creation call. The compiler supports two patterns:

**Spawning a function call:**

```
spawn do_work()
```

The MIR emits a `spawn` instruction that names the function directly. The codegen:
1. Gets the function's address as an i64 pointer
2. Calls `kryos_spawn(fn_ptr, args_ptr, arg_count)` from the runtime

**Spawning a block:**

```
spawn {
    println("background work")
}
```

The compiler generates a synthetic wrapper function (`__spawn_0`, `__spawn_1`, etc.) for the block body. Captured variables from the enclosing scope become parameters to the wrapper. The MIR looks like:

```
spawn __spawn_0(captured_var1, captured_var2)
```

The runtime's `kryos_spawn` function:
1. Copies the argument values (all i64 slots)
2. Creates an OS thread via `std::thread::spawn`
3. Dispatches to the function pointer with the correct number of arguments
4. Stores the thread handle in a global registry

At program exit, `kryos_spawn_wait_all()` joins all spawned threads, ensuring they complete before the process terminates.

### Sleep

`sleep(seconds)` pauses the current thread. The argument is a float (f64). The codegen bitcasts the f64 value to i64 bits and calls `kryos_sleep(bits)`, which reconstructs the f64 and calls `std::thread::sleep`.

```
sleep(0.5)  // sleep 500 milliseconds
sleep(1.0)  // sleep 1 second
```

### Float Math

Float arithmetic (`+`, `-`, `*`, `/`) uses native CPU instructions. Float modulo (`%`) and power (`**`) require runtime calls:

| Operation | Codegen |
|-----------|---------|
| `a + b` (float) | `fadd` instruction |
| `a - b` (float) | `fsub` instruction |
| `a * b` (float) | `fmul` instruction |
| `a / b` (float) | `fdiv` instruction |
| `a % b` (float) | `kryos_fmod(a, b)` runtime call |
| `a ** b` (float) | `kryos_fpow(a, b)` runtime call |
| `a ** b` (int) | `kryos_ipow(a, b)` runtime call |

Integer modulo (`%`) uses the native `srem` instruction. Integer power uses the `kryos_ipow` runtime function, which implements fast exponentiation by squaring.

### Channels

Channel operations compile to i64-based runtime wrappers:

| Kryos code | Runtime call |
|-----------|-------------|
| `chan()` | `kryos_chan_new_i64()` |
| `send(ch, val)` | `kryos_chan_send_i64(ch, val)` |
| `recv(ch)` | `kryos_chan_recv_i64(ch)` |

Channels are thread-safe MPMC (multi-producer, multi-consumer) queues backed by `Mutex<VecDeque>` + `Condvar`. Reference-counted handles allow sharing across threads.

## Debugging Compiled Output

### Reading LLVM IR

Use `--emit-llvm` with `--release` to inspect the generated LLVM IR:

```bash
kryos build hello.kry --release --emit-llvm
```

LLVM IR is verbose but straightforward. Key things to look for:

- **`define`** -- function definitions
- **`entry:`** -- every function starts here
- **`alloca`** -- stack allocation for a local variable
- **`store`/`load`** -- write to / read from a stack slot
- **`add`/`sub`/`mul`/`sdiv`** -- integer arithmetic
- **`fadd`/`fsub`/`fmul`/`fdiv`** -- float arithmetic
- **`icmp`/`fcmp`** -- integer/float comparisons
- **`br`** -- branch (conditional or unconditional)
- **`call`** -- function call
- **`ret`** -- return from function
- **`getelementptr`** -- pointer arithmetic for field/slot access

### Reading MIR

Use `--emit-mir` to inspect the mid-level IR:

```bash
kryos build hello.kry --emit-mir
```

Output:

```
fn main() -> void {
    let _0: void;

    bb0:
        _0 = call println("Hello, World!")
        drop(_0)
        return
}
```

MIR shows the program as basic blocks (`bb0`, `bb1`, ...) with SSA-style locals (`_0`, `_1`, ...), instructions (`assign`, `call`, `drop`, `spawn`), and terminators (`return`, `goto`, `branch`, `switch`). Named locals show their source name as a comment. This is useful for understanding how the compiler lowers control flow and desugars high-level constructs before handing off to the codegen backend.

### Common Issues

**"llc not found"** -- Install LLVM. On Ubuntu: `apt install llvm`. On macOS: `brew install llvm`. On Windows: install LLVM from the releases page and add to PATH. Only needed for `--release` builds.

**"clang not found"** -- Install Clang. Usually comes with LLVM. Only needed for `--release` builds.

**Cranelift errors** -- If a debug build fails in codegen, the issue is likely an unsupported construct in the Cranelift backend. Try `--release` (LLVM) to see if the issue is backend-specific, and report the error.

### Verifying LLVM IR

You can run `lli` (LLVM's IR interpreter) to test generated LLVM IR without compiling to native:

```bash
kryos build hello.kry --release --emit-llvm > hello.ll
lli hello.ll
```

This is useful for quick iteration when debugging codegen issues.

## Runtime Functions

The compiler links against a small runtime library that provides implementations for aggregate types. These are the `kryos_*` functions called by generated code:

| Function | Purpose |
|----------|---------|
| `kryos_string_new` | Create a KryosString from raw bytes |
| `kryos_string_concat` | Concatenate two strings |
| `kryos_string_eq` | Compare two strings for equality |
| `kryos_string_len` | Get string byte length |
| `kryos_string_free` | Free a string's heap allocation |
| `kryos_println_str` | Print a string to stdout with newline |
| `kryos_print_str` | Print a string to stdout |
| `kryos_eprintln_str` | Print a string to stderr with newline |
| `kryos_i64_to_string` | Convert an i64 to its string representation |
| `kryos_f64_to_string` | Convert an f64 to its string representation |
| `kryos_bool_to_string` | Convert a bool to "true" or "false" |
| `kryos_array_new` | Create an empty array |
| `kryos_array_push` | Append a value |
| `kryos_array_get` | Get value by index |
| `kryos_array_set` | Set value by index |
| `kryos_array_len` | Get array length |
| `kryos_map_new` | Create an empty map |
| `kryos_map_insert` | Insert a key-value pair (integer key) |
| `kryos_map_get` | Get value by integer key |
| `kryos_map_insert_str` | Insert a key-value pair (string key, content-hashed) |
| `kryos_map_get_str` | Get value by string key (content-hashed) |
| `kryos_map_len` | Get map size |
| `kryos_string_hash` | FNV-1a content hash of a KryosString (used by string-key maps) |
| `kryos_chan_new_i64` | Create a channel for i64 values |
| `kryos_chan_send_i64` | Send an i64 through a channel |
| `kryos_chan_recv_i64` | Receive an i64 from a channel (blocking) |
| `kryos_builtin_len` | Generic `len()` for any collection |
| `kryos_builtin_to_string` | Generic `to_string()` conversion |
| `kryos_ipow` | Integer exponentiation (`**` operator) |

These runtime functions are compiled as part of the Kryos runtime crate (`kryos-rt`) and linked into every binary automatically. The compiler locates the runtime library using the following search order:

1. `KRYOS_RT_LIB` environment variable (explicit path override)
2. `<compiler_dir>/../lib/` (distribution layout)
3. `<compiler_dir>/` (flat distribution)
4. `target/debug/` or `target/release/` (Cargo development builds)

Use `--verbose` to see which runtime library the compiler finds:

```bash
kryos build hello.kry --verbose
```

```
[kryos] runtime lib: /path/to/libkryos_rt.a
```
