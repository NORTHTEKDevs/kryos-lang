# Error and warning code reference

This page indexes every `E`/`W` diagnostic code the Kryos compiler knows
about: the ones registered in `compiler/crates/kryos-errors/src/codes.rs`,
every long-form article in `compiler/crates/kryos-errors/src/explain.rs`
(`kryos explain <CODE>`), and the codes actually emitted by the compiler
today, found by grepping `compiler/crates/*/src` for `.with_code(...)`
call sites. These three sources do not perfectly agree, and this page says
so explicitly rather than silently reconciling them -- see
["Known gaps"](#known-gaps) at the bottom.

Every example on this page was run against the reference compiler
(`compiler/target/release/kryos.exe`, `KRYOS_STDLIB_DIR=$PWD/compiler/stdlib`)
on 2026-08-28. Where the live diagnostic differs from what `kryos explain`
or the code's registered one-line summary implies, this page uses the live
output and flags the discrepancy rather than the aspirational text.

For the full narrative version of any code (more prose, more edge cases),
run `kryos explain <CODE>` -- it's built into the compiler and works
offline. This page is the browsable index; `explain` is the detail view.

## How to read an entry

Each entry gives:

- **Meaning** -- one line, what triggered it.
- **Broken** -- a minimal `.kry` snippet that produces it (marked
  `// ERROR` on the offending line, per this repo's doc convention).
- **Real output** -- the actual `kryos check` diagnostic, copied verbatim.
- **Fix** -- the smallest change that clears it.

---

## Parse errors (E00xx) and warnings (W00xx)

### E0001 -- unexpected token

A top-level declaration couldn't be parsed at all -- the parser found a
token that cannot start a `fn`/`struct`/`enum`/`use`/`let`/... declaration.

<!-- docs-example: skip -->
```kryos
123   // ERROR: not a valid top-level declaration
fn main() { }
```

Real output:

```
error[E0001]: unexpected token integer literal
```

Fix: every top-level item must be a declaration. Stray expressions,
including a `const NAME: TYPE = value` (there is no `const` keyword --
see [glossary](glossary.md#const)), land here.

### E0002 -- expected identifier

An identifier was required and a non-identifier token was found -- this
fires specifically in identifier-list positions like a `deny!(...)`
capability atom list, not in every "expected a name" spot (see E0009 below
for the more common one).

<!-- docs-example: skip -->
```kryos
fn main() {
    deny!(123) {     // ERROR: capability name must be an identifier
        println("x")
    }
}
```

Real output:

```
error[E0002]: unexpected token '123', expected identifier
```

Fix: use a real capability name (`net`, `fs:read`, ...).

### E0003 -- expected expression

The parser wanted a value -- a literal, call, operator chain, block, or
`if`/`match` -- and found something that can't start one.

<!-- docs-example: skip -->
```kryos
fn main() {
    let x = let y = 1     // ERROR: `let` is a statement, not an expression
}
```

Real output:

```
error[E0003]: unexpected token 'let', expected expression
```

Fix: split into two statements.

```kryos
fn main() {
    let y = 1
    let x = y
    println(to_string(x))
}
```

### E0004 -- expected type

A type was required (after `:`, after `->`, in a generic argument list) and
the parser found a non-type token.

<!-- docs-example: skip -->
```kryos
fn add(a: , b: i64) -> i64 { return a + b }   // ERROR: no type after `a:`
fn main() { }
```

Real output (first diagnostic; the parser recovers and reports the
resulting cascade too -- read the first one, the rest are recovery noise):

```
error[E0004]: unexpected token ',', expected type
```

Fix: give every parameter a type.

### E0009 -- syntax error (uncategorized)

The catch-all for a syntax problem that doesn't fit E0001-E0004: a
handful of specific shapes (assignment inside a condition, a stray `;`, an
unterminated string/block comment, `name!(...)` macro-call syntax -- Kryos
has no macros) share this code rather than getting their own.

<!-- docs-example: skip -->
```kryos
fn main() {
    let x: i64 = 1
    if x = 2 {          // ERROR: `=` in a condition, not `==`
        println("no")
    }
}
```

Real output:

```
error[E0009]: assignment `=` is not allowed in a condition
  = note: use `==` to compare
```

Fix: use `==` for comparison; `=` is assignment only.

### E0010 -- program nesting too deep

The compiler bounds syntactic nesting (256 levels) and single-expression
node count (2048 nodes) so that no input, however pathological, can crash
the compiler with a native stack overflow instead of a diagnostic. No
hand-written program approaches these limits -- only generated code does,
so there's no useful minimal example to paste here.

Fix: split a long expression chain into intermediate `let` bindings, or
flatten deep nesting into early returns / `match`.

### W0001 -- ambiguous newline-led continuation

The parser has no line-number awareness (tokens carry only byte-offset
spans), so a fresh line starting with `||`, `-`, `[`, or `(` is ambiguous:
it could be a new statement or a continuation of the previous expression.
The parser always continues, silently, unless this warning is live. See
`CLAUDE.md` hard rule 1 for the full story, including why single `|` and
`*`/`&` are NOT covered by this warning (real-corpus false-positive risk
for `|`; ungated entirely for `*`/`&`).

<!-- docs-example: skip -->
```kryos
fn check_a() -> bool { return true }
fn check_b() -> bool { return true }

fn main() {
    let ready: bool = check_a()
    || check_b()          // ERROR (W0001): silently reads as `check_a() || check_b()`
    println(to_string(ready))
}
```

Real output:

```
warning[W0001]: this line starts with `||`, which silently continues the
PREVIOUS statement's expression as a boolean-or -- if you meant to start a
NEW statement (e.g. a closure literal), this merge produces a wrong value
with no error
```

Fix: never let a fresh statement's first token be `-`, `(`, `[`, `|`, or
`||`. Bind a closure via its own `let name = || ...` instead of leaving it
as a bare trailing statement.

---

## Type errors (E01xx)

### E0100 -- type mismatch

An expression's actual type doesn't match what the surrounding context
requires. Kryos has no implicit numeric/bool/string conversions.

<!-- docs-example: skip -->
```kryos
fn double(n: i64) -> i64 { return n * 2 }
fn main() {
    let x: f64 = 1.5
    print(to_string(double(x)))   // ERROR: f64 passed where i64 required
}
```

Real output:

```
error[E0100]: type mismatch: expected `i64`, found `f64`
```

Fix: convert explicitly (`i64_to_f64`/`f64_to_i64`) or change the
declared type.

```kryos
fn double(n: i64) -> i64 { return n * 2 }
fn main() {
    let x: i64 = 1
    print(to_string(double(x)))
}
```

### E0101 -- unknown type

The name used as a type isn't visible: misspelled, or its module isn't
imported.

<!-- docs-example: skip -->
```kryos
fn area(r: Float) -> Float { return 3.14 * r * r }   // ERROR: no type `Float`
fn main() { }
```

Real output:

```
error[E0101]: unknown type `Float`
```

Fix: Kryos's float type is `f64` (or `f32`).

### E0102 -- undefined variable

The identifier was used as a value but nothing binds it in scope -- typo,
or a `let` that only exists inside an inner `{ }` block.

<!-- docs-example: skip -->
```kryos
fn main() {
    if true {
        let x = 1
    }
    print(to_string(x))     // ERROR: x is out of scope here
}
```

Real output:

```
error[E0102]: undefined variable `x`
```

Fix: declare the binding in the scope that needs it.

### E0103 -- unknown struct

A struct literal or pattern names a struct that was never declared or
imported.

<!-- docs-example: skip -->
```kryos
fn main() {
    let p = Point { x: 1, y: 2 }   // ERROR: Point is not declared
}
```

Real output:

```
error[E0103]: unknown struct `Point`
```

Fix: declare it (`struct Point { x: i64, y: i64 }`) or import it.

### E0104 -- wrong number of arguments (registered, never emitted -- see gaps)

Registered in `codes.rs` and documented in `explain.rs` as "wrong number
of arguments", but no emission site in the compiler actually uses this
code today. The real diagnostic for a call with the wrong argument count
is **E0110** (general type error):

```
error[E0110]: function `add` expects 2 arguments, found 1
```

See [Known gaps](#known-gaps).

### E0105 -- unknown trait

A trait name in `impl X for Y`, a trait bound, or a `dyn Trait` type
doesn't resolve.

<!-- docs-example: skip -->
```kryos
struct Point { x: i64, y: i64 }
impl Printable for Point { fn show(self) { } }   // ERROR: Printable undeclared
fn main() { }
```

Real output:

```
error[E0105]: unknown trait `Printable` in `impl Printable for Point` --
the trait is not declared; a misspelled trait name silently becomes an
unchecked inherent impl
```

Fix: declare the trait, or fix the spelling.

### E0106 -- no such field

The field accessed doesn't exist on that struct type.

<!-- docs-example: skip -->
```kryos
struct Point { x: i64, y: i64 }
fn main() {
    let p = Point { x: 1, y: 2 }
    print(to_string(p.z))     // ERROR: no field `z`
}
```

Real output:

```
error[E0106]: no field `z` on type `Point`
```

Fix: use an existing field name, or add the field to the struct.

### E0107 -- no such method

The method called doesn't exist for the receiver's type -- misspelled, or
defined by a trait that isn't imported.

<!-- docs-example: skip -->
```kryos
struct Vec3 { x: f64, y: f64, z: f64 }
impl Vec3 { fn length(self) -> f64 { return 0.0 } }
fn main() {
    let v = Vec3 { x: 1.0, y: 2.0, z: 3.0 }
    let n = v.norm()          // ERROR: no method `norm`
}
```

Real output:

```
error[E0107]: no method `norm` found for type `Vec3`
```

Fix: call `.length()`, or import the trait that defines the method.

### E0108 -- missing fields in struct literal (registered, never emitted -- see gaps)

Registered in `codes.rs` and documented in `explain.rs` as "missing fields
in struct literal", but the actual emission site is **E0100** (type
mismatch):

```
error[E0100]: missing field `y` in `Point` literal -- every field must be
initialized (Kryos has no default field values)
```

See [Known gaps](#known-gaps).

### E0109 -- `Self` used outside of impl/trait

`Self` and `self` only have meaning inside an `impl` or `trait` block.

<!-- docs-example: skip -->
```kryos
fn make() -> Self { return Self { } }   // ERROR: no enclosing impl/trait
fn main() { }
```

Real output (the checker reports both the misuse and the resulting
unresolved-type fallout in the same pass):

```
error[E0109]: `Self` used outside of impl or trait block
error[E0103]: unknown struct `Self`
```

Fix: move the function inside an `impl` block.

```kryos
struct Point { x: i64, y: i64 }
impl Point {
    fn make() -> Point { return Point { x: 0, y: 0 } }
}
fn main() {
    let p: Point = Point::make()
    println(to_string(p.x))
}
```

### E0110 -- type error (general)

The catch-all for a type problem that doesn't fit E0100-E0109: wrong
generic argument count, wrong argument count on a call (see E0104's gap
note above), an operation applied to an incompatible type. Read the
message; it names the specific problem.

### E0111 -- integer literal out of range for declared type

An integer literal doesn't fit the narrow integer type it's assigned to.
Before this check existed, the literal silently truncated (`999` as `u8`
became `231`) -- a source-level data-corruption bug this code closed.

<!-- docs-example: skip -->
```kryos
fn main() {
    let small: u8 = 999     // ERROR: u8 holds 0..=255
}
```

Real output:

```
error[E0111]: integer literal `999` is out of range for `u8` (valid range: 0..=255)
```

Fix: use a wider type, or cast explicitly if truncation is genuinely
intended (`(999 as u8)`).

### E0112 -- non-exhaustive match

A `match` over an enum must cover every variant, or supply a wildcard `_`.

<!-- docs-example: skip -->
```kryos
enum Color { Red, Green, Blue }
fn main() {
    let c: Color = Color::Red
    match c {                        // ERROR: Blue is not covered
        Color::Red => println("r"),
        Color::Green => println("g"),
    }
}
```

Real output:

```
error[E0112]: non-exhaustive match: missing variant(s) `Blue`
```

Fix: add the missing arm, or a `_ => ...` catch-all.

### E0113 -- generic monomorphization resource limit exceeded

A compile-time denial-of-service guard: bounds recursive generic
instantiation depth, total distinct instantiation count, and per-type
structural size, so a generic that doubles its own type on every call
(`fn dup<T>(x: T) -> (T, T)` chained repeatedly) or a self-recursive
generic that grows its type at every level can't hang or OOM the compiler
on a tiny source file. No hand-written program gets near these limits, so
there's no useful minimal example -- see `kryos explain E0113` for the two
shapes that do trigger it and how to restructure around each.

---

## Resolution errors (E02xx)

### E0200 -- module path could not be resolved

A `use std::<module>::{...}` (or project-local import) names a module that
doesn't exist on disk, or whose own file fails to parse.

<!-- docs-example: skip -->
```kryos
use std::strnig::{trim}    // ERROR: typo, no such module
fn main() {
    let s: str = trim("  x  ")
    println(s)
}
```

Real output -- for a genuinely missing module, the live diagnostic has
**no `[E0200]` tag at all**, even though the code is wired via
`.with_code(codes::E0200)` in `kryos-driver/src/resolve.rs`:

```
error: module `std::strnig` not found; searched: <path>\std\strnig.kry, ...
```

This is a real gap between the source and the rendered output -- see
[Known gaps](#known-gaps). `E0200` DOES render correctly for the sibling
case (the imported module resolves to a real file, but that file itself
fails to parse) -- only the "module doesn't exist" bail path loses the tag.

Fix: check the module name against `docs/19-language-reference.md`'s
stdlib list, or the project's own layout for a local module.

### E0201 -- qualified call resolves to a different module than named

Kryos has one flat function namespace with no import aliasing.
`Mod::name(...)` is sugar for the flat name `name`, valid only when `name`
was imported FROM `Mod`.

<!-- docs-example: skip -->
```kryos
use std::csv::{parse}
fn main() {
    let v = json::parse("x")   // ERROR: `parse` in scope came from csv, not json
}
```

Real output:

```
error[E0201]: `json::parse` refers to `parse` imported from `csv`, not `json`
```

Fix: import the name from the module you actually intend to call.

```kryos
use std::json::{parse, is_number}
fn main() {
    let v = parse("42")
    println(to_string(is_number(v)))
}
```

### E0202 -- qualified call names a symbol that was never imported

`Mod::name(...)` requires `name` to already be in scope via `use` -- the
qualifier is sugar, not an alternate lookup path.

<!-- docs-example: skip -->
```kryos
use std::json::{stringify}
fn main() {
    let v = json::parse("x")   // ERROR: `parse` was never imported
}
```

Real output:

```
error[E0202]: `json::parse` is not imported: add `parse` to the `use` list for `json`
```

Fix: add the name to the `use` list.

### E0203 -- import of a private/internal module member

Only public (non-underscore-prefixed) names are importable.

<!-- docs-example: skip -->
```kryos
use std::os::{_env_or_empty}   // ERROR: leading underscore = internal
fn main() { }
```

Real output:

```
error[E0203]: `_env_or_empty` is a private/internal member of module
`std::os` and cannot be imported
```

Fix: call the module's public wrapper, or the underlying global builtin
directly (e.g. `env_get(...)`).

### E0204 -- module has no export by that name

Not a visibility problem (that's E0203) -- the name simply doesn't exist
in that module.

<!-- docs-example: skip -->
```kryos
use std::string::{capitalize_words}   // ERROR: not a real export
fn main() { }
```

Real output:

```
error[E0204]: module `std::string` has no export `capitalize_words`
```

Fix: check the exact name against `kryos doc` output or
`docs/19-language-reference.md`; don't guess a plausible stdlib name.

### E0205 -- duplicate name imported from multiple modules

One flat namespace, no aliasing: two `use` statements bringing in the same
name from different modules collide.

<!-- docs-example: skip -->
```kryos
use std::csv::{parse}
use std::json::{parse}    // ERROR: `parse` already imported from csv
fn main() { }
```

Real output:

```
error[E0205]: duplicate function `parse` imported from multiple modules
```

Fix: import only the module you need `parse` from; reach the other one
through a qualified call, or restructure to import disjoint names.

---

## Ownership errors (E03xx) and warnings (W03xx)

Read this section together with `CLAUDE.md`'s value-semantics section
before trusting any specific example below: ARC-backed values (`str`,
`[T]`, `map<K, V>`, structs) are largely **reused safely after being
passed**, and this whole diagnostic family is documented as advisory, not
a hard block, for exactly that reason.

### E0300 -- use of moved value

Documented as firing when a value is used after being moved into a
function that took ownership. **Live-tested 2026-08-28: this did not
fire** for a struct-with-array-field passed to a function and read again
afterward -- `kryos check` returned clean, matching the "advisory, may not
block reuse" contract this code's own compiler comments describe (see
`kryos-ownership/src/analysis.rs`, which documents several historical
false-positive fixes for exactly this code). Treat any specific "still
blocks this" example for E0300 as unverified until you reproduce it
against the current compiler -- don't assume the shape in `kryos explain
E0300` still reproduces.

### E0301 -- use of uninitialized value

A `let mut` binding with no initializer, read on a control-flow path that
didn't assign it first.

<!-- docs-example: skip -->
```kryos
fn some_condition() -> bool { return true }
fn main() {
    let mut x: i64
    if some_condition() {
        x = 1
    }
    print(to_string(x))    // ERROR: x may be uninitialized here
}
```

Real output:

```
error[E0301]: use of uninitialized variable: `x`
```

Fix: initialize on every path, or at declaration.

```kryos
fn some_condition() -> bool { return true }
fn main() {
    let mut x: i64 = 0
    if some_condition() {
        x = 1
    }
    print(to_string(x))
}
```

### E0302 -- assignment to immutable variable

`let` is immutable by default; reassigning needs `let mut`.

<!-- docs-example: skip -->
```kryos
fn main() {
    let x = 1
    x = 2      // ERROR: x is not `mut`
}
```

Real output:

```
error[E0302]: assignment to immutable variable `x`
```

Fix: `let mut x = 1`.

### E0303 -- use of partially moved value

Documented as firing when one field of a struct is moved out (passed by
value to a function) and then the same field is read again. **Live-tested
2026-08-28: this also did not fire**, for the same reason as E0300 above
-- struct-field reuse-after-pass compiled clean. Same caveat: don't trust
a specific "this blocks" example without re-testing it.

### W0300 -- conditional move

Documented as a warning when a value moves on one `if`/`match` branch but
not the other. **Live-tested 2026-08-28: did not fire** on the
struct-move shape from `kryos explain W0300` -- consistent with E0300/
E0303 above; the whole move-diagnostic family appears to have been
relaxed further than the `explain.rs` articles currently describe.

---

## Runtime panics with an explain article, but no compile-time code (E04xx)

E0400 and E0401 are **not compiler diagnostics** -- there is no
`.with_code(codes::E0400)` (or E0401) anywhere in the compiler, and
neither constant exists in `codes.rs`'s registry. They are runtime PANIC
messages that happen to have a long-form `kryos explain` article for
discoverability. Running the snippets below never prints `error[E0400]` or
`[E0401]` -- it prints a bare `kryos panic: ...` line at a nonzero exit
code. See [Known gaps](#known-gaps).

### E0400 -- integer overflow in checked operation

`checked_add`/`checked_sub`/`checked_mul` panic on i64 overflow (unlike
the default `+`/`-`/`*`, which wrap). Choose `wrapping_add` (silent wrap,
fastest), `checked_add` (panics -- use when overflow is a bug), or
`saturating_add` (clamps to `i64::MIN`/`i64::MAX`) for the semantics you
want. See `docs/16-integer-overflow.md`.

### E0401 -- stack overflow (recursion too deep)

Unbounded recursion exhausts the default 8 MB stack. Kryos installs a
SIGSEGV handler at program start specifically so this prints a
`kryos panic: stack overflow (possible infinite recursion)` message and
exits 134, instead of a silent crash. Fix: add a base case, convert to
iteration, or raise the thread stack via `spawn_with_stack(bytes, fn)` for
deep-but-bounded recursion.

---

## Unsafe and capability errors (E05xx) and warnings (W05xx)

### E0500 -- unsafe operation outside `unsafe` context

Dereferencing a raw pointer (`*p`), calling an `unsafe`/`extern` function,
or reading `mut static` data outside an `unsafe { }` block.

<!-- docs-example: skip -->
```kryos
@capabilities(ffi)
fn main() {
    let addr: i64 = alloc(8)
    let p: *i64 = (addr as *i64)
    let v: i64 = *p       // ERROR: raw deref outside unsafe
    println(to_string(v))
}
```

Real output:

```
error[E0500]: dereference of raw pointer requires an `unsafe` block
```

Fix: wrap the operation.

```kryos
@capabilities(ffi)
fn main() {
    let addr: i64 = alloc(8)
    let p: *i64 = (addr as *i64)
    let v: i64 = unsafe { *p }
    println(to_string(v))
}
```

`unsafe` is a promise, not an escape hatch: it enables the listed
operations, it does not disable any check the compiler otherwise runs.

### E0501 -- capability import violation

An import brings in a function whose capability requirements exceed what
the importing scope may grant; rejected at the `use` site, not the call
site. Fix: grant the capability on the enclosing function, or import a
more attenuated API.

### E0502 -- missing required capability

Documented as the general "function calls something it lacks the
capability for" code. **Live-tested 2026-08-28:** a direct builtin call
missing its capability (both inferred and `--strict-capabilities` modes)
actually surfaces as **E0505** (below), not E0502 -- see
[Known gaps](#known-gaps) for the two codes' overlap. Reserve E0502 in
your mental model for cases E0505/E0506/E0507 don't more specifically
cover.

### E0503 -- capability attenuation violation

A child scope tried to claim a capability broader than the one its parent
handed it -- attenuation may only narrow, never widen. Fix: request only a
subset of the parent's capabilities.

### E0504 -- capability escalation

Code tried to acquire a capability it was never granted anywhere in the
call graph. Capabilities flow strictly downward from the entry point; they
cannot be conjured locally. Fix: thread the capability in from a caller
that legitimately holds it.

### E0505 -- builtin capability violation

A gated builtin (`file_write`, `spawn`, `http_get`, ...) was called
without the capability it requires. This is the code that actually fires
for the common "I forgot the annotation" case, in both `inferred` (the
default) and `--strict-capabilities` mode.

<!-- docs-example: skip -->
```kryos
fn main() {
    file_write("out.txt", "hi")   // ERROR: fs:write not granted
}
```

Real output:

```
error[E0505]: builtin `file_write` requires `fs:write` capability
  = note: add `@capabilities(fs:write)` to the enclosing function or actor
```

Fix:

```kryos
@capabilities(fs:write)
fn main() {
    file_write("out.txt", "hi")
}
```

### E0506 -- FFI capability violation

Calling an `extern` function requires the `ffi` capability -- FFI is
unverifiable by the compiler, so it's treated as maximally privileged
regardless of which specific extern function is called.

<!-- docs-example: skip -->
```kryos
extern "C" {
    fn kryos_env_get(key_ptr: i64, key_len: i64, out_ptr: i64) -> i64
}
fn main() {
    let n: i64 = kryos_env_get(0, 0, 0)   // ERROR: ffi not granted
    println(to_string(n))
}
```

Real output:

```
error[E0506]: extern function `kryos_env_get` requires `process` capability
```

(The specific sub-capability named depends on which `kryos_*` runtime
symbol is behind the extern -- here `process`, because this symbol backs
`env_get`; a non-`kryos_*` extern always demands plain `ffi`.) Fix: grant
the capability, and prefer wrapping raw FFI behind a small, audited,
capability-attenuated module rather than calling it from `main` directly.

### E0507 -- capability propagation violation

An intermediate function calls something that needs a capability the
intermediate function itself doesn't declare, so the requirement can't
propagate up to ITS caller.

<!-- docs-example: skip -->
```kryos
@capabilities(fs:write)
fn save(x: str) {
    file_write("out.txt", x)
}
@capabilities(fs:read)
fn main() {
    save("hi")    // ERROR: main only has fs:read, save needs fs:write
}
```

Real output:

```
error[E0507]: call to `save` requires capabilities [fs:write] not granted to caller
  = note: function `save` has @capabilities(fs:write) but caller lacks [fs:write]
```

Fix: grant the missing capability on the caller too.

### E0508 -- unsupported extern declaration shape

Two rejected shapes: (1) a non-`kryos_*` extern name (real C-library FFI
-- not implemented; the parameter/symbol info never reaches codegen, so
such a declaration either fails to link or silently collides with an
unrelated Kryos builtin of the same name); (2) a `kryos_*`-prefixed name
hand-declared with a `str`/array/map/struct/enum/fn-typed parameter or
return -- the real runtime symbol expects raw pointer/length pairs, and a
mismatched hand-declared signature reads/writes through the wrong pointer
shape at runtime (segfault). Fix: call the documented builtin/`std::`
wrapper instead of hand-declaring the raw symbol; see
`compiler/stdlib/os.kry` for the correct raw-signature pattern if you're
adding a new wrapper.

### W0400 -- tracked value discarded

A `Tracked<T>` value (from `std::tracked`) carries provenance/lineage
metadata; discarding one without using it silently loses that lineage.
Warning only -- the program still compiles and runs. Fix: use the value,
or discard it explicitly with `tracked_discard(t, "reason")`.

### W0500 -- unrecognized capability name in `deny!()`

A `deny!(...)` block names a capability the compiler doesn't recognize,
so the block silently protects nothing.

<!-- docs-example: skip -->
```kryos
fn fetch_data() { }
fn main() {
    deny!(newtork) {         // ERROR (W0500): typo, denies nothing
        fetch_data()
    }
}
```

Real output:

```
warning[W0500]: deny!(newtork) names no recognized capability; the block has no effect
```

Fix: use a recognized name (`net`, `fs:read`, `fs:write`, `db`, `crypto`,
`ffi`, `process`, ...). This is a warning so existing programs keep
compiling, but treat a misspelled `deny!` as an error in security-
sensitive code -- it provides zero sandboxing as written.

### W0505 -- strict-capabilities would reject this function (emitted, unregistered -- see gaps)

Not in `codes.rs` and has no `kryos explain` article, but IS a real,
live-emitted diagnostic -- from the LSP's editor-integration path
(`kryos-lsp/src/cap_surface.rs`), not from `kryos check` on the CLI. It
warns, inline in the editor, on every top-level unannotated function whose
computed capability surface is non-empty -- i.e. exactly the functions
`--strict-capabilities` would reject. See
[Known gaps](#known-gaps).

---

## Known gaps

These are concrete mismatches between the three sources (`codes.rs`
registry, `explain.rs` articles, and what the compiler actually emits),
found by cross-checking all three against live `kryos check` output on
2026-08-28. None of these are hidden elsewhere in the docs; they're
recorded here because a reference page that silently smooths over its own
sources isn't trustworthy.

**Registered + explained, but never emitted (dead codes):**

- **E0104** ("wrong number of arguments") -- no emission site anywhere in
  the compiler. The real scenario emits **E0110** instead.
- **E0108** ("missing fields in struct literal") -- no emission site. The
  real scenario emits **E0100** instead.

**Emitted, but missing from the registry and from `kryos explain`:**

- **W0505** ("strict-capabilities would reject this function") -- real
  code, LSP-only (`kryos-lsp/src/cap_surface.rs`), not in `codes.rs`, no
  `explain` article. `kryos explain W0505` returns "no explanation
  available" today.

**Explained, but not a real compiler diagnostic code at all:**

- **E0400**, **E0401** -- both are runtime panic message topics with a
  `kryos explain` article for discoverability, not codes any
  `.with_code(...)` call site in the compiler actually attaches. They
  never appear as `error[E04xx]` in real output; they appear as a bare
  `kryos panic: ...` line.

**Documented behavior that doesn't currently reproduce live:**

- **E0300**, **E0303**, **W0300** -- the "moved struct field reused"
  and "conditionally moved" repro shapes described in
  `explain.rs`/`kryos explain` did not fire against the current compiler
  in this page's live testing (2026-08-28). This matches the ownership
  checker's own source comments (`kryos-ownership/src/analysis.rs`),
  which record a string of false-positive fixes for exactly these codes,
  and matches `CLAUDE.md`'s documented value-semantics contract that
  ARC-backed reuse-after-pass is safe. Don't treat the `explain` examples
  for these three as current ground truth without re-testing them.
- **E0502** ("missing required capability") -- the shapes tested for this
  page (direct builtin call missing its capability, in both `inferred`
  and `--strict-capabilities` modes) surfaced as **E0505** instead. E0502
  may cover a narrower shape than its one-line summary suggests; this
  page could not isolate one.
- **E0200** -- renders correctly (`error[E0200]: ...`) when an imported
  module resolves to a real file that itself fails to parse, but a
  genuinely nonexistent module path (`use std::typo::{x}`) prints
  `error: module ... not found; searched: ...` with **no `[E0200]` tag**,
  even though `kryos-driver/src/resolve.rs` does call
  `.with_code(codes::E0200)` on that path. Some earlier bail-out in the
  resolution flow evidently doesn't reach the tagged renderer for this
  specific case.

If you're fixing one of these in the compiler, `tools/loop/LEDGER.md` is
where the fix should be recorded per this repo's standing process; this
page should be updated in the same change.
