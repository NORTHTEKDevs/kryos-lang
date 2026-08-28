# Glossary

One precise sentence per term, in the sense Kryos uses it -- not the sense
the word carries in Rust, Go, or another language you might be coming
from, where that differs. Each entry links to where the term is actually
taught, not just mentioned. Terms are alphabetical within each group;
groups follow the rough order a reader meets them in
[`docs/learn/book/`](learn/book/README.md).

For canonical error-code text (as opposed to prose vocabulary), see
[`docs/error-codes.md`](error-codes.md).

## Core execution model

### Backend
One of the three code generators a `.kry` source file compiles through:
**Cranelift** (`kryos run`, fast debug JIT-via-subprocess), **LLVM**
(`kryos build --release`, optimizing AOT), or **wasm** (`--backend wasm`,
experimental, JS-host contract). Never "target" for this -- target is the
OS/arch triple, a separate axis. Taught in
[Chapter 1](learn/book/01-hello.md) and
[`docs/15-codegen.md`](15-codegen.md).

### AOT (ahead-of-time)
Compiling all the way to a native executable before running it --
`kryos build --release`, via the LLVM backend. Contrast **JIT**.

### JIT (just-in-time)
Kryos's Cranelift path (`kryos run`) compiles to native code and executes
it via a subprocess in one step, without producing a standalone binary on
disk. Despite the name, this is not an in-process bytecode interpreter --
it's still native codegen, just invoked per-run instead of once.

### `kryos check`
Type-checks and runs ownership/capability analysis without generating
code -- the fastest feedback loop, used by both the editor (`kryos-lsp`)
and this repo's docs-examples CI gate.

### Self-hosting
The Kryos compiler's own frontend, partially reimplemented in Kryos itself
under `compiler/self-host/` (~19k lines), verified stage-by-stage
(`kryos check` on every self-host file, then a real parse-and-run smoke
test) as a proof the language is expressive enough to write a real
compiler in. See the `selfhost-stage1` CI job in
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml).

## Values and ownership

### ARC (automatic reference counting)
The mechanism behind `str`, `[T]`, `map<K, V>`, and struct/enum value
sharing: every heap allocation carries a live count of references to it,
incremented on share and decremented (freeing at zero) on scope exit.
Never call this "garbage collection" (no cycle collector, no GC pause) or
"ownership" alone (Rust's move-based ownership is a different model --
Kryos values are shared by refcount, not moved, in the common case). Taught
in [Chapter 10](learn/book/10-ownership-and-arc.md).

### Value semantics (reuse-after-pass)
The observable consequence of ARC: passing a `str`/`[T]`/`map`/struct to a
function shares the underlying data via a cheap refcount bump, so the
caller's original binding is still valid and usable after the call
returns -- unlike a destructive move, no `.clone()` is needed to keep using
a value you just passed somewhere. Taught in
[Chapter 10](learn/book/10-ownership-and-arc.md); the hard rule is also in
this repo's `CLAUDE.md`.

### Box
An ARC-managed heap allocation -- what a `str`/`[T]`/`map`/struct handle
actually points at. Only use this word for the runtime allocation itself;
it is never a verb for "wrap in `Option`" (that's "wrap" or "construct
`Some(..)`").

### Move (advisory)
Kryos's ownership checker (`kryos-ownership`) still tracks a notion of a
value being "moved" into a callee and flags reuse with `E0300`/`E0303`,
but per this repo's documented contract that diagnostic is advisory, not a
hard compile block, for ARC-backed types -- see
[`docs/error-codes.md`](error-codes.md#ownership-errors-e03xx-and-warnings-w03xx)
for what currently fires live versus what the diagnostic text describes.

### Copy type
A primitive (`i64`, `f64`, `bool`) or a `@copy`-annotated all-scalar
struct: duplicated at the point of use instead of sharing a heap handle,
so there is nothing to refcount. Taught in
[Chapter 10](learn/book/10-ownership-and-arc.md).

### Mutability (binding vs. value)
`let` is an immutable binding by default; `let mut` allows reassignment.
This is independent of the pointed-to value's own mutability -- reassigning
`x` and mutating a field reached through `x` (`x.field = ...`) are
different operations gated by different rules. Taught in
[Chapter 3](learn/book/03-bindings.md).

## Capabilities

### Capability
A declared, compile-time-checked grant of authority to use a class of
system resource (`fs:read`, `net:http`, `process`, `crypto`, `ffi`, ...).
Never "permission" (a runtime/OS concept; Kryos capabilities are static)
or "scope" (a different, unrelated term for variable visibility). Taught
in [Chapter 11](learn/book/11-capabilities.md) and
[`docs/10-capabilities.md`](10-capabilities.md).

### Sub-capability
A finer-grained capability written `family:scope` (`net:http`, `net:tcp`,
`fs:read`, `fs:write`) under a coarser family (`net`, `fs`/`io`). A
function declaring the coarse family gets every sub-capability under it;
declaring only a sub-capability is the least-privilege default and does
NOT imply the others. See the "Builtins available everywhere" section of
`CLAUDE.md` for the full builtin-to-sub-capability table.

### Capability mode
One of three enforcement levels, set via `--capabilities-mode=<mode>` or
`kryos.toml`: **inferred** (the default -- deny-by-default at the
boundary; only `pub` functions need `@capabilities(...)`), **strict**
(`--strict-capabilities` -- every function must declare its own set), and
**permissive** (only annotated functions are checked at all; used by the
docs-examples CI gate so an illustrative snippet doesn't need capability
noise to make its point).

### Attenuation
Narrowing a capability set as it's threaded down the call graph -- a
callee may request a SUBSET of what its caller holds, never more.
Violating this (claiming something broader) is `E0503`.

### Capability escalation
Acquiring a capability that was never granted anywhere upstream in the
call graph -- capabilities flow strictly downward from the entry point and
cannot be conjured locally. This is `E0504`.

### `deny!()` block
A lexical capability-revocation block (`deny!(net) { ... }`) that rejects
the named capabilities for the code inside it, even if the enclosing
function holds them -- a sandboxing tool for running untrusted logic
inside an otherwise-privileged function. A misspelled capability name
inside one is `W0500` and silently protects nothing.

### Capability surface
The full set of capabilities a function body requires, computed by
walking every builtin/qualified call it makes (transitively, through the
call graph). What `--strict-capabilities` compares your `@capabilities(...)`
annotation against.

## Types and the type system

### Trait
A named set of method signatures a type can implement (`impl Trait for
Type`), used for both static dispatch (a generic bound) and dynamic
dispatch (`dyn Trait`). Taught in
[Chapter 9](learn/book/09-generics-and-traits.md).

### `dyn Trait`
A single trait-object value/param/field/return/let-binding with dynamic
dispatch. Cannot currently be stored INSIDE a container (`[dyn Shape]`,
`Option<dyn Shape>`) -- that's a compile-time `E0110`; use an enum + `match`
for a heterogeneous collection instead. Taught in
[Chapter 9](learn/book/09-generics-and-traits.md).

### Monomorphization
The compiler's process of generating one concrete, fully-typed copy of a
generic function/struct per distinct type it's instantiated with. Bounded
on three axes (instantiation depth, total count, per-type structural size)
specifically to keep a pathological generic from hanging or exhausting
memory at compile time -- see `E0113` in
[`docs/error-codes.md`](error-codes.md#e0113----generic-monomorphization-resource-limit-exceeded).

### Row (compiler-internal -- not reader-facing)
`kryos-types`'s internal representation for capability-checker
polymorphism (`deny!` block enforcement, per-function capability
inference). This is implementation vocabulary; user-facing docs should say
"the capability checker" and describe observable behavior instead.

### `comptime` block
An expression-only block (`let x = comptime { 6 * 7 }`) whose VALUE is
kept during MIR lowering. It is NOT a general compile-time evaluator (real
compile-time evaluation is deferred past 1.0) and is NOT valid in
statement position -- a `comptime` block used for its side effects alone is
a clean `E0110` today, not silent no-op behavior. Taught in
[Chapter 4](learn/book/04-functions.md).

## Concurrency

### Actor
A `Name()`-constructed concurrent unit with private, zero-initialized
state and message-handler methods -- one of Kryos's two concurrency
primitives alongside `spawn` + channels. Never "goroutine" or "thread": an
actor is a higher-level construct that may or may not map 1:1 to an OS
thread depending on backend. Constructed with `Name()` (no struct-literal
form -- state is private). Taught in
[Chapter 13](learn/book/13-concurrency.md).

### `spawn`
Starts a concurrent unit of work from a closure or function. A closure
captured by `spawn` is SHARED (not snapshotted) across every `spawn`
referencing it; calls into a shared mutating closure are serialized under
a per-closure lock, so concurrent calls converge correctly (at the cost of
throughput -- a hot shared counter is faster through `std::sync::atomic_int()`).
Taught in [Chapter 13](learn/book/13-concurrency.md).

### Channel
A `chan`-typed message-passing primitive between concurrent units
(`send`/`recv`). A blocking `recv` on a closed, drained channel returns
`0`, indistinguishable from a real `send(ch, 0)` -- use `chan_try_recv`/
`chan_is_closed` to tell the two apart. Taught in
[Chapter 13](learn/book/13-concurrency.md).

## Errors and diagnostics

### Diagnostic code
An `E````` (error) or `W````` (warning) identifier attached to a compiler
message, documented in [`docs/error-codes.md`](error-codes.md) and
explainable in long form via `kryos explain <CODE>`. Not every code that
exists in the source is currently emitted, and not every emitted code is
currently registered -- see that page's "Known gaps" section before
assuming the two always match.

### `Result<T, E>`
The recoverable-error return type, from `std::result` (`Ok(x)` / `Err(e)`).
Must be explicitly annotated on a function signature -- a bare `Result`
(no type arguments) erases the payload to `i64`. Taught in
[Chapter 12](learn/book/12-error-handling.md).

### `throw`
Raises an unrecoverable exception that unwinds to the nearest `try`/
`catch`; the thrown value is stringified at the throw site, so a `catch`
binding is always `str`. An uncaught throw prints
`kryos: uncaught exception: <msg>` and exits 101. Distinct from a runtime
PANIC (div-by-zero, index-out-of-bounds, a missing-file `file_read`),
which `try`/`catch` cannot intercept at all. Taught in
[Chapter 12](learn/book/12-error-handling.md).

### Advisory diagnostic
A diagnostic that reports a real condition but does not block compilation
-- the program still compiles and runs with the flagged behavior intact.
`W0300`, `W0400`, `W0500`, and (per its own documented contract) `E0300`
are all advisory in this sense; a warning-level code (`W`````) is always
advisory by construction, but `E0300` is the one place an `E`-coded
diagnostic is deliberately non-blocking.

## Module system

### `use std::<module>::{name}`
Kryos's import syntax -- a flat function namespace with **no import
aliasing** (`use m::{parse as p}` is a parse error). Two modules exporting
the same name cannot both be imported; the module resolver additionally
pulls in every struct an imported module defines regardless of the
selective `{}` list, so two modules that both define a same-named struct
collide even with disjoint function imports. Taught in
[Chapter 15](learn/book/15-modules-and-packages.md).

### Qualified call (`Mod::name(...)`)
Sugar for the flat name `name` -- valid only when `name` was actually
imported FROM `Mod` via a `use` statement; it is not an alternate way to
reach a symbol that was never imported (`E0201`/`E0202` cover the two ways
this can go wrong).

## Terms with a Kryos-specific gotcha (see `CLAUDE.md` for the full story)

### `elif`
The keyword for an else-if branch. `else if` (two tokens) also parses, but
this repo's own compiler source uses `elif` by convention. Taught in
[Chapter 5](learn/book/05-control-flow.md).

### W0001 continuation trap
The parser has no line-number awareness, so a fresh line starting with
`||`, `-`, `[`, or `(` can silently merge into the previous line's
expression instead of starting a new statement -- see
[`W0001`](error-codes.md#w0001----ambiguous-newline-led-continuation) for
the mechanism and the fix. Taught in
[Chapter 3](learn/book/03-bindings.md) and
[Chapter 5](learn/book/05-control-flow.md).

### Const (does not exist)
Not a Kryos keyword. A module-level constant is written as a top-level
`let NAME: TYPE = value` -- `const NAME: TYPE = value` fails to parse
(`E0001`). See [`docs/19-language-reference.md`](19-language-reference.md)
§11.2.
