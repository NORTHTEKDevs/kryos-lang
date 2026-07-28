# Ownership

> **Implementation Status:** Kryos uses ARC (automatic reference counting) for heap-backed values (`str`, `[T]`, `map<K, V>`, structs/enums) and `Copy` semantics for primitives (`i64`, `f64`, `bool`, ...). Passing a value to a function **shares** it (a refcount bump), not a destructive move — reusing the original binding afterward is safe and compiles cleanly on both backends. The compiler runs an advisory ownership/borrow analyzer that surfaces move/borrow *diagnostics* (`E0300` and friends) for signal, but it does **not** block reuse of ARC-backed values. Shared references (`&T`) and mutable references (`&mut T`) are parsed and type-checked; full borrow-checking enforcement (preventing overlapping `&mut`) is not yet implemented.

This chapter explains how Kryos manages memory without a garbage collector and without forcing you to think about destructive moves the way Rust does.

## Why ARC instead of GC or manual management

Most languages handle memory in one of two ways:

1. **Garbage collection** (Go, Java, C#) -- a runtime process scans for unused memory and frees it. Simple for the programmer but adds latency and uses more memory.
2. **Manual management** (C, C++) -- the programmer allocates and frees memory. Fast but riddled with bugs: dangling pointers, double frees, use-after-free, data races.

Kryos takes a third path, closer to Swift than to Rust: **automatic reference counting (ARC)**. Every heap-backed value (`str`, `[T]`, `map<K, V>`, structs, enums) carries a refcount. Passing it around bumps the count; when the count hits zero, the runtime frees it. No GC pauses, no manual `free()`, and -- unlike Rust -- no destructive moves to track in your head. Primitives (`i64`, `f64`, `bool`, and friends) are `Copy` and don't participate in refcounting at all.

## Value semantics: passing shares, it doesn't consume

Passing a value to a function **shares** the underlying data. The callee gets its own handle to the same refcounted allocation; the caller's binding stays valid and can be used again afterward:

```kryos
fn main() {
    let s: str = "hello"
    consume(s)
    println(s)   // OK -- s is still valid; the data is shared, not consumed
}

fn consume(x: str) {
    println(x)
}
```

This compiles and runs cleanly on both backends. There is no destructive move in Kryos and no `.clone()` method to call before reuse -- reuse after passing already works.

```kryos
struct Point {
    x: i32,
    y: i32
}

fn manhattan(p: Point) -> i32 {
    return abs(p.x) + abs(p.y)
}

let p = Point { x: 2, y: 3 }
println(manhattan(p))  // 5
println(manhattan(p))  // 5 -- p is still valid, this is not an error
```

## Copy types

Primitives are `Copy`: they duplicate on assignment or when passed, and the original stays valid too. Everything else is an ARC-backed heap handle:

| Copy types (duplicated) | ARC-backed heap handles (shared) |
|------------------------|-----------------------------------|
| `i8`, `i16`, `i32`, `i64`, `i128` | `str` |
| `u8`, `u16`, `u32`, `u64`, `u128` | Arrays (`[T]`) |
| `f32`, `f64` | `map<K, V>` |
| `bool` | Structs |
| `char` | Enums |
| `int`, `float` | Tuples containing any of the above |

```kryos
let x = 42
let y = x
println(x)  // fine -- integers are Copy, x and y are independent
println(y)  // also fine
```

For a heap-backed value, both bindings stay valid too -- they just point at shared data rather than independent copies at the moment of the call:

```kryos
let s = "hello"
let t = s
println(s)  // fine -- s is still valid
println(t)  // fine
```

The rule of thumb: everything reuses safely after being passed or assigned. The distinction between Copy and ARC-backed types is about *how* the value is represented (duplicated bits vs. a shared refcounted handle), not about whether you're allowed to keep using the original.

## Independent copies

`let b = a` for a heap-backed value doesn't just alias -- it **deep-copies** the heap fields, so `a` and `b` start out as independent values. If you then want to mutate one without affecting the other, assign into a `let mut` local and mutate that local:

```kryos
let mut scores = [90, 85, 92]
let mut backup = scores   // backup is an independent copy (heap fields deep-copied)
backup[0] = 100
println(scores[0])  // 90 -- unaffected
println(backup[0])  // 100
```

`.clone()` is not currently a method in Kryos -- you don't need it. Assignment already gives you an independent copy of heap fields; see gotcha #23 in `CLAUDE.md` for the exact copy/mutation semantics and the residual per-backend caveats around mutating a struct-typed *function parameter* in place (the portable pattern there is the same: copy into a `let mut` local before mutating, or return the modified value).

## Mutability: `let` vs `let mut`

Variables are immutable by default. Use `let mut` when you need to reassign:

```kryos
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

This is a real, hard error (`E0302`) -- it's unrelated to the ARC/move-diagnostic story above.

## The advisory ownership analyzer

The compiler still runs an ownership/borrow analysis pass after type checking, and it can surface diagnostics tagged `E0300` ("use of moved value") when a value is passed and then read again. **This is advisory, not a hard error** -- ARC-backed values may be reused after being passed, and programs that do so compile and run correctly on both backends:

```kryos
fn main() {
    let s: str = "hi"
    consume(s)
    println(s)   // compiles fine -- s is shared, not consumed
}

fn consume(x: str) {
    println(x)
}
```

Run `kryos explain E0300` for the long-form description of the diagnostic. Treat it the way you'd treat a linter hint, not the way you'd treat a Rust borrow-check error -- it does not block `kryos check`, `kryos run`, or `kryos build` from succeeding on reuse-after-pass code.

## Borrowing (partially implemented)

Shared references (`&T`) and mutable references (`&mut T`) are parsed and type-checked, and tracked through MIR with mutability preserved. Auto-deref works for field access through references. Full borrow-checking enforcement (preventing overlapping `&mut` borrows) is **not yet implemented** -- Kryos's memory safety today comes from ARC (heap values are freed when their refcount hits zero), not from a Rust-style borrow checker.

In practice this means you rarely need `&`/`&mut` syntax for the everyday case of "use a value, then use it again" -- ARC sharing plus reuse-after-pass already covers that. Reach for references when you specifically want to avoid a refcount bump/deep copy in a hot path, or when interfacing with `unsafe` code that expects pointer-like semantics.

## Ownership in loops

Because reuse-after-pass is safe, using an ARC-backed value repeatedly inside a loop body is fine -- the loop doesn't consume it on the first iteration:

```kryos
let s = "hello"
for i in range(0, 3) {
    println(s)  // fine on every iteration -- s is shared, not consumed
}
```

## Ownership with function parameters

Passing a value into a function parameter shares it; the caller's binding stays valid after the call returns:

```kryos
fn process(data: str) -> str {
    return data + " processed"
}

let input = "raw"
let result = process(input)
println(input)   // "raw" -- still valid
println(result)  // "raw processed"
```

Copy types are duplicated into parameters, so the original remains valid too -- same outcome, different mechanism:

```kryos
fn double(x: i32) -> i32 {
    return x * 2
}

let n = 21
println(double(n))  // 42
println(n)          // 21 -- still valid, integers are Copy
```

## Ownership with return values

Returning a value hands the caller a handle to it (for ARC-backed types) or a duplicate (for Copy types) -- either way the caller ends up with a valid value, and nothing about the return path requires you to give up the original if it came from an argument you already hold elsewhere:

```kryos
fn make_point() -> Point {
    let p = Point { x: 1, y: 2 }
    return p
}

let result = make_point()  // result owns a valid handle to the Point
```

## Ownership with arrays and structs

Arrays and structs are ARC-backed. Assigning one to another binding deep-copies the heap fields, so both bindings are valid and independent:

```kryos
let mut scores = [90, 85, 92]
let backup = scores
println(scores[0])  // 90 -- still valid
println(backup[0])  // 90 -- independent copy
```

Struct fields are accessed through the owning variable as usual, and reuse after copying/passing is fine:

```kryos
struct Point { x: i64, y: i64 }

fn main() {
    let p = Point { x: 2, y: 3 }
    let q = p
    println(to_string(p.x))  // 2 -- still valid
    println(to_string(q.x))  // 2
}
```

## How Kryos differs from Rust

Kryos's memory model is inspired by Rust's ownership vocabulary but lands closer to Swift/Objective-C ARC in practice.

| Concept | Rust | Kryos |
|---------|------|-------|
| Memory safety mechanism | Ownership + borrow checker | ARC (refcounting) + advisory diagnostics |
| Destructive move semantics | Yes -- reuse after move is a hard error | No -- reuse after passing/assigning is safe |
| Use-after-move detection | Hard compile error | Advisory diagnostic (`E0300`), does not block compilation |
| Borrow checker | Yes, enforced | References parsed/type-checked; enforcement not yet implemented |
| `&T` / `&mut T` reference syntax | Yes, required for zero-copy sharing | Exists, but rarely needed -- ARC sharing covers the common case |
| Lifetime annotations (`'a`) | Yes | No -- not needed |
| `Clone` / `Copy` traits | Explicit traits | `Copy` is built-in based on type; no `Clone`/`.clone()` today (not needed -- assignment already deep-copies heap fields) |
| `Rc`, `Arc`, `Box` | Smart pointer types you opt into | Every heap-backed value is ARC'd by default; no wrapper types needed |

The core guarantee is the same: no use-after-free, no double-free, no dangling pointers. Kryos gets there with refcounting plus deep-copy-on-assign instead of a borrow checker, which trades some of Rust's zero-copy guarantees for a model where "just reuse the value" always works.

## Coming from Rust

If you already know Rust's ownership system, the important differences:

- Passing or assigning a value does **not** consume it. There's no equivalent of Rust's "value moved here" error for the common case -- reuse the original binding freely.
- No `Clone::clone()` method to reach for. `let b = a` already gives you an independent copy of heap fields for `str`/`[T]`/`map`/structs.
- No `&` or `&mut` in typical code. The syntax exists and type-checks, but you don't need it just to reuse a value.
- No lifetime annotations. The analyzer does not require `'a` parameters.
- Copy types are determined by the type name, not by a trait implementation.
- The ownership analyzer's `E0300`/`E0301`/`E0302`/`E0303` diagnostics exist and are worth reading, but only `E0301` (use of uninitialized value), `E0302` (assignment to an immutable binding), and capability violations are hard errors that block compilation today. `E0300` (move diagnostic) is advisory.

## Common mistakes

**Mutating an immutable binding**

```kryos
let x = 10
x = 20  // ERROR: cannot assign to immutable variable 'x'
```

Fix: declare with `let mut x = 10`.

**Expecting a mutation to propagate without a `let mut` local**

```kryos
let mut scores = [90, 85, 92]
let backup = scores
backup[0] = 100          // mutates backup's own copy
println(scores[0])       // 90 -- backup was an independent copy, not a view
```

If you wanted `backup` to observe mutations made through `scores` (or vice versa), that's not what assignment gives you -- assignment deep-copies heap fields. Restructure to pass the same handle explicitly (e.g. mutate through a function that takes and returns the value) if you need shared mutable state across two names in one scope.

**Assuming you need `.clone()` before reusing a value**

```kryos
fn consume(x: str) {
    println(x)
}

fn main() {
    let s = "hello"
    consume(s)
    println(s)   // fine -- no .clone() needed, this is not an error
}
```

This is the opposite of a Rust habit worth unlearning: in Kryos, reuse-after-pass just works.
