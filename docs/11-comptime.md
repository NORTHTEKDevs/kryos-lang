# Compile-Time Evaluation

> **Implementation Status:** The `comptime` keyword is parsed and lowered through the compiler pipeline (lexer, parser, AST, MIR, codegen). Currently the inner expression is lowered directly as a regular value -- the planned compile-time interpreter that folds expressions into constants before codegen is not yet implemented. The syntax is reserved and will be fully operational in a future release.

`comptime` blocks are designed to run during compilation, not at runtime. The result will be baked into the program as a constant. Use them for lookup tables, precomputed values, configuration constants, and anything expensive that does not need to be recalculated every time the program starts.

## Syntax

Wrap any expression or group of statements in `comptime { }`:

```
let pi = comptime {
    3.14159
}
```

After compilation, this is identical to `let pi = 3.14159`. The `comptime` block is evaluated once during compilation, and the result replaces the block entirely in the program.

## How It Works (Target Design)

The planned implementation will:

1. Walk the AST before codegen and find `ComptimeBlock` nodes
2. Create a fresh, isolated evaluator instance
3. Execute the block's statements in that evaluator
4. Convert the result back into an AST literal node (IntLiteral, StringLiteral, etc.)
5. Replace the `ComptimeBlock` in the AST with that literal

Currently, `comptime` blocks are parsed as `Expr::ComptimeBlock` in the AST, lowered to `RValue::Comptime(inner)` in MIR, and both the Cranelift and LLVM codegens lower the inner expression directly. The compile-time evaluation step (folding to constants) is planned.

## Use Cases

### Precomputed lookup tables

Build arrays of values at compile time so they are ready instantly at runtime:

```
let squares = comptime {
    let mut arr = []
    for i in range(5) {
        push(arr, i * i)
    }
    arr
}

println(to_string(squares[4]))    // 16
```

The `squares` array `[0, 1, 4, 9, 16]` is built during compilation. At runtime, it is just a literal array -- no loop runs.

### Computed constants

Derive constants from expressions:

```
let table_size = comptime {
    let base = 1024
    base * 16
}
// table_size is 16384 at runtime, no multiplication needed
```

### Configuration values

Compute configuration at build time:

```
let max_retries = comptime {
    let base = 3
    let multiplier = 2
    base * multiplier
}
```

### Mathematical constants

Precompute values that would otherwise require function calls:

```
let hypotenuse = comptime {
    let a = 3
    let b = 4
    a * a + b * b
}
println(to_string(hypotenuse))    // 25
```

### String assembly

Build strings at compile time:

```
let greeting = comptime {
    let parts = ["hello", " ", "world"]
    join("", parts)
}
println(greeting)    // hello world
```

### Multiple comptime blocks

You can have as many comptime blocks as you need in a single module:

```
let a = comptime { 10 + 5 }
let b = comptime { 20 * 2 }
println(to_string(a + b))    // 55
```

### Comptime inside functions

Comptime blocks work inside function bodies too. The table is built once at compile time, not on every function call:

```
fn get_table() {
    return comptime {
        let mut t = []
        for i in range(10) {
            push(t, i * 2)
        }
        t
    }
}

let table = get_table()
println(to_string(table[5]))    // 10
```

Every call to `get_table()` returns the same precomputed array. No loop runs at runtime.

## What Can Run at Comptime

Comptime blocks run in a fresh interpreter instance. They have access to:

- All arithmetic and comparison operators
- `let` and `let mut` bindings
- `if`/`elif`/`else` conditionals
- `for` and `while` loops
- `range()`, `len()`, `push()`, `pop()`
- String operations (`join`, concatenation)
- Array construction and indexing
- `to_string()`, `abs()`, `min()`, `max()`, `sqrt()` and other math builtins
- Function definitions inside the block (local helper functions)
- `return` statements (exits the comptime block with that value)

## What Cannot Run at Comptime

Comptime blocks are **isolated**. They cannot:

- Read or write files (`file_read`, `file_write`)
- Make network requests (`http_get`, `http_post`)
- Access environment variables or command-line arguments
- Call FFI functions
- Use `spawn` or actors
- Access variables defined outside the comptime block
- Use `println` (output goes nowhere -- the comptime interpreter is headless)
- Access GPU or quantum operations

The isolation is by design. Comptime evaluation must be **deterministic** -- the same source code must always produce the same compiled program, regardless of what is on disk or on the network at build time.

### Common mistake: I/O in comptime

This is the most frequent comptime error. Trying to read a file or make a network call inside comptime fails:

```
// This does NOT work
let config = comptime {
    file_read("config.toml")    // Error: file_read is not available in comptime
}
```

If you need file contents baked into the binary, use a build script that generates a `.kry` source file with the content as a string literal before compilation.

## Supported Result Types

The comptime evaluator can produce these types, which it converts back to AST literals:

| Runtime type | AST node | LLVM IR constant |
|-------------|----------|-----------------|
| `int` | `IntLiteral` | `i64 42` |
| `float` | `FloatLiteral` | `double 0x...` (hex) |
| `str` | `StringLiteral` | `[N x i8] c"...\00"` |
| `bool` | `BoolLiteral` | `i1 0` or `i1 1` |
| `none` | `NoneLiteral` | `i64 0` |
| `list` | `ArrayLiteral` | (element-wise) |

For complex types (structs, enums), the evaluator falls back to a string representation. Keep comptime results to primitives and arrays of primitives for best results.

## Caching

The evaluator caches results by AST node identity. If the same comptime block appears in a hot code path (inside a function called in a loop), it is only evaluated once. Subsequent hits return the cached value.

This is an implementation detail -- you should not rely on it for correctness. But it means you do not pay for accidental redundant evaluation.

## Performance Benefits

Comptime moves computation from runtime to compile time. The tradeoff:

- **Compile time** increases by however long the comptime blocks take to evaluate
- **Runtime** decreases because the values are already computed
- **Binary size** may increase if you embed large arrays or strings

For lookup tables, precomputed coefficients, or configuration constants, the tradeoff is almost always worth it. A 1-second compile cost that saves 100 microseconds on every runtime invocation pays for itself after 10,000 calls.

## Comparison with Other Languages

### Rust `const fn`

Rust's `const fn` marks a function as callable at compile time. The function itself is written normally, and the compiler evaluates it when used in a `const` context.

```rust
// Rust
const fn square(x: i32) -> i32 { x * x }
const VALUE: i32 = square(5);
```

Kryos `comptime` is more flexible -- it accepts arbitrary blocks of statements, not just single-expression functions. But it is less integrated: Rust can use `const fn` in type-level computations, while Kryos comptime is purely for value computation.

### Zig `comptime`

Kryos comptime is directly inspired by Zig's `comptime` blocks. The semantics are similar: a block of code runs at compile time and the result replaces the block.

```zig
// Zig
const squares = comptime blk: {
    var arr: [5]i32 = undefined;
    for (0..5) |i| { arr[i] = i * i; }
    break :blk arr;
};
```

Zig goes further -- `comptime` can influence type computation, generate functions, and unroll loops at compile time. Kryos comptime is simpler: it evaluates expressions and produces values. No type-level computation yet.

### C/C++ `constexpr`

C++ `constexpr` functions and variables are evaluated at compile time when possible. The compiler decides whether to evaluate at compile time or runtime based on context.

Kryos `comptime` is explicit: if you write `comptime { }`, it **always** evaluates at compile time. There is no ambiguity about when evaluation happens.

## Summary

| Feature | Detail |
|---------|--------|
| Syntax | `comptime { statements }` |
| When it runs | Before interpretation or codegen |
| What it produces | Literal values (int, float, str, bool, array) |
| Isolation | Fresh interpreter, no I/O, no side effects |
| Caching | Yes, by AST node identity |
| Main use | Lookup tables, constants, precomputed data |
| Main restriction | No filesystem, network, FFI, or external state |
