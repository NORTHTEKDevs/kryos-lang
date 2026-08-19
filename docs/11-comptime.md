# Compile-Time Evaluation

> **UPDATE 2026-08-14 -- READ THIS FIRST, it corrects the status block below.**
> The block below says a `comptime { }` body is lowered "directly as ordinary
> runtime code, in place, every time control reaches it -- exactly like a bare
> `{ }` block", and that `println` inside one "executes at runtime, printing
> every time". **Both statements are false, and were verified false by
> measurement, not by reading source:**
>
> ```
> comptime { println("INSIDE") }   -> emits NO println into MIR at all; nothing prints
> comptime { n = 99 }              -> survives as `_0 = const 99_i64`; the mutation APPLIES
> ```
>
> MIR lowering keeps only the block's VALUE, so a statement inside one is
> dropped or applied depending on its shape -- a silent, inconsistent split. A
> debug print vanishing while the assignment beside it lands is a trap: the
> reader concludes the block did not run, and is wrong.
>
> `comptime` is therefore now **expression-only**. `let x = comptime { 6 * 7 }`
> is the supported shape (and is what every real use in this repo does). A
> `comptime` block in statement position, or a side-effecting statement inside
> one, is a clean `E0110` compile error. Real compile-time evaluation remains
> deferred past 1.0 per `HANDOFF.md`; until it lands the keyword refuses the
> shapes it cannot honour instead of pretending. See LEDGER item 42; pinned by
> `tests/diagnostics_gate.sh`.

> **Implementation Status (SUPERSEDED by the update above -- retained as the
> historical record of what was believed): `comptime { }` does NOT
> evaluate at compile time.** It is fully parsed (`Expr::ComptimeBlock`) and
> lowered through MIR (`RValue::Comptime(inner)`), but both the Cranelift and
> LLVM backends lower the inner expression **directly as ordinary runtime
> code, in place, every time control reaches it** -- exactly like a bare
> `{ }` block with no `comptime` keyword at all. There is no compile-time
> constant evaluator, no AST-to-literal folding, no isolated interpreter, no
> caching, and no restriction on what the block can do. Concretely, and
> verified by running the compiler, NOT by reading its source:
>
> - A comptime block **reads outer-scope variables** normally.
> - `println` (and any other I/O) **executes at runtime**, printing every time
>   the block runs -- including once per call if it's inside a function called
>   more than once.
> - `file_read`, `http_get`, `env_get`, and every other capability-gated
>   builtin work inside a comptime block exactly as they would outside one,
>   subject to the *same* `@capabilities` rules as the enclosing function --
>   there is no comptime-specific I/O restriction.
> - Nothing is cached. Two calls to a function containing the same comptime
>   block re-run the block's statements both times.
>
> If you are choosing Kryos in part because `comptime` promises isolation or
> determinism, **do not rely on that today.** Everything below the "What Runs
> Today" section describes the *planned* design, clearly marked as such, not
> current behavior.

## Why `comptime { }` exists at all right now

The syntax and MIR plumbing were added ahead of the evaluator so that
`comptime`-using programs parse, type-check, and produce correct *runtime*
answers (a comptime block's value is whatever a plain block containing the
same statements would produce) without breaking once real compile-time
evaluation lands. That is also why the examples throughout this page compute
the right numbers -- runtime evaluation gives the same answer as compile-time
evaluation would, for anything with no I/O and no compile-time-only intent.
The keyword is real, reserved, and forward-compatible; the compile-time
*semantics* it is meant to convey are not implemented yet.

## What runs today

`comptime` is EXPRESSION-ONLY. The block must evaluate to a value; a
side-effecting STATEMENT inside one, or a `comptime` block used in statement
position, is a compile error (`E0110`):

```kryos
fn main() {
    let x: i64 = 7
    let y = comptime {
        x + 1
    }
    println(to_string(y))
}
```

Output:

```
8
```

`x` (an outer-scope runtime variable) is read normally inside the block, and
the block's VALUE is what survives. A comptime block called from a function
invoked N times evaluates N times:

```kryos
fn get_value() -> i64 {
    return comptime {
        42
    }
}

fn main() {
    println(to_string(get_value()))   // prints "42"
    println(to_string(get_value()))   // prints "42" AGAIN
}
```

**Why statements are rejected rather than allowed.** This page previously
showed a `println` inside a `comptime` block and claimed it "executes exactly
once, at the point the block is reached". Measured 2026-08-16 via `--emit-mir`,
that was false, and false in a dangerous direction: MIR lowering keeps only the
block's VALUE, so `comptime { println("x") }` emitted NO println at all and
vanished silently, while `comptime { n = 99 }` survived and applied. A debug
print disappearing while the assignment beside it lands is a trap -- you
conclude the block did not run, and you are wrong. Rather than pick one of
those two behaviours and pretend the keyword means it, both shapes are now
rejected with a diagnostic that names the limitation (LEDGER item 42).

Treat `comptime { EXPR }` as syntactic sugar for `{ EXPR }` today -- a plain
value block, evaluated where it appears, with full access to the enclosing
scope. There is no isolation, no determinism guarantee, and no performance
benefit: it costs exactly what the equivalent bare block would cost, every
time it runs.

## Syntax (this part is real)

```kryos
fn main() {
    let pi = comptime {
        3.14159
    }
    println(to_string(pi))
}
```

Wraps an EXPRESSION in `comptime { }`. Type-checks and runs like
`let pi = { 3.14159 }` would. Statements inside the block, and `comptime` in
statement position, are rejected -- see above.

## Should I use `comptime` today?

There is no reason to reach for it over a plain block or a plain function
call -- it has no effect the language doesn't already give you a normal
block. If your program's *correctness* would depend on the block actually
running at compile time (a lookup table you need embedded in the binary with
no runtime cost, a deterministic-by-construction constant, I/O suppressed at
build time), **that guarantee does not exist yet** -- don't write code that
assumes it. If you just want a computed constant and don't care when it's
computed, a plain `let` is equivalent and clearer:

```kryos
// Equivalent to a comptime block today -- no compile-time work happens
// either way, so this is the honest way to write it:
let table_size = 1024 * 16
```

## Planned design (ASPIRATIONAL -- not implemented)

Everything in this section describes the intended future evaluator. None of
it is true of the compiler today. It is documented here so contributors
building the real evaluator have a target, and so readers can tell "planned"
apart from "current" at a glance.

The planned implementation will:

1. Walk the AST before codegen and find `ComptimeBlock` nodes.
2. Create a fresh, isolated evaluator instance with no access to outer-scope
   runtime state.
3. Execute the block's statements in that evaluator, with I/O (`println`,
   `file_read`, `http_get`, `env_get`, FFI, `spawn`/actors, GPU/quantum ops)
   rejected as compile-time errors -- comptime evaluation is meant to be
   **deterministic**: the same source must always produce the same compiled
   program regardless of what's on disk or the network at build time.
4. Convert the result back into an AST literal node (`IntLiteral`,
   `StringLiteral`, etc.) for primitives and arrays of primitives.
5. Replace the `ComptimeBlock` in the AST with that literal, so the runtime
   program never re-executes the block -- and cache by AST node identity so a
   comptime block reached from a hot path is evaluated once, not once per
   call.

If you need file contents baked into the binary *today*, the honest
workaround is a build script that generates a `.kry` source file containing
the content as a string literal, then compile that -- not `comptime`, which
currently would just read the file at runtime, on every call, gated by
ordinary capability rules (see "What runs today" above).

### Planned use cases (once the evaluator exists)

Precomputed lookup tables, derived constants, and compile-time configuration
values are the intended sweet spot -- moving computation from runtime to
compile time so a 1-second compile-time cost replaces a per-call runtime
cost. None of this happens today; the tradeoff described here (compile time
up, runtime down, possible binary-size increase for embedded data) is a
description of the goal, not a measured property of the current compiler.

### Planned isolation model

The intent is that a comptime block cannot read or write files, make network
requests, access environment variables or arguments, call FFI, use `spawn` or
actors, read variables defined outside the block, call `println` (comptime
output would go nowhere -- a headless evaluator), or touch GPU/quantum ops.
**None of these restrictions exist today** -- see "What runs today" above,
where the exact opposite was demonstrated for outer-variable reads, I/O, and
`println`.

### Planned result types

The eventual evaluator is meant to fold to these AST literal kinds: `int`,
`float`, `str`, `bool`, `none`, and `list` (element-wise), falling back to a
string representation for structs/enums. This table describes the target
folding surface, not a current capability -- today nothing folds; the block's
runtime value is used directly, whatever type it is.

## Comparison with other languages (target design, not current behavior)

These comparisons describe what Kryos `comptime` is *meant* to become, modeled
loosely on Zig's `comptime` blocks (a block of code that runs at compile time
and whose result replaces the block) and contrasted with Rust's `const fn`
and C++'s `constexpr`. Read this section as design intent -- as of today,
Kryos `comptime` is closer to "a block with a reserved keyword in front of
it" than to any of these.

## Summary

| Feature | Today | Planned |
|---|---|---|
| Syntax | `comptime { statements }` -- parses, type-checks | same |
| When it runs | At runtime, in place, every time reached | Before interpretation/codegen |
| Outer-scope variable access | Yes, reads normally | No -- isolated evaluator |
| I/O (`println`, `file_read`, `http_get`, `env_get`, FFI, `spawn`) | Works normally, gated by ordinary `@capabilities` rules | Rejected as a compile-time error |
| Caching / re-evaluation | None -- re-runs every time reached | Cached by AST node identity |
| Determinism guarantee | None (it's runtime code) | Yes, by construction |
| Runtime cost | Same as an equivalent plain block | None -- replaced by a literal |

If you hit unexpected behavior with `comptime`, the most likely explanation
is that you expected the planned column and got the "Today" column. This is
tracked as a known gap, not something to work around per-program -- there is
no current substitute that gives you the planned guarantees; genuine
compile-time constant folding is not available yet.
