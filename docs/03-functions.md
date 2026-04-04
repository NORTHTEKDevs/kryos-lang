# Functions

Functions are the primary unit of abstraction in Kryos. They're declared with `fn`, take typed parameters, optionally return a value, and can be passed around like any other value.

## Function declarations

```
fn add(a: i32, b: i32) -> i32 {
    return a + b
}

println(add(5, 10))  // 15
```

The structure: `fn` keyword, name, parameters in parentheses with type annotations, an arrow `->` followed by the return type, and a body in braces.

### Parameters

Every parameter must have a type annotation. There's no default parameter syntax and no variadic arguments -- keep your function signatures explicit.

```
fn greet(name: str) -> str {
    return "hello " + name
}

println(greet("world"))  // hello world
```

### Return values

Use `return` to send a value back from a function. The return type goes after `->`:

```
fn square(x: i32) -> i32 {
    return x * x
}
```

Functions that don't return a meaningful value can omit the return type annotation:

```
fn log_message(msg: str) {
    println(msg)
}
```

> **Common mistake:** Forgetting the return type annotation. If your function returns something and you omit `-> Type`, the compiler may not catch this at parse time, but the type checker will flag it. Always annotate return types on functions that return values.

## Recursive functions

Kryos supports direct recursion. The function name is in scope within its own body:

```
fn factorial(n: i32) -> i32 {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

println(factorial(5))  // 120
```

```
fn fibonacci(n: i32) -> i32 {
    if n <= 1 {
        return n
    }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

println(fibonacci(10))  // 55
```

## Functions as first-class values

Functions can be stored in variables, passed as arguments, and returned from other functions. When accepting a function as a parameter, use the `fn` type:

```
fn apply_twice(f: fn, x: i32) -> i32 {
    return f(f(x))
}

fn double(x: i32) -> i32 {
    return x * 2
}

println(apply_twice(double, 3))  // 12
```

`apply_twice` takes any function `f` and applies it to `x` twice. `double(3)` gives 6, then `double(6)` gives 12.

## Closures

A closure is a function that captures variables from its surrounding scope. In Kryos, you create closures by returning inner functions:

```
fn make_adder(x: i32) -> fn {
    fn adder(y: i32) -> i32 {
        return x + y
    }
    return adder
}

let add5 = make_adder(5)
println(add5(10))   // 15
println(add5(20))   // 25
```

The inner function `adder` captures `x` from `make_adder`'s scope. Each call to `make_adder` produces a new closure with its own captured `x`.

This pattern is useful for:
- **Configuration:** `make_adder(5)` creates a specialized function without repeating the base value.
- **Encapsulation:** The captured `x` is private to the closure. Nothing else can see or modify it.
- **Factories:** Build families of related functions from a single template.

## Anonymous functions

When you need a quick function without naming it, use the anonymous syntax:

```
let add = fn(a, b) {
    return a + b
}
println(add(3, 4))  // 7
```

Anonymous functions use `fn(params) { body }` without a name. Parameters in anonymous functions don't require type annotations -- the types are inferred from usage.

```
let greet = fn(name) {
    return "Hello, " + name
}
println(greet("World"))  // Hello, World
```

Single-expression anonymous functions can be written on one line:

```
let double = fn(n) { return n * 2 }
```

## Higher-order functions

Combining anonymous functions with functions that accept `fn` parameters gives you higher-order programming:

```
fn apply(f, x) {
    return f(x)
}

let double = fn(n) { return n * 2 }
println(apply(double, 5))  // 10
```

This pattern is the foundation for functional-style data processing. Pass behavior as an argument instead of hardcoding it.

## Pipe operator

The pipe operator `|>` passes the result of the left expression as the argument to the function on the right:

```
// Without pipe:
let result = process(transform(value))

// With pipe -- reads left to right:
let result = value |> transform |> process
```

`value |> transform` is equivalent to `transform(value)`. Pipes chain naturally, making data transformation pipelines readable in the order they execute.

```
fn add_one(x: i32) -> i32 { return x + 1 }
fn double(x: i32) -> i32 { return x * 2 }
fn to_str(x: i32) -> str { return to_string(x) }

// Read left to right: start with 5, add one, double, convert to string
let result = 5 |> add_one |> double |> to_str  // "12"
```

This is especially useful when combining multiple transformations. Instead of nesting function calls inside out, you write them in the order they happen.

## Nested function calls

Functions can call other functions, including composing results inline:

```
fn square(x: i32) -> i32 {
    return x * x
}

fn add_one(x: i32) -> i32 {
    return x + 1
}

println(add_one(square(3)))  // 10
```

The inner call evaluates first: `square(3)` returns 9, then `add_one(9)` returns 10.

## Coming from Python

- **`fn` not `def`.** Shorter, and consistent with Rust and other systems languages.
- **Explicit parameter types.** `fn add(a: i32, b: i32)` instead of `def add(a, b)`. Kryos needs to know types at compile time.
- **Braces, not indentation.** Function bodies are wrapped in `{ }`, not determined by whitespace. You still want to indent for readability, but the compiler doesn't care about it.
- **`return` is explicit.** There's no implicit last-expression return (unlike Rust). Always use `return` to send values back.
- **No `*args` or `**kwargs`.** Function signatures are fixed. If you need flexibility, use arrays or structs as parameters.
- **Closures work like you'd expect.** Python's late-binding closure behavior (the classic `lambda i: i` in a loop problem) doesn't apply here. Kryos closures capture values at creation time.

## Coming from Rust

- **Simpler closure syntax.** No `|x| x + 1` syntax -- Kryos closures are just inner `fn` declarations or `fn(params) { body }` anonymous functions.
- **`fn` as a type.** When accepting a function parameter, you write `f: fn` rather than `f: impl Fn(i32) -> i32`. Simpler, but less precise.
- **No lifetime annotations.** Closures capture by value. No need to reason about `'a` or `move`.
- **No implicit return.** Rust returns the last expression without `return`. Kryos always requires `return`.
