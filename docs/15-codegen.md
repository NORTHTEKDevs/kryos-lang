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

### Entry Point

Every Kryos program must define `fn main()`. The compiler wraps it as the process entry point:

```llvm
define i32 @main() {
entry:
  call void @kryos_main()
  ret i32 0
}
```

Your `fn main()` is compiled as `@kryos_main` and called from the real `@main`.

### Strings

Strings are opaque handles managed by the `kryos_string` runtime. String literals are interned at compile time. All string operations go through runtime functions:

- `kryos_string_new(data, len)` -- create a string from raw bytes
- `kryos_string_concat(a, b)` -- concatenate two strings, returns a new handle
- `kryos_string_len(s)` -- return the byte length
- `kryos_string_print(s)` -- print to stdout

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

Map literals compile to `kryos_map_new()` followed by `kryos_map_insert()` calls:

```
let m = { "a": 1, "b": 2 }
```

Becomes:

```
m = kryos_map_new()
kryos_map_insert(m, "a", 1)
kryos_map_insert(m, "b", 2)
```

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

### Lambda Expressions

Lambdas use `fn(params) -> RetType { body }` syntax. The codegen distinguishes two cases:

**No captures** -- the lambda compiles to a plain function pointer stored as an i64:

```
let f = fn(x: i32) -> i32 { return x * 2 }
```

Generates a top-level function (`__lambda_0`) and stores its pointer in `f`.

**With captures** -- the lambda compiles to a heap-allocated closure environment: `[func_ptr: i64, cap0: i64, cap1: i64, ...]`. The function pointer is at index 0, followed by captured values.

```
let offset = 10
let f = fn(x: i32) -> i32 { return x + offset }
```

Generates a closure environment `[__lambda_1_ptr, 10]` and a function `__lambda_1(env, x)` that reads `offset` from `env[1]`.

### Ranges

Range expressions compile to a 24-byte stack allocation with three i64 fields:

```
[start: i64, end: i64, inclusive: i64]
```

`range(0, 10)` produces `[0, 10, 0]` (exclusive). `range_inclusive(0, 10)` produces `[0, 10, 1]`.

`for x in range(start, end)` compiles to a counter-based loop:

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

### Generics (Monomorphization)

Generic functions are compiled by monomorphization -- each unique combination of type arguments produces a specialized copy. Since all types use i64 slots, monomorphization primarily affects type-checking rather than codegen in the current implementation.

### Boolean Operators

Boolean operators use keywords: `and`, `or`, `not` (not `&&`, `||`, `!`). They compile to short-circuit evaluation -- `and` skips the right operand if the left is false, `or` skips if the left is true.

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

MIR shows the program as basic blocks with SSA-style assignments and explicit terminators (Branch, Return, Switch). This is useful for understanding how the compiler lowers control flow and desugars high-level constructs before handing off to the codegen backend.

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
| `kryos_string_new` | Create a string from raw bytes |
| `kryos_string_concat` | Concatenate two strings |
| `kryos_string_len` | Get string byte length |
| `kryos_string_print` | Print string to stdout |
| `kryos_array_new` | Create an empty array |
| `kryos_array_push` | Append a value |
| `kryos_array_get` | Get value by index |
| `kryos_array_set` | Set value by index |
| `kryos_array_len` | Get array length |
| `kryos_map_new` | Create an empty map |
| `kryos_map_insert` | Insert a key-value pair |
| `kryos_map_get` | Get value by key |
| `kryos_map_len` | Get map size |

These runtime functions are compiled as part of the Kryos runtime crate and linked into every binary.
