# Kryos Language Reference

Authoritative spec of the Kryos language as implemented in the compiler at
this commit. Lighter than a formal standard, more rigorous than a tutorial.

> Numbered chapters (`01-...` through `15-...`) are tutorial-style; this
> file is the **reference**. When tutorials and reference disagree, the
> reference wins. When the reference and compiler disagree, that's a bug —
> please file it.

## Table of contents

1. [Lexical structure](#1-lexical-structure)
2. [Types](#2-types)
3. [Expressions](#3-expressions)
4. [Statements & control flow](#4-statements--control-flow)
5. [Declarations](#5-declarations)
6. [Pattern matching](#6-pattern-matching)
7. [Ownership and drop order](#7-ownership-and-drop-order)
8. [Integer overflow](#8-integer-overflow)
9. [Concurrency](#9-concurrency)
10. [Unsafe code](#10-unsafe-code)
11. [Modules and visibility](#11-modules-and-visibility)
12. [Runtime panics](#12-runtime-panics)
13. [Conformance checklist](#13-conformance-checklist)

---

## 1. Lexical structure

### 1.1 Source encoding

Source files are UTF-8. The `.kry` extension is conventional; the compiler
accepts any text file passed as input.

### 1.2 Line breaks

Kryos is **line-sensitive**. Statement boundaries are line breaks; there
are no semicolons. To break a long expression across lines, end the line
with an open-bracket (`(`, `[`, `{`), a comma, or a binary operator.
The lexer emits a hint if it sees a `;`:

```
note: Kryos does not use semicolons; line breaks terminate statements
```

### 1.3 Comments

<!-- docs-example: skip -->
```kryos
// single-line comment
/// doc comment, attached to the following declaration
/* block comment */
/* block comments /* nest */ correctly */
```

Block comments (`/* ... */`) are supported and nest. (The self-host compiler
source avoids them by convention, but the language accepts them.)

### 1.4 Identifiers

Identifiers match `[A-Za-z_][A-Za-z0-9_]*`. Identifiers starting with `_`
are still legal but suppress unused-variable warnings.

### 1.5 Keywords (reserved)

```
fn  let  mut  if  elif  else  while  for  in  loop  break  continue
return  match  struct  enum  trait  impl  actor  trait  pub  use  as
async  await  spawn  chan  send  recv  unsafe  extern  Self  self
true  false
```

### 1.6 Literals

| Form         | Type     | Examples                              |
| ------------ | -------- | ------------------------------------- |
| Integer      | `i64`    | `0`, `42`, `1_000_000`, `0x1f`, `0b101` |
| Float        | `f64`    | `1.0`, `3.14`, `1e6`, `1.5e-3`        |
| Bool         | `bool`   | `true`, `false`                       |
| String       | `str`    | `"hello"`, `"line\n"`                 |
| Char (i64)   | `i64`    | `'A'`, `'\n'`                         |
| Array        | `[T]`    | `[1, 2, 3]`, `[]`                     |
| Map          | `map<K,V>` | `{1: "a", 2: "b"}`                  |

String escapes: `\n`, `\r`, `\t`, `\\`, `\"`, `\0`, `\x{NN}`. Numeric
literals may contain `_` as a digit separator anywhere except the first
position.

---

## 2. Types

### 2.1 Primitive types

| Type | Width | Range |
| --- | --- | --- |
| `i8`, `i16`, `i32`, `i64` | 8/16/32/64 bits | signed two's complement |
| `u8`, `u16`, `u32`, `u64` | 8/16/32/64 bits | unsigned |
| `f32`, `f64` | 32/64 bits | IEEE 754 |
| `bool` | 1 bit (stored as 8) | `true` / `false` |
| `str` | heap object | UTF-8 byte sequence |
| `chan` | heap handle | MPMC channel |
| `()` | 0 bits | unit |

### 2.2 Compound types

- **Array**: `[T]` — heap-allocated, dynamically sized.
- **Map**: `map<K, V>` — hash map, K must be hashable.
- **Tuple**: `(T1, T2, ...)` — fixed-arity, heap-allocated.
- **Struct**: nominal record with named fields. Declared with `struct`.
- **Enum**: tagged union with named variants. Declared with `enum`.
- **Option<T>**: `Option::Some(T)` or `Option::None`.
- **Result<T, E>**: `Result::Ok(T)` or `Result::Err(E)`.

### 2.3 Function types

```kryos
fn(i64, i64) -> i64           // function pointer type
async fn(str) -> Result<str>  // async function
```

### 2.4 References

`&T` is a shared reference; `&mut T` is exclusive. Reference lifetimes
are inferred — there is no explicit lifetime syntax (today).

### 2.5 Generics

Generic parameters go in `<>` after the item name:

```kryos
fn first<T>(arr: [T]) -> T { return arr[0] }
struct Box<T> { value: T }
trait Show<T> { fn show(self) -> str }
```

Trait bounds use `: Trait`:

```kryos
fn print_all<T: Show>(items: [T]) { ... }
```

### 2.6 Type inference

Local `let` bindings infer from the initializer. Function parameters and
return types must be explicit. The inference algorithm is local
Hindley-Milner with no global constraint propagation.

---

## 3. Expressions

Operator precedence, lowest to highest. All operators are left-associative
except `**` (right) and the assignment `=` (right).

| Level | Operators |
| --- | --- |
| 1 | `=` (assign), `+=`, `-=`, `*=`, `/=`, `%=` |
| 2 | `or` |
| 3 | `and` |
| 4 | `not` (prefix) |
| 5 | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| 6 | `|`, `^` (bitwise or, xor) |
| 7 | `&` (bitwise and) |
| 8 | `<<`, `>>` |
| 9 | `+`, `-` |
| 10 | `*`, `/`, `%` |
| 11 | `**` (power) |
| 12 | `as` (type cast), unary `-`, `+`, `!` |
| 13 | `.` (field), `[]` (index), `()` (call) |

### 3.1 Casts

`expr as T` for primitive numeric and pointer conversions. There are no
implicit conversions. The cast set is:

- Integer ↔ integer (sign- or zero-extend / truncate).
- Integer ↔ float (round to nearest).
- `bool` → integer (`false` = 0, `true` = 1).
- Reference → raw pointer (in `unsafe` contexts).

### 3.2 Method calls

`receiver.method(args)` resolves to either:

1. A method defined on the inherent `impl` of the receiver's type.
2. A trait method, if the trait is in scope (`use mod { TraitName }`).

### 3.3 Closures (lambdas)

```kryos
let add = fn(a: i64, b: i64) -> i64 { return a + b }
```

Closures capture by **reference** if the captured binding is not mutated
inside the closure, otherwise by **move**. Explicit capture clauses are
not yet supported.

---

## 4. Statements & control flow

### 4.1 `if` / `elif` / `else`

```kryos
if cond1 {
    ...
} elif cond2 {
    ...
} else {
    ...
}
```

`if` is also an expression: every branch must produce the same type, and
the result is the value of the taken branch.

### 4.2 `while`

```kryos
while cond {
    ...
}
```

Conditions must be `bool`. `break` and `continue` work as in C/Rust.

### 4.3 `for ... in`

```kryos
for i in 0..n { ... }            // exclusive range
for x in collection { ... }      // any Iter<T>
```

### 4.4 `loop`

`loop { ... }` is an unconditional infinite loop, exited via `break`,
`return`, or an exception. It desugars to `while true` and works fully
on both backends.

### 4.5 `match`

See [§6](#6-pattern-matching).

### 4.6 `return`

Exits the enclosing function. The expression after `return` must match
the function's declared return type. A function with `-> ()` (or no
return type) may use a bare `return`.

---

## 5. Declarations

### 5.1 Functions

```kryos
fn name<G: Bound>(p1: T1, p2: T2) -> R {
    body
}
```

Functions are declared with `fn`. Parameters and the return type must
be explicitly typed. The body is a block expression; if the function
has a non-unit return type, every control-flow path must produce a
value via `return`.

### 5.2 Structs

```kryos
struct Point { x: i64, y: i64 }

impl Point {
    fn new(x: i64, y: i64) -> Point { return Point { x: x, y: y } }
    fn shift(self: Point, dx: i64) -> Point { return Point { x: self.x + dx, y: self.y } }
}
```

### 5.3 Enums

```kryos
enum Color { Red, Green, Blue }
enum Shape { Circle(f64), Rect(f64, f64) }
```

**Constructing** an enum value uses dot syntax (`Color.Red`,
`Shape.Circle(1.0)`).

**Matching** against an enum pattern uses the path syntax (`Color::Red`,
`Shape::Circle(r)`). This split is unusual but is current behavior.

### 5.4 Traits and impls

```kryos
trait Describable {
    fn describe(self) -> str
}

impl Describable for Point {
    fn describe(self) -> str {
        return "(" + to_string(self.x) + ", " + to_string(self.y) + ")"
    }
}
```

### 5.5 Type aliases

```kryos
type UserId = i64
type Matrix = [[f64]]
```

### 5.6 Extern declarations (FFI)

```kryos
extern "C" {
    fn write(fd: i32, buf: i64, len: i64) -> i64
}
```

All `extern` functions are implicitly `unsafe` — calling them requires
an `unsafe` block.

---

## 6. Pattern matching

```kryos
fn classify(value: i64) -> str {
    match value {
        0           => "zero",
        1 | 2 | 3   => "small",
        n if n < 10 => "single-digit",
        _           => "other",
    }
}
```

### 6.1 Pattern kinds

| Pattern              | Example                  |
| -------------------- | ------------------------ |
| Literal              | `42`, `"yes"`, `-1`, `true` |
| Wildcard             | `_`                      |
| Variable bind        | `n`                      |
| Range                | `0..10`                  |
| Tuple                | `(a, b)`                 |
| Struct               | `Point { x: 0, y }`      |
| Enum variant         | `Color::Red`, `Some(x)`  |
| OR                   | `1 \| 2 \| 3`           |
| Guard                | `n if n > 0`             |

### 6.2 Exhaustiveness

The compiler enforces exhaustiveness for enum and bool matches. A
non-exhaustive match is a hard error; use `_` as the fallback to silence
it.

---

## 7. Ownership and drop order

### 7.1 Ownership rules

1.  Every value has exactly one **owning** binding.
2.  Passing a value to a function or assigning to another binding
    transfers ownership (a **move**), unless the type is `Copy`.
3.  After a move, the source binding is **uninitialized** — reading it
    is `E0300: use of moved value`.
4.  References (`&T`, `&mut T`) borrow without moving. Standard borrow
    checker rules: any number of `&T` *or* exactly one `&mut T` at a time.

### 7.2 Copy types

The following are `Copy`:

- All scalar primitives (`i8`-`i64`, `u8`-`u64`, `f32`, `f64`, `bool`).
- `()` (unit).
- Tuples of `Copy` types.
- `repr(C)` structs whose fields are all `Copy`.

Strings, arrays, maps, channels, and user-defined structs *not* marked
`repr(C)` (or containing non-Copy fields) are **moved**, not copied.

### 7.3 Drop order

When a block exits, bindings are dropped in **reverse order of
declaration**. This applies to:

- `let` bindings inside a block.
- Function parameters (dropped after the last `let`).
- Temporaries (dropped at the end of the statement that created them).

Drop semantics for compound types:

- A struct is dropped by first running its `Drop` impl (if any), then
  dropping its fields in declaration order.
- An enum is dropped by running its `Drop` impl (if any), then dropping
  the data attached to the active variant.

### 7.4 Mutability

`let x = 0` is **immutable** — reassigning is `E0302`. Use `let mut x = 0`
for a mutable binding. Field mutability follows the binding: through a
mutable binding all fields are mutable; through an immutable binding, no
fields are.

---

## 8. Integer overflow

Default arithmetic (`+`, `-`, `*`) on integer types **wraps modulo 2^N**
where N is the type width (64 for `i64`). This matches Rust's release
mode and C's unsigned semantics. There is no compile-time overflow check.

For explicit overflow handling, use the runtime builtins:

| Builtin               | Behavior                                |
| --------------------- | --------------------------------------- |
| `wrapping_add(a, b)`  | Wrap mod 2^64 (same as `+`).            |
| `wrapping_sub(a, b)`  | Wrap mod 2^64.                          |
| `wrapping_mul(a, b)`  | Wrap mod 2^64.                          |
| `checked_add(a, b)`   | **Panic** on overflow (`E0400`).        |
| `checked_sub(a, b)`   | Panic on overflow.                      |
| `checked_mul(a, b)`   | Panic on overflow.                      |
| `saturating_add(a, b)` | Clamp to `i64::MIN`..`i64::MAX`.        |
| `saturating_sub(a, b)` | Clamp.                                 |
| `saturating_mul(a, b)` | Clamp.                                 |

Division by zero panics (`E0401`) regardless of which form is used.
See [docs/16-integer-overflow.md](16-integer-overflow.md) for rationale.

---

## 9. Concurrency

### 9.1 Channels

```kryos
let c = chan()
spawn fn() { send(c, 42) }
let v = recv(c)
```

`chan()` constructs an MPMC channel (multi-producer, multi-consumer).
`send(c, v)` and `recv(c)` are free functions. Channels are typed by
inference from their first send/recv.

Channels are **unbounded** today (sends never block). Bounded channels
are on the roadmap.

### 9.2 `spawn`

`spawn fn() { ... }` runs the closure on a background thread. The
returned handle can be `join`ed to wait for completion.

### 9.3 `async` / `await`

```kryos
async fn fetch(url: str) -> Result<str> { ... }

fn main() {
    let body = await fetch("https://example.com")
}
```

`async`/`await` run on a **cooperative executor** (`coop_spawn` /
`coop_run`): `await` suspends the current task and hands control to the
next ready task, so tasks genuinely interleave (verified on both backends:
`coop_spawn(task_a()) + coop_spawn(task_b()) + coop_run()` prints
`A0 B0 A1 B1 A2 B2`, not `A0 A1 A2 B0 B1 B2`). What is **not** yet shipped
is a non-blocking I/O runtime behind `await` -- an `await` on a blocking
syscall still blocks its OS thread. For CPU-bound parallelism use
`spawn` + channels.

### 9.4 Actors

`ActorName()` spawns an actor on its own OS thread and returns a handle;
`handle.method(args)` sends a message. Handlers run one at a time, in order,
mutating private state via `self.field`. Fully working on both backends.

```kryos
actor Counter {
    count: i64,

    fn inc(self) { self.count = self.count + 1 }
    fn add(self, n: i64) { self.count = self.count + n }
}

fn main() {
    let c = Counter()
    c.inc()
    c.add(10)
}
```

Actors own their state and process one message at a time; the runtime
serializes messages. Handler return values are discarded (fire-and-forget) --
a request-response pattern needs a reply channel. Message arguments (`i64`,
`str`, arrays, ...) are transmitted through the mailbox. At `main` exit the
runtime drains and joins every actor, so all messages sent before `main`
returns are processed with no `sleep` needed. See `examples/actors.kry`.

---

## 10. Unsafe code

`unsafe { ... }` blocks let you perform operations the compiler can't
verify:

- Dereference raw pointers (`*const T`, `*mut T`).
- Call functions declared `extern "C"` or marked `unsafe fn`.
- Read or write `mut static` data.
- Use unions (when supported).

`unsafe` is a **promise**: you've personally verified the invariants of
each unsafe operation. The compiler still type-checks; `unsafe` doesn't
disable any check, it only enables the listed operations.

Using an unsafe operation outside `unsafe { ... }` is `E0500`. The
runtime's own unsafe code is catalogued in
[docs/17-unsafe-audit.md](17-unsafe-audit.md).

---

## 11. Modules and visibility

### 11.1 Module structure

Each `.kry` file is a module. A directory containing `mod.kry` is also a
module. Submodules are accessed with `::`:

<!-- docs-example: skip -->
```kryos
use net.http { HttpClient }
use math.linalg.Matrix
```

### 11.2 Visibility

By default, items are **private to their module**. Mark them `pub` to
export. Visibility applies to:

- Top-level declarations (`fn`, `struct`, `enum`, `trait`, `type`, `const`).
- Struct fields and methods (each can be independently `pub` or private).

A `pub` item is reachable from any module that `use`s the containing module.

### 11.3 `use`

<!-- docs-example: skip -->
```kryos
use std.io                  // import the module
use net.http { HttpClient, get }   // selective import
use math.* { sin, cos, tan }       // group import
```

---

## 12. Runtime panics

A panic is an unrecoverable error that prints a message + stack trace
and exits with **status 98**. An uncaught `throw` or explicit `panic()`
exits with **status 101**. Causes:

- Array index out of bounds (exit 98).
- Division by zero (exit 98).
- `checked_*` overflow.
- Stack overflow — see note below.
- Explicit `panic("message")` (exit 101).

Each panic frame includes the source file and line.

**Stack overflow:** On the Cranelift (`kryos run`) backend, deeply
recursive programs currently trigger a Cranelift verifier error (a
compiler bug, not a clean runtime handler) rather than a graceful panic.
On the LLVM AOT backend the behaviour is OS-defined (SIGSEGV/stack
guard). A clean cross-backend stack-overflow handler is not yet
implemented. See
[docs/17-unsafe-audit.md §5](17-unsafe-audit.md#5-signal-handler-stack_guardrs)
for details.

---

## 13. Conformance checklist

What "implemented" means in 1.0.0-beta.3:

| Feature                     | Status              |
| --------------------------- | ------------------- |
| Primitive types             | Implemented         |
| Generics                    | Monomorphized       |
| Traits + impls              | Implemented         |
| Pattern matching            | Implemented         |
| Ownership + move semantics  | Implemented         |
| Borrow checking (basic)     | Implemented         |
| Drop order                  | Reverse declaration |
| Integer overflow builtins   | Implemented         |
| Stack overflow detection    | Partial — Cranelift: compiler bug (no clean handler); LLVM: OS SIGSEGV |
| Channels (unbounded MPMC)   | Implemented         |
| `spawn` (OS threads)        | Implemented         |
| `async` / `await`           | Cooperative executor (interleaves CPU tasks). Concurrent I/O via spawn/channels/actors (OS threads). |
| Actors                      | Implemented (JIT + AOT) |
| `unsafe` blocks             | Implemented         |
| FFI (extern "C")            | Implemented         |
| Cross-compile (LLVM)        | Implemented         |
| `--explain` for errors      | 32 codes documented |
| Bounded channels            | Not yet             |
| Explicit lifetimes          | Not yet (inferred)  |
| Closures with explicit capture | Not yet          |
| Const generics              | Not yet             |
| Procedural macros           | Not yet             |

This document is updated each release. The grammar lives in
[docs/grammar.md](grammar.md); tutorial chapters live in `docs/01-*` ...
`docs/15-*`.
