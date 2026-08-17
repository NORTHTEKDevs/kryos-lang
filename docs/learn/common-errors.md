# Common errors and how to fix them

The top compile errors new Kryos users hit, in rough order of frequency. Every entry includes the verbatim error message and the fix.

## Syntax & layout

### "expected newline before next statement"

<!-- docs-example: skip -->
```kryos
fn main() { let x = 1; let y = 2 }   //  ERROR
fn main() {                          //  OK
    let x = 1
    let y = 2
}
```

Kryos uses **newlines, not semicolons** to terminate statements. If you must fit two statements on one line, break the line.

### "expected `}` to close block"

<!-- docs-example: skip -->
```kryos
if x > 0 {
    println("yes")
                         //  no closing }
```

Every `{` must match a `}`. The error span points at the inner unclosed block, not the missing close.

### "unexpected token `else if`"

```kryos
if x { ... } else if y { ... }    //  ERROR
if x { ... } elif y { ... }       //  OK
```

It's `elif`, not `else if`. The grammar diverges from Rust here on purpose — saves a token, no scope ambiguity.

## Strings

### `E0009: unterminated string literal` when hand-building JSON

```kryos
let body = "{\"status\": \"ok\"}"     //  ERROR - E0009
let body = "{{\"status\": \"ok\"}}"   //  OK - literal braces doubled
```

**Every Kryos string literal interpolates** - `"hi {name}"`, delimiter `{ }` not
`${ }` - so a bare `{` in ANY string, including a hand-built JSON object, opens
an interpolation. The content after it (here, an escaped quote) is not a valid
expression, so the compiler reports `E0009`. This is the single most common
trap when hand-rolling JSON with `+` (as in most of the HTTP examples in this
repo before they were fixed): every literal `{` and `}` you want to APPEAR in
the output string must be doubled (`{{`, `}}`) or backslash-escaped (`\{`,
`\{`). An interpolation also cannot start with a string literal directly
(`"{"lit" + x}"` fails the same way) - put a space after the brace or bind the
string to a variable first.

For anything beyond a one-off literal, prefer `std::json::json_object` (or
build the string with `+` instead of embedding raw braces) over hand-escaping.

## Types

### `E0101: unknown type \`T\``

```kryos
fn add(a: int, b: int) -> int { ... }   //  ERROR — int doesn't exist
fn add(a: i64, b: i64) -> i64 { ... }   //  OK
```

Common typos: `int` → `i64`, `float` → `f64`, `string` → `str`, `boolean` → `bool`. Compiler will offer a `did you mean ...?` suggestion.

### `E0102: undefined variable \`name\``

You referenced a variable that's out of scope or wasn't declared:

<!-- docs-example: skip -->
```kryos
fn main() {
    if condition {
        let x = 5
    }
    println(to_string(x))   //  ERROR — x is gone after the if block
}
```

Declare `x` outside the scope you need it in, or return it from the block.

### `E0106: no field \`foo\` on type \`Bar\``

```kryos
struct Point { x: f64, y: f64 }
let p = Point { x: 1.0, y: 2.0 }
println(to_string(p.z))   //  ERROR — no field z
```

Compiler offers a `did you mean ...?` suggestion if a similarly-named field exists (`x`, `y`).

### `E0107: no method \`foo\` found`

```kryos
let s = "hello"
let n = s.lenght()        //  ERROR — typo
let n = s.length()        //  OK
```

Suggestion fires here too.

## Ownership

### Reusing a value after passing it is fine

Kryos values are ARC-backed (reference-counted) heap handles for `str`, `[T]`, `map<K, V>`, and structs/enums; primitives (`i64`, `f64`, `bool`) are `Copy`. Passing a value to a function **shares** it (a refcount bump), so reusing the original binding afterward is safe and allowed — this is not an error:

```kryos
fn main() {
    let s: str = "hi"
    consume(s)
    println(s)          //  OK — s is still valid, the data is shared
}
fn consume(x: str) { println(x) }
```

The compiler runs an advisory move/borrow analyzer that may surface an `E0300` diagnostic here, but it's a lint, not a hard error — it does not block compilation, and `.clone()` is not a method in Kryos today (it isn't needed: `let b = a` already deep-copies heap fields, giving you an independent copy when you actually want one). See [chapter 6](../06-ownership.md) for the full model. (`E0382` is a Rust error code, not a Kryos one.)

## Capability errors

### `E0501: capability violation`

```kryos
@pure
fn add(a: i64, b: i64) -> i64 {
    println("adding")   //  ERROR — println needs the `io` capability
    return a + b
}
```

Either drop the `@pure` annotation, or move the side-effecting call out:

```kryos
@pure
fn add(a: i64, b: i64) -> i64 { return a + b }
```

See [chapter 10](../10-capabilities.md) for the full capability matrix.

## Runtime

### `panic: index out of bounds`

Array / string indexing past the end:

```kryos
let s = "abc"
let c = char_code(substr(s, 5, 6))   //  ERROR at runtime — substr out of range
```

`substr` and `arr[i]` check at runtime. Validate `i < len(arr)` first, or use the iterator (`for x in arr`) which is bounds-safe.

### `panic: division by zero`

<!-- docs-example: skip -->
```kryos
let n = total / count
```

Wrap in a guard:

<!-- docs-example: skip -->
```kryos
if count == 0 {
    println("no records")
    return
}
let n = total / count
```

### `panic: out of memory`

You allocated past the OS limit. The most common cause is an unbounded `let mut s: str = ""` loop with `s = s + chunk` — the cumulative copy is O(n²). Use a `[str]` and join at the end instead.

## `kryos run` vs `kryos build --release`

If your code works under `kryos run` (Cranelift JIT) but breaks under `kryos build --release` (LLVM AOT), it's a compiler bug — please file an issue with the minimal repro. The reverse usually means you depended on some undefined behavior the JIT happened to tolerate.

## When in doubt

- `kryos explain Exxxx` prints a long-form explanation with examples.
- `kryos check file.kry` does fast type-check-only feedback.
- File a Discussion at https://github.com/NORTHTEKDevs/kryos-lang/discussions if the message itself is confusing — that's a docs bug, not a you-bug.
