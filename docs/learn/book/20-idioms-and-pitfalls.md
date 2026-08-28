# 20 · Idioms & pitfalls

After this chapter you will be able to recognize every named trap this book
has flagged along the way in one place, tell idiomatic Kryos from code that
merely compiles, and -- when you hit a bug this checklist doesn't name --
work through a repeatable method for isolating it instead of guessing. This
is the book's capstone chapter: no new language feature, just the accumulated
sharp edges and the habits that keep you off them.

## The ASI-class trap, in full

Chapter 3 introduced this in miniature. Here is the complete, current
picture, because it is the single mistake most likely to cost you a silent
wrong answer rather than a compile error.

Kryos has no semicolons -- newlines terminate statements -- but the parser
has **no newline awareness at all**. Tokens carry only byte-offset spans, no
line numbers, and the Pratt expression loop keeps consuming any token with
infix binding power regardless of whether a line break sits in front of it.
Four tokens are both a valid first token of a brand new statement *and*
something the parser can read as continuing the previous line's expression:
`||` (boolean-or, or an empty-param closure opener), `-` (subtraction, or a
negative literal), `[` (index access, or an array literal), and `(` (a
function call, or a parenthesized expression). When a fresh line starts with
one of these and the previous line already looked like a complete
expression, the parser always chooses "continue the previous expression" --
silently, with no diagnostic, for as long as this compiler has existed.

Four real, verified shapes -- each one a silent wrong value, not a crash:

```kryos
fn main() {
    let a: bool = false
    let b: bool = true
    let c: bool = a
    || b
    println(to_string(c))
}
```

```
warning[W0001]: this line starts with `||`, which silently continues the PREVIOUS statement's expression as a boolean-or -- if you meant to start a NEW statement (e.g. a closure literal), this merge produces a wrong value with no error
 --> pipe_pipe_trap.kry:5:5
  5 |     || b
    |     ^^ ambiguous: continues the previous line instead of starting fresh
  = note: move the operator to the END of the previous line instead (`a ||`), or restructure so this statement's first token isn't `||`
true
```

The `let c: bool = a` statement's initializer expression doesn't end at that
line -- the parser keeps consuming, sees `|| b` next, and reads the whole
thing as one statement: `let c: bool = a || b`. `a` is `false`, `b` is
`true`, so `c` ends up `true`, not `a`'s value, and not the reader's
apparent intent of a separate closure-literal line whose value gets
discarded. There is no diagnostic-free way to write "a bare closure literal
on its own line, value ignored" directly after a `let` -- the newline that
looked like it separated two statements never did, as far as the parser is
concerned.

The same shape with `-`, `[`, and `(`:

```kryos
fn main() {
    let a = 5
    -1
    println(to_string(a))
}
```

```
warning[W0001]: this line starts with `-`, which silently continues the PREVIOUS statement's expression as subtraction -- if you meant to start a NEW statement (e.g. a negative number literal), this merge produces a wrong value with no error
4
```

`a` is `4`, not `5` -- `let a = 5` merged with `-1` into `let a = 5 - 1`.

```kryos
fn main() {
    let arr = [10, 20, 30]
    let x = arr
    [0]
    println(to_string(x))
}
```

```
warning[W0001]: this line starts with `[`, which silently continues the PREVIOUS statement's expression as an index access -- if you meant to start a NEW statement (e.g. an array literal), this merge produces a wrong value with no error
10
```

`x` is `10` (`arr[0]`), not the whole array followed by an unused `[0]`
literal.

```kryos
fn square(x: i64) -> i64 {
    return x * x
}
@capabilities()
fn main() {
    let f = square
    (5)
    println(to_string(f))
}
```

```
warning[W0001]: this line starts with `(`, which silently continues the PREVIOUS statement's expression as a function call -- if you meant to start a NEW statement (e.g. a parenthesized expression), this merge produces a wrong value with no error
25
```

`f` is `25` (`square(5)`, called immediately), not a stored function value
followed by an unused `(5)` expression.

### The W0001 warning: what it catches and what it deliberately doesn't

All four shapes above now produce `warning[W0001]` -- **a warning, not an
error**: the program still compiles and runs with the merged reading
unchanged, exactly as it always did. The warning only adds visibility; it
does not change behavior, so code written before this diagnostic existed
keeps working identically. `kryos explain W0001` gives the full writeup with
all four examples side by side.

The warning fires only on the **first occurrence** of one of these tokens
while building a given expression -- an established multi-line *chain*,
where the same token already appeared earlier in the same statement, is
common and intentional, and does not warn:

```kryos
fn is_digit(c: str) -> bool {
    return c == "0" || c == "1" || c == "2" || c == "3" || c == "4"
        || c == "5" || c == "6" || c == "7" || c == "8" || c == "9"
}
```

This `is_digit`-style predicate, `matrix[i][j]`-style chained indexing, and
an operator-trailing multi-line subtraction chain all check clean with no
W0001 -- the heuristic specifically targets the *first, surprising*
occurrence, not every legitimate multi-line expression.

**Single `|` is deliberately not covered.** It has the identical hazard (it
is also a closure opener, `|x| ...`), but an empirical sweep of this
repo's own shipped code found newline-led single-`|` bitwise-or
bit-packing to be a common, legitimate pattern -- exactly the shape W0001
would need to flag as the trap, with no way to tell it apart from intent:

```kryos
fn pack(a: i64, b: i64, c: i64, d: i64) -> i64 {
    let v = (a << 24)
        | (b << 16)
        | (c << 8)
        | d
    return v
}
```

This checks clean, no warning. `*` and `&` share the identical unary-vs-infix
grammar collision (dereference/borrow vs. multiply/bitwise-and) and have no
diagnostic at all yet -- W0001 is a partial net, not a complete one. The
underlying rule, unconditionally: **never let a fresh statement's first
token be `-`, `(`, `[`, `|`, or `||`.** Bind a closure via its own
`let name = || ...` rather than a bare trailing line; parenthesize a leading
negative literal; restructure rather than lead with an index or a call.

### `kryos fmt` refuses to launder it

An AST-based formatter re-emits the *parsed* reading in clean canonical
form -- which, for one of these traps, means deleting the exact line break
that was the only visible trace anything was ambiguous, with no warning of
its own (a formatter's own parse historically discarded diagnostics
entirely). `kryos fmt` now refuses instead: it leaves a file with a live
W0001 completely untouched and reports the skip, rather than silently baking
the merged reading into innocent-looking formatted source.

```bash
kryos fmt ambiguous.kry
```

```
  skipped ambiguous.kry (contains an ambiguous newline-led continuation -- run `kryos check ambiguous.kry` to see the W0001 warning; file left untouched)
kryos fmt: skipped 1 file rather than destroy a comment or launder an ambiguous continuation (see above)
```

The file comes back byte-for-byte identical -- not partially reformatted
around the ambiguity, which would itself be a subtler bug. Run `kryos check`
on the reported file, fix the ambiguity by restructuring (never by trusting
`fmt` to guess your intent), then re-run `fmt`.

## The named-trap checklist

Everything else this book has flagged, in one table. Each row is real,
verified behavior, taught in depth in the chapter linked -- this table is
the index, not a replacement for reading the worked example there.

| Trap | The one-line rule | Taught in |
|---|---|---|
| `push` aliasing | `push(arr, v)` mutates the shared buffer in place; always reassign `arr = push(arr, v)`, never read a pre-push alias | [Ch 7](07-collections.md) |
| `any` type erasure | `any` has no runtime type tag; `to_string`/`format` on a `str`/`f64` routed through a direct `any` slot mis-renders it (a container `any` slot is a clean compile error instead) | [Ch 7](07-collections.md), [Ch 9](09-generics-and-traits.md) |
| Struct-argument leak | Passing a struct with heap fields across a call boundary leaks ~85 bytes/call -- read fields directly in a hot loop instead | [Ch 6](06-structs-and-enums.md), [Ch 10](10-ownership-and-arc.md) |
| Capability precision cost | 41 of 75 enumerated pure-closure-through-container shapes need `@capabilities(all)` under the fail-closed design -- deliberate, not a bug | [Ch 11](11-capabilities.md) |
| `Result`/`Option` erasure | A bare, unannotated `Result`/`Option` signature erases the payload to `i64` -- always annotate `Result<T, E>`/`Option<T>` | [Ch 12](12-error-handling.md) |
| `std::result::to_array<T>` | Needs an explicit type annotation to stay type-safe; unannotated it renders a raw pointer | [Ch 12](12-error-handling.md) |
| Two concurrency-primitive hangs | A `ChanWaitGroup` with two `wg_wait` callers only releases one; a shared mutating closure's lock held across a coop-yield point deadlocks | [Ch 13](13-concurrency.md), [Ch 14](14-async.md) |
| `dyn Trait` containers | Cannot be stored inside `[dyn T]`/`Option<dyn T>`/a tuple or map value -- clean `E0110`; use an enum + `match` instead | [Ch 9](09-generics-and-traits.md) |
| `comptime {}` is expression-only | Write `let x = comptime { ... }`; a statement-position block or a side-effecting statement inside one is a clean `E0110` | [Ch 5](05-control-flow.md) |
| Second-alias mutation, backend-divergent | Mutating through a struct alias then reading a *second* alias of the same source agrees on Cranelift, but only the mutated alias sees it on LLVM -- read a field immediately after aliasing, don't rely on a second alias | `CLAUDE.md` gotcha #23 |
| Backend feature gap | `kryos run` (Cranelift) supports fewer codegen paths than LLVM -- if `run` fails but `build --release` works, prefer `--release` | [Ch 18](18-backends.md) |
| wasm's narrower surface | A handful of specific builtins (`round`, global `split`, `to_lower`, `char_from`, array index-*assignment*) are refused at wasm compile time, not silently miscompiled | [Ch 18](18-backends.md) |
| `extern`/`unsafe` boundaries | Non-`kryos_*` externs are rejected (`E0508`); raw-memory builtins need `ffi` but no `unsafe` block; `*T` deref needs `unsafe` (`E0500`) but has no working pointer source today | [Ch 19](19-ffi-and-unsafe.md) |
| The ASI-class continuation trap | Covered in full above | [Ch 3](03-bindings.md), this chapter |

A few more, real but narrower, exist even without a full worked example
here:

- **Narrow-int literals aren't range-checked at compile time.** An
  out-of-range `u8`/`i8`/`u16`/etc. literal truncates silently rather than
  erroring.
- **Shift amount `>=` the operand's bit width is hardware-dependent** (masks
  modulo width on some platforms, doesn't on others) -- keep shift amounts
  strictly less than the operand's width.
- **`gcd`/`lcm` of `i64::MIN`** cannot return the correct positive
  magnitude, because `|i64::MIN|` itself overflows `i64` -- every other
  input is correct.
- **Importing a name that shadows a builtin another imported stdlib module
  uses internally** breaks that module, because imports share one flat
  namespace with no aliasing (`use m::{parse as p}` is a parse error) --
  don't selectively import `contains` from `std::trie`/`set`/`interval` if
  you've also imported something that needs the global `contains` builtin.

## Idiomatic vs. unidiomatic, side by side

Several of the rules above have a clean idiomatic form once you know them.
These pairs are the muscle memory worth building.

<!-- docs-example: skip -->
```kryos
// Unidiomatic: reading a pre-push alias (undefined which state you get)
let a = [1, 2, 3]
let b = push(a, 4)   // mutates a's buffer in place
println(to_string(len(a)))   // don't rely on this

// Idiomatic: always reassign the binding you push through
let mut a = [1, 2, 3]
a = push(a, 4)
println(to_string(len(a)))   // 4, unambiguous
```

<!-- docs-example: skip -->
```kryos
// Unidiomatic: O(n^2) string building in a loop
let mut s = ""
for chunk in chunks {
    s = s + chunk
}

// Idiomatic: O(n) via string_builder
use std::string::{string_builder}
let sb = string_builder()
for chunk in chunks {
    sb.append(chunk)
}
let s = sb.build()
```

<!-- docs-example: skip -->
```kryos
// Unidiomatic: a self-referential closure via reassignment captures the
// OLD value of `fact`, not the one being built -- infinite-recurses into
// garbage or panics, it does not compute a factorial.
let mut fact = |n: i64| n
fact = |n: i64| if n <= 1 { 1 } else { n * fact(n - 1) }

// Idiomatic: a named recursive fn has no such capture problem
fn fact(n: i64) -> i64 {
    if n <= 1 { return 1 }
    return n * fact(n - 1)
}
```

<!-- docs-example: skip -->
```kryos
// Unidiomatic: coarse capability out of habit
@capabilities(all)
fn load_config(path: str) -> str {
    return file_read(path)
}

// Idiomatic: the narrowest sub-capability that covers what the function does
@capabilities(fs:read)
fn load_config(path: str) -> str {
    return file_read(path)
}
```

<!-- docs-example: skip -->
```kryos
// Unidiomatic: trusting a program's outer @capabilities as if it bounded
// every code path inside it at the same precision
@capabilities(fs:read, net)
fn run_plugin(config_path: str) -> str {
    let config: str = file_read(config_path)
    return call_plugin(config)   // plugin logic runs with the FULL outer grant
}

// Idiomatic: deny! narrows authority around the specific span you trust least
@capabilities(fs:read, net)
fn run_plugin(config_path: str) -> str {
    let config: str = file_read(config_path)
    deny!(net) {
        return call_plugin(config)   // plugin logic cannot reach the network
    }
}
```

## How to debug a weird behavior

When something compiles but produces a value you didn't expect, work through
these in order rather than guessing at the language feature that's "probably"
involved -- most weird-behavior reports turn out to be one of the traps
above, and this order finds them fastest.

**1. Run `kryos check` first, even if you already ran the program.**
`kryos check` surfaces every diagnostic the compiler has, including
warnings a plain `run`/`build` might scroll past in other output. If the
mystery is a wrong VALUE rather than a crash, look specifically for
`warning[W0001]` -- it is the single most common source of "this ran but the
number is wrong" reports in this language, per the section above.

**2. If there's an error or warning code, run `kryos explain <code>`
before doing anything else.** The example in this book that started this
habit:

```bash
kryos explain E0505
```

```
E0505: builtin capability violation

A builtin was called in a context lacking the capability that builtin
requires. Builtins like `file_read`/`file_write` (io), `spawn` (process),
and network operations (net) are capability-gated.

Fix: grant the required capability, or avoid the builtin on this path.
```

Every code in this compiler's diagnostics has a matching `explain` entry
with the fix. Reading it is faster than re-deriving the rule from a
one-line error message.

**3. If it's a value bug with no diagnostic at all, check `--emit-mir`
before suspecting the backend.** MIR is what BOTH backends consume -- if the
MIR already shows the wrong computation, the bug (or your misunderstanding)
is in parsing/lowering, shared by every backend, not a codegen issue:

```bash
kryos build mystery.kry --emit-mir
```

Compare what you see against what you *meant* to write. This is exactly how
you'd catch the `-1`-continuation trap independently of the W0001 warning:
the MIR for `let a = 5` followed by a bare `-1` line shows one `sub`
instruction feeding `a`, not two separate statements -- the merge is visible
in the IR even before you know the diagnostic exists.

**4. If the MIR looks right but the RUNTIME behavior is wrong, test both
backends before assuming the bug is universal.** `kryos run file.kry`
(Cranelift) and `kryos build --release file.kry && ./file` (LLVM) should
agree; when they don't, you've found a genuine backend divergence (this
book names the known ones -- second-alias struct mutation, NaN sign bit,
`parse_float("-0.0")`'s sign -- rather than a bug in your program). Chapter 18
covers reading backend-specific output when you need to go further.

**5. Grep `CLAUDE.md`'s gotcha list and `tools/loop/LEDGER.md` by topic
before assuming you found something new.** Most sharp edges in this
language are already named, root-caused, and workaround-documented --
`docs/claude/FULL-REFERENCE.md` has the full history (commit hashes,
resolved-bug narratives) behind the compressed rule in `CLAUDE.md`. If your
symptom matches a LEDGER entry marked OPEN, you've confirmed a known
limitation rather than found a new bug -- the entry's own text usually names
the workaround.

**6. Minimize before you go further.** If none of the above explains it, cut
the program down to the smallest input that still reproduces the behavior --
one file, ideally under 20 lines, no unrelated imports. A minimized repro is
the difference between a bug report someone can act on today and one that
sits in a queue because nobody can tell which of 40 lines matters.

## Exercises

1. Take the `is_digit`-style example from this chapter and delete the last
   `|| c == "9"` line's leading `||`, moving it to trail the previous line
   instead. Run `kryos check` -- does W0001 fire? Why or why not, given the
   "first occurrence" rule?
2. Write a two-line program that trips the `[`-continuation trap on
   purpose, then fix it two different ways: restructure so the second line
   isn't a bare `[...]`, and (separately) merge them onto one line with an
   explicit trailing `[`. Confirm both silence the warning.
3. Pick one row from the named-trap checklist you haven't personally hit
   yet. Write the two-line program that would trip it, run it, and read the
   real diagnostic (or lack of one) yourself rather than trusting this
   table.
4. Take the unidiomatic `@capabilities(all)` example from the side-by-side
   section and run `kryos check --strict-capabilities` against both
   versions. Confirm the narrow version still passes -- if it doesn't,
   which specific sub-capability is missing?

## Summary

- The ASI-class continuation trap covers four tokens (`||`, `-`, `[`, `(`):
  a fresh line starting with one of these silently continues the previous
  statement's expression instead of starting a new one, because the parser
  has no newline awareness. `warning[W0001]` now flags the FIRST occurrence
  of each while leaving the merged (wrong) behavior unchanged for backward
  compatibility -- it's a warning, not an error.
- An established multi-line chain of the same token (an `is_digit`-style
  `||` predicate, chained `[i][j]` indexing, a trailing-operator subtraction
  chain) does not false-positive. Single `|` is deliberately uncovered --
  bitwise bit-packing is a common, legitimate shipped pattern that looks
  identical to the trap.
- `kryos fmt` refuses (skips, byte-identical) any file with a live W0001
  rather than silently baking the merged reading into clean-looking
  formatted source.
- The named-trap checklist above is the index to every sharp edge this book
  taught in depth -- read the linked chapter's worked example, don't just
  memorize the one-liner.
- When debugging a weird behavior: `kryos check` first (catch warnings),
  `kryos explain <code>` for any diagnostic, `--emit-mir` to see the shared
  lowering before suspecting a backend, run both backends before assuming a
  bug is universal, check `CLAUDE.md`/`LEDGER.md` before assuming it's new,
  and minimize before escalating further.

---

This is the last chapter of the book. From here, `docs/learn/cookbook/`'s 27
recipes are reference material for things people actually build, and
`docs/19-language-reference.md` stays the spec to check any of this book's
claims against as the language keeps moving. If you hit a sharp edge this
book didn't name, `tools/loop/LEDGER.md`'s OPEN section is where it either
already lives or belongs.
