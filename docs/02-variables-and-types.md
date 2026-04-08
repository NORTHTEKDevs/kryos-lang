# Variables and Types

Kryos is statically typed with type inference. You declare what you mean, the compiler catches what you miss, and the runtime never guesses.

## Variable declarations

### Immutable by default: `let`

```
let x = 10
let name = "hello"
```

Variables declared with `let` cannot be reassigned. This is intentional -- immutability by default eliminates an entire class of bugs. If you don't need to change a value, don't pay the complexity cost of allowing it.

### Mutable: `let mut`

```
let mut y = 20
y = 25

let mut s = "original"
s = "updated"
```

When you need reassignment, say so explicitly with `mut`. This makes mutation visible at the declaration site, not buried in the middle of a function.

> **Common mistake:** Forgetting `mut` when you plan to reassign. The compiler will catch this, but it trips up newcomers. If you're writing a loop accumulator or building a string incrementally, you need `let mut`.
>
> ```
> // This will fail:
> let total = 0
> total = total + 1  // error: cannot reassign immutable variable
>
> // This works:
> let mut total = 0
> total = total + 1
> ```

## Type annotations vs inference

Kryos infers types from the right-hand side of assignments. You can also annotate types explicitly:

```
// Inferred:
let x = 42          // i32
let pi = 3.14       // f64
let name = "Kryos"  // str
let flag = true     // bool

// Explicit:
let x: i32 = 42
let pi: f64 = 3.14
let name: str = "Kryos"
let flag: bool = true
```

Explicit annotations are useful when you want a specific numeric width, when the type isn't obvious from context, or when documenting a public API.

## Numeric types

### Signed integers

| Type   | Size    | Range                                      |
|--------|---------|---------------------------------------------|
| `i8`   | 8-bit   | -128 to 127                                |
| `i16`  | 16-bit  | -32,768 to 32,767                          |
| `i32`  | 32-bit  | -2,147,483,648 to 2,147,483,647            |
| `i64`  | 64-bit  | -9.2 x 10^18 to 9.2 x 10^18               |
| `i128` | 128-bit | -1.7 x 10^38 to 1.7 x 10^38               |

`i32` is the default integer type. When you write `let x = 42`, you get an `i32`.

### Unsigned integers

| Type   | Size    | Range                          |
|--------|---------|--------------------------------|
| `u8`   | 8-bit   | 0 to 255                      |
| `u16`  | 16-bit  | 0 to 65,535                   |
| `u32`  | 32-bit  | 0 to 4,294,967,295            |
| `u64`  | 64-bit  | 0 to 1.8 x 10^19             |
| `u128` | 128-bit | 0 to 3.4 x 10^38             |

### Floating-point

| Type  | Size   | Precision         |
|-------|--------|--------------------|
| `f32` | 32-bit | ~7 decimal digits  |
| `f64` | 64-bit | ~15 decimal digits |

`f64` is the default float type. When you write `let x = 3.14`, you get an `f64`.

### Integer promotions

Smaller integers can be assigned to larger integers of the same signedness without explicit conversion. Unsigned integers can be assigned to a signed integer if there is room (e.g., `u8` to `i16`). Integers can also widen to floats automatically.

## Integer literal formats

Kryos supports multiple representations for integer literals:

```
// Decimal
let decimal = 42

// Hexadecimal (0x or 0X prefix)
let hex = 0xFF         // 255

// Binary (0b or 0B prefix)
let binary = 0b1010    // 10

// Octal (0o or 0O prefix)
let octal = 0o77       // 63

// Underscore separators for readability
let million = 1_000_000
let bytes = 0xFF_FF
let bits = 0b1111_0000
```

Underscores can appear between any digits and are purely visual -- the compiler strips them.

## Float literals

```
// Standard decimal
let pi = 3.14159

// Scientific notation (e or E)
let small = 2.5e-3     // 0.0025
let large = 1.0e10     // 10,000,000,000
let positive = 6.022E23

// Underscore separators work in floats too
let big = 1_234.567_89
```

The exponent can have an optional `+` or `-` sign.

## Boolean type

```
let flag: bool = true
let done = false
```

`true` and `false` are the only boolean values. Kryos does not do implicit boolean coercion -- `0`, `""`, and `none` are not `false`.

## String type

Strings use double quotes and are UTF-8:

```
let greeting = "hello world"
let empty = ""
```

### String concatenation

```
let s = "hello" + " " + "world"    // "hello world"
let name = "world"
let msg = "hello " + name + "!"    // "hello world!"
```

### String interpolation

Embed expressions directly in strings with `{}`:

```
let name = "Kryos"
let version = 1
println("Welcome to {name} v{version}")
```

Any expression can go inside the braces. The lexer handles nested braces correctly, so you can put function calls and complex expressions in interpolations.

### Escape sequences

| Sequence | Character            |
|----------|----------------------|
| `\n`     | Newline              |
| `\r`     | Carriage return      |
| `\t`     | Tab                  |
| `\\`     | Backslash            |
| `\"`     | Double quote         |
| `\'`     | Single quote         |
| `\0`     | Null character       |
| `\{`     | Literal `{`          |
| `\}`     | Literal `}`          |
| `\u{...}`| Unicode codepoint    |

Unicode escapes use hex digits inside braces: `\u{1F600}` produces a grinning face.

### Triple-quoted strings

For multiline content, use triple double quotes:

```
let sql = """
    SELECT *
    FROM users
    WHERE active = true
"""
```

Triple-quoted strings support the same interpolation and escape sequences as regular strings.

## Char type

Characters use single quotes and represent a single Unicode character:

```
let letter: char = 'K'
let newline = '\n'
let emoji = '\u{1F680}'
```

Character literals support the same escape sequences as strings. Empty character literals (`''`) are a compile error.

## None type

```
let nothing = none
```

`none` represents the absence of a value. Its type is `none`.

## Array types

Arrays are ordered, zero-indexed collections of a single element type:

```
let arr = [1, 2, 3]            // inferred as [i32]
let names: [str] = ["a", "b"]  // explicit type annotation
```

Access elements by index:

```
let first = arr[0]    // 1
let second = arr[1]   // 2
```

Mutable arrays support `push` and `pop`:

```
let mut nums = [10, 20, 30]
push(nums, 40)     // nums is now [10, 20, 30, 40]
let last = pop(nums)  // last is 40
```

Array length:

```
println(len(arr))  // 3
```

## Type conversion

Kryos provides built-in functions for explicit type conversion:

```
// to_string -- converts any value to its string representation
let s = to_string(42)        // "42"
let sf = to_string(3.14)     // "3.14"
let sb = to_string(true)     // "true"

// parse_int -- parses a string to integer
let n = parse_int("42")      // 42

// parse_float -- parses a string to float
let f = parse_float("3.14")  // 3.14
```

## Runtime type inspection

Use `type_of()` to inspect a value's type at runtime:

```
println(type_of(42))        // "i32"
println(type_of(3.14))      // "f64"
println(type_of("hello"))   // "str"
println(type_of(true))      // "bool"
println(type_of([1, 2]))    // "array"
println(type_of(none))      // "none"
```

For struct instances, `type_of()` returns the struct name.

## Coming from Rust

- **Simpler type syntax.** No lifetime annotations, no borrowing syntax. `let x: i32 = 42` works the same, but you never write `&'a str` or `Box<dyn Trait>`.
- **Same numeric types.** `i8` through `i128`, `u8` through `u128`, `f32`, `f64` -- all present and named identically.
- **`mut` on the binding, not the type.** `let mut x = 5` instead of `let mut x: i32 = 5` -- though the latter works too. No `&mut` references.
- **`none` instead of `None`.** Lowercase, and it's a value, not a variant of `Option` (though Kryos has `Option` as a built-in type as well).
