# Control Flow

Kryos provides the control flow constructs you'd expect from a systems language, with a few deliberate choices: `elif` instead of `else if`, braces always required, and `match` for pattern matching on values.

## if / elif / else

```
let x = 5
if x > 0 {
    println("positive")
} elif x == 0 {
    println("zero")
} else {
    println("negative")
}
```

Key rules:
- **Braces are always required.** No single-statement shortcuts. This prevents dangling-else bugs and keeps code scannable.
- **`elif`, never `else if`.** This is a single keyword, not two. The compiler will reject `else if` -- use `elif`.
- **No parentheses around the condition.** `if x > 0 {` not `if (x > 0) {`. Parentheses are allowed but unnecessary.

### Chaining conditions

```
if 4 % 2 == 0 {
    println("even")
} else {
    println("odd")
}
```

You can chain as many `elif` branches as you need:

```
let score = 85
if score >= 90 {
    println("A")
} elif score >= 80 {
    println("B")
} elif score >= 70 {
    println("C")
} else {
    println("F")
}
```

> **Common mistake: using `else if` instead of `elif`.** This is the single most common syntax error for developers coming from C, JavaScript, or Go. Kryos uses the `elif` convention -- it is one keyword, not two.
>
> ```
> // Wrong:
> if x > 0 {
>     println("positive")
> } else if x == 0 {   // error: unexpected token 'if'
>     println("zero")
> }
>
> // Correct:
> if x > 0 {
>     println("positive")
> } elif x == 0 {
>     println("zero")
> }
> ```

## while loops

```
let mut sum = 0
let mut i = 0
while i < 5 {
    sum = sum + i
    i = i + 1
}
println(sum)  // 10
```

The loop runs as long as the condition is true. Like `if`, braces are required and parentheses around the condition are optional.

## for loops

### Iterating over a range

Kryos supports two equivalent syntaxes for range-based loops:

```
// Range operator syntax (preferred)
for i in 0..5 {
    println(i)
}
// prints: 0, 1, 2, 3, 4

// range() function syntax
for i in range(0, 5) {
    println(i)
}
// prints: 0, 1, 2, 3, 4
```

Both produce identical code -- a simple counter loop. The `..` operator creates a half-open range (inclusive start, exclusive end). `range(start, end)` does the same thing.

The `range()` function also supports a step parameter: `range(0, 10, 2)` produces `0, 2, 4, 6, 8`.

Single-argument form starts from 0:

```
for i in range(5) {
    println(i)  // 0 through 4
}
```

### Iterating over a collection

`for` works with any iterable, including arrays:

```
let names = ["Alice", "Bob", "Charlie"]
for name in names {
    println(name)
}
```

This is the preferred way to process array elements when you don't need the index.

### Building results with for loops

A common pattern is accumulating a result across iterations:

```
let mut result = ""
for i in range(0, 5) {
    if result != "" {
        result = result + " "
    }
    result = result + to_string(i)
}
println(result)  // 0 1 2 3 4
```

### Iterating with index access

When you need the index (e.g., to access elements by position), combine `range` with `len`:

```
let data = [10, 25, 3, 47, 12]
for i in range(0, len(data)) {
    println(to_string(i) + ": " + to_string(data[i]))
}
```

## break and continue

### break

Exit a loop early:

```
let mut i = 0
while true {
    if i >= 5 {
        break
    }
    println(i)
    i = i + 1
}
```

`break` exits the innermost enclosing `while` or `for` loop immediately.

### continue

Skip to the next iteration:

```
for i in range(0, 10) {
    if i % 2 == 0 {
        continue
    }
    println(i)  // prints only odd numbers: 1, 3, 5, 7, 9
}
```

`continue` jumps to the next iteration of the innermost enclosing loop, skipping any remaining statements in the current iteration.

## match expressions

`match` provides pattern matching on values. It's cleaner than a chain of `if`/`elif` when you're comparing a single value against multiple possibilities.

### Basic matching

```
match "hello" {
    "bye" => println("bye"),
    "hello" => println("greeting"),
    _ => println("unknown"),
}
// prints: greeting
```

Each arm has a pattern, `=>`, and an expression or statement. Arms are separated by commas. The wildcard `_` matches anything and acts as the default case.

### match as an expression

`match` returns a value, so you can assign the result directly:

```
let x = match 42 {
    1 => "one",
    42 => "answer",
    _ => "other",
}
println(x)  // answer
```

This is much cleaner than an `if`/`elif` chain when you're mapping a value to a result:

```
let status = match 404 {
    200 => "ok",
    404 => "not found",
    500 => "error",
    _ => "unknown",
}
println(status)  // not found
```

### When to use match vs if

Use `match` when you're comparing a single value against discrete possibilities. Use `if`/`elif` when you have compound conditions, range checks, or unrelated boolean expressions.

```
// Good use of match -- one value, multiple cases:
let label = match error_code {
    1 => "not found",
    2 => "forbidden",
    3 => "timeout",
    _ => "unknown",
}

// Better as if/elif -- range-based logic:
if score >= 90 {
    println("A")
} elif score >= 80 {
    println("B")
} elif score >= 70 {
    println("C")
}
```

## Nested control flow

All control flow constructs can be nested freely:

```
for i in range(0, 5) {
    if i % 2 == 0 {
        let mut j = 0
        while j < 3 {
            println(to_string(i) + "," + to_string(j))
            j = j + 1
        }
    }
}
```

Inner loops have their own scope. Variables declared inside a block are not visible outside it.

## Coming from Rust

- **`elif` instead of `else if`.** Rust chains `else if`. Kryos uses the single keyword `elif`. The behavior is identical.
- **`match` syntax is similar.** `match value { pattern => result, _ => default }` will feel familiar. The main difference: Kryos `match` currently handles value patterns and wildcards, not destructuring or enum variants with bindings.
- **No `loop` keyword.** Use `while true { }` for infinite loops.
- **`for` supports both `0..5` and `range(0, 5)`.** Rust's `for i in 0..5` works identically in Kryos. You can also use `range(start, end)` or `range(start, end, step)` for the function-call style.
- **`break` and `continue` are the same.** No labeled breaks (`'outer: loop`) yet -- they apply to the innermost loop.
