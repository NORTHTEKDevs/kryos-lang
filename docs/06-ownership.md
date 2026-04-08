# Ownership

> **Implementation Status:** Move semantics and use-after-move detection are fully implemented in the compiler. The ownership analyzer runs after type checking and reports errors for moved values. Shared references (`&T`) and mutable references (`&mut T`) are parsed, type-checked, and tracked through MIR with mutability preserved (`MirType::Ref { inner, mutable }`). Auto-deref works for field access through references. Borrow checking (preventing overlapping `&mut`) is not yet implemented -- Kryos uses ownership-based memory safety for now. Full borrow checking with lifetime enforcement is planned for a future release. ARC (automatic reference counting) insertions are implemented for values that escape their scope.

This is the most important chapter in the manual. Ownership is how Kryos gives you memory safety without a garbage collector. Once you internalize the rules, they become second nature -- and the compiler catches the mistakes before your code runs.

## Why ownership exists

Most languages handle memory in one of two ways:

1. **Garbage collection** (Go, Java, C#) -- a runtime process scans for unused memory and frees it. Simple for the programmer but adds latency and uses more memory.
2. **Manual management** (C, C++) -- the programmer allocates and frees memory. Fast but riddled with bugs: dangling pointers, double frees, use-after-free, data races.

Kryos takes a third path: **compile-time ownership tracking**. The compiler proves at build time that every value is used correctly. No runtime cost, no GC pauses, no dangling pointers.

## The ownership rules

There are three rules. Everything else follows from these.

1. **Every value has exactly one owner** -- the variable that holds it.
2. **When the owner goes out of scope, the value is cleaned up.**
3. **Assigning or passing a value to another variable transfers ownership** (a "move").

## Move semantics

When you assign a variable to another variable, the value moves:

```
let a = "hello"
let b = a
// a is now moved -- the string belongs to b
```

The same happens when you pass a value to a function:

```
struct Point {
    x: i32,
    y: i32
}

fn manhattan(p: Point) -> i32 {
    return abs(p.x) + abs(p.y)
}

let p = Point { x: 2, y: 3 }
println(manhattan(p))  // 5
// p is moved into manhattan -- it no longer exists here
```

After the call to `manhattan`, the variable `p` is gone. The function took ownership of the Point.

## Use-after-move

If you try to use a value after it has been moved, the compiler rejects your code:

```
let p = Point { x: 2, y: 3 }
println(manhattan(p))  // p moves into manhattan
println(manhattan(p))  // ERROR: use of moved value 'p'
```

The error message tells you where the value was moved:

```
ownership error at line 4, col 1: use of moved value 'p'
  note: value moved at line 3
```

This is a compile-time error, not a runtime crash. The program never runs with invalid state.

## Copy types

Not every value moves. Small, stack-allocated types are **copied** instead of moved. These are called Copy types:

| Copy types | Non-Copy types |
|------------|---------------|
| `i8`, `i16`, `i32`, `i64`, `i128` | `str` (heap-allocated) |
| `u8`, `u16`, `u32`, `u64`, `u128` | Arrays |
| `f32`, `f64` | Structs |
| `bool` | Tuples |
| `char` | Tensors |
| `int`, `float` | |

When you assign or pass a Copy type, the original stays valid:

```
let x = 42
let y = x
println(x)  // fine -- integers are Copy
println(y)  // also fine
```

Compare with a non-Copy type:

```
let s = "hello"
let t = s
// println(s)  -- ERROR: use of moved value 's'
println(t)     // fine
```

The rule of thumb: if it is a number or a boolean, it copies. Everything else moves.

## Mutability: `let` vs `let mut`

Variables are immutable by default. Use `let mut` when you need to reassign:

```
let x = 10
// x = 20     -- ERROR: cannot assign to immutable variable 'x'

let mut y = 10
y = 20        // fine
```

Immutability by default is a safety feature. It prevents accidental mutation and makes code easier to reason about. The compiler tells you when you need `mut`:

```
ownership error: cannot assign to immutable variable 'x'
  note: consider declaring with 'let mut'
```

## Borrowing (partially implemented)

> **Status:** References (`&T`, `&mut T`) are parsed, type-checked, and tracked through MIR. Auto-deref works for field access through references. Full borrow checking (preventing overlapping `&mut`) is not yet enforced. Kryos currently relies on move semantics for memory safety. Full borrow checking with lifetime enforcement is planned for a future release.

Sometimes you want to read a value without taking ownership. This is called borrowing. The planned borrow checker will enforce these rules:

1. **You can have multiple immutable borrows** -- any number of readers.
2. **You can have one mutable borrow** -- exactly one writer.
3. **You cannot have immutable and mutable borrows at the same time.**

These rules will prevent data races at compile time.

### What counts as a borrow

In the current Kryos model, borrowing happens implicitly. When you read from a variable in an expression, you are borrowing it. When a variable holds a reference to another variable's data, the borrow checker tracks the relationship.

### Cannot mutate while borrowed

If other code is reading from a value, you cannot change it:

```
let mut data = [1, 2, 3]
let view = data       // view borrows from data
// data[0] = 99       -- ERROR: cannot mutate 'data' because it is borrowed
```

The borrow is released when the borrowing variable goes out of scope (exits its block).

### Cannot borrow mutably more than once

```
// ERROR: cannot borrow 'data' as mutable more than once at a time
```

This prevents two parts of your code from modifying the same data simultaneously, which is the root cause of most concurrency bugs.

## Ownership in loops

The borrow checker is extra careful inside loops because the body executes multiple times. Moving a value inside a loop body means the second iteration would use a moved value:

```
let s = "hello"
for i in range(0, 3) {
    println(s)  // ERROR: value 's' moved inside loop body --
                //        would be used-after-move on next iteration
}
```

The compiler catches this at compile time, even though the first iteration would work fine. The fix depends on the situation -- clone the value, or restructure the code to avoid the move.

## Ownership with function parameters

Values move into function parameters:

```
fn process(data: str) -> str {
    return data + " processed"
}

let input = "raw"
let result = process(input)
// input is moved -- it belongs to process now
println(result)  // "raw processed"
```

Copy types are copied into parameters, so the original remains valid:

```
fn double(x: i32) -> i32 {
    return x * 2
}

let n = 21
println(double(n))  // 42
println(n)          // 21 -- still valid, integers are Copy
```

## Ownership with return values

Returning a value moves it out of the function:

```
fn make_point() -> Point {
    let p = Point { x: 1, y: 2 }
    return p  // p moves out to the caller
}

let result = make_point()  // result now owns the Point
```

This is how you transfer ownership back to the caller. The value is not copied -- it is moved.

## Ownership with arrays and structs

Arrays and structs are non-Copy. They follow all the same move rules:

```
let mut scores = [90, 85, 92]
let backup = scores
// scores is moved -- backup owns the array now
```

Struct fields are accessed through the owning variable:

```
let p = Point { x: 2, y: 3 }
let q = p
// p is moved
println(q.x)  // 2
```

## Re-assignment restores ownership

If a variable was moved, assigning a new value to it brings it back:

```
let mut s = "first"
let t = s          // s is moved
s = "second"       // s is owned again with a new value
println(s)         // "second"
```

This is tracked by the borrow checker -- the variable transitions from `moved` back to `owned`.

## Patterns for avoiding ownership issues

### Pattern 1: Restructure to avoid the move

Instead of passing a struct, pass its fields if they are Copy:

```
// Instead of this (moves p):
fn manhattan(p: Point) -> i32 {
    return abs(p.x) + abs(p.y)
}

// Consider this (copies x and y):
fn manhattan_xy(x: i32, y: i32) -> i32 {
    return abs(x) + abs(y)
}

let p = Point { x: 2, y: 3 }
println(manhattan_xy(p.x, p.y))  // p is not moved
println(p.x)                     // still valid
```

### Pattern 2: Return the value back

If a function needs temporary ownership, have it return the value when done:

```
fn inspect_and_return(p: Point) -> Point {
    println("Point: " + to_string(p.x) + ", " + to_string(p.y))
    return p
}

let mut p = Point { x: 1, y: 2 }
p = inspect_and_return(p)  // p moves in, then moves back out
println(p.x)               // still valid
```

### Pattern 3: Create new values

Instead of trying to reuse a moved value, create a fresh one:

```
let p1 = Point { x: 1, y: 2 }
println(manhattan(p1))  // p1 moves

let p2 = Point { x: 1, y: 2 }  // new Point with same values
println(manhattan(p2))  // p2 moves
```

## How Kryos differs from Rust

Kryos ownership is inspired by Rust but deliberately simpler.

| Concept | Rust | Kryos |
|---------|------|-------|
| Move semantics | Yes | Yes |
| Use-after-move detection | Yes | Yes |
| Borrow checker | Yes | Yes |
| `&T` reference syntax | Yes | No -- borrowing is implicit |
| `&mut T` mutable references | Yes | No -- mutation checked differently |
| Lifetime annotations (`'a`) | Yes | No -- not needed |
| `Clone` / `Copy` traits | Explicit traits | Built-in based on type |
| `Rc`, `Arc`, `Box` | Smart pointer types | Not needed |

The core guarantee is the same: no use-after-free, no data races, no dangling pointers. Kryos achieves this with fewer concepts. You do not need to learn lifetime syntax, reference types, or smart pointers. The trade-off is less fine-grained control over borrowing -- but for most programs, the simpler model is sufficient.

## Coming from Rust

If you already know Rust's ownership system, Kryos will feel familiar with these differences:

- No `&` or `&mut` in the syntax. You do not write reference types.
- No lifetime annotations. The analyzer does not require `'a` parameters.
- No `Clone::clone()` method. (Future versions may add this.)
- Copy types are determined by the type name, not by a trait implementation.
- The borrow checker is conservative -- it may reject some valid programs that Rust would accept. This is by design; false positives are preferred over false negatives.

The mental model is the same: values have one owner, moves transfer ownership, and simultaneous mutable access is forbidden. You just write less syntax to express it.

## Common mistakes

**Using a value after passing it to a function**

```
let data = [1, 2, 3]
println(find_max(data))  // data moves into find_max
println(sum_array(data)) // ERROR: use of moved value 'data'
```

Fix: store the data you need before passing, or restructure the code.

**Mutating an immutable binding**

```
let x = 10
x = 20  // ERROR: cannot assign to immutable variable 'x'
```

Fix: declare with `let mut x = 10`.

**Moving a value inside a loop**

```
let msg = "hello"
for i in range(0, 5) {
    println(msg)  // ERROR: moved inside loop body
}
```

The compiler knows the loop runs more than once and the move on iteration 1 would leave `msg` invalid for iteration 2.

**Assigning to a borrowed variable**

```
let mut data = [1, 2, 3]
let view = data
data[0] = 99  // ERROR: cannot assign to 'data' because it is borrowed
```

Fix: limit the scope of the borrow, or do the mutation before creating the borrow.
