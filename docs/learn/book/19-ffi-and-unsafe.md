# 19 · FFI & unsafe

After this chapter you will know exactly what Kryos's `extern` and `unsafe`
surfaces can and cannot do today -- which is a narrower, more honest claim
than "how FFI works," because a meaningful chunk of what the grammar accepts
here is deliberately rejected at compile time rather than left to fail
later. You will be able to read an `extern` block and predict whether it
compiles, use the raw-memory builtins safely inside the capability system
that gates them, and know precisely which one guarantee `unsafe { }` still
gives you and which it currently doesn't.

## The one-sentence status, up front

**Arbitrary C-library FFI is not implemented.** You can declare an `extern`
block naming any C function you want -- the parser and type checker accept
it -- but calling anything that isn't one of the compiler's own `kryos_*`
runtime symbols is rejected at `kryos check` time with `error[E0508]`, before
codegen or linking ever runs. This used to not be true: earlier versions let
these declarations compile clean and then fail unpredictably -- a linker
error, a codegen crash, or worst of all a silent no-op at runtime. The
current behavior trades "maybe it works" for "you find out immediately, at
the cheapest possible point." Keep that trade in mind for the rest of this
chapter -- most of what follows is about the `kryos_*` surface that *does*
work, because the arbitrary-C-library surface, once you've seen the one
error it always produces, doesn't need much more room.

## `extern` blocks and the `kryos_*` rule

```kryos
extern "C" {
    fn getpid() -> i32   // ERROR: E0508, not a kryos_* symbol
}
```

```bash
kryos check pid.kry
```

```
error[E0508]: extern function `getpid` is not a `kryos_*` runtime symbol -- arbitrary C-library FFI is not implemented by this compiler (declaring it is accepted, but calling it will not reliably link, marshal, or execute correctly; see docs/13-ffi.md, `kryos explain E0508`)
```

Any extern name that doesn't start with `kryos_` gets exactly this error --
`sin`, `cos`, `puts`, `getenv`, whatever you name. There is no capability,
flag, or `kryos.toml` setting that unlocks it; real third-party C-library
linking is not a feature this compiler has yet. If what you actually want is
`sin`/`cos`/`sqrt`/`pow`, skip `extern` entirely -- they're already ambient
Kryos builtins (see "Builtins available everywhere" in `CLAUDE.md`), no
import or capability needed.

The `kryos_*` prefix names the compiler's own runtime symbols -- the
functions the ordinary builtins and stdlib are themselves implemented on top
of. Declaring one of these is legal, but it doesn't mean you should
hand-write the declaration instead of calling the documented builtin. Two
things happen when you do:

**A `str`/array/map/struct-typed signature is rejected too**, even for a
real `kryos_*` name, because the real native symbol expects raw
pointer/length pairs, not a Kryos heap handle:

```kryos
extern "C" {
    fn kryos_env_get(key: str) -> str   // ERROR: E0508, str-typed hand-declare
}

fn main() {
    println("hi")
}
```

```
error[E0508]: extern function `kryos_env_get` hand-declares a runtime symbol with a str/array/map/struct-typed parameter or return -- this bypasses the runtime's internal marshalling and segfaults at runtime (call the safe builtin/stdlib wrapper instead; see `kryos explain E0508`)
```

Before this check existed, this exact declaration compiled clean and
segfaulted both backends at runtime -- the real `kryos_env_get` symbol takes
`(key_ptr: i64, key_len: i64, val_buf: i64, val_buf_len: i64) -> i64`, a raw
pointer/length ABI, not a Kryos `str` handle. `env_get("PATH")`, the
documented builtin, already does this marshalling correctly -- there is no
reason to hand-declare the extern at all.

**An i64/i32/f64/pointer-only signature that matches the real symbol's raw
ABI is accepted at check time** -- this is a legitimate, working path, used
throughout `compiler/stdlib/ffi.kry` for exactly this purpose:

```kryos
extern "C" {
    fn kryos_env_get(key: i64) -> i64
}

fn main() {
    println("hi")
}
```

This one passes `kryos check` clean. It compiles because the signature shape
matches what codegen can genuinely marshal -- but note that this
particular declaration's arity doesn't actually match the real
`kryos_env_get` symbol (four params, not one); it type-checks because the
compiler validates the *shape* (scalar-only), not that you got the exact
signature of a symbol you're hand-guessing. Getting the arity wrong here is
exactly the kind of mistake that used to surface as a runtime segfault
instead of a compile error -- the safer move, always, is calling the
documented builtin (`env_get(key)`) rather than hand-declaring the
runtime symbol underneath it.

## Calling an extern is capability-gated

Declaring an extern is free. Calling one costs the same capability its
`kryos_*` symbol backs (`E0506`) -- `kryos_env_get` needs `process`, the same
capability `env_get(...)` needs, because it's the same underlying authority:

```kryos
extern "C" {
    fn kryos_env_get(key: i64) -> i64
}

fn main() {
    let p: i64 = kryos_env_get(0)   // ERROR: E0506, process not declared
    println(to_string(p))
}
```

```
error[E0506]: extern function `kryos_env_get` requires `process` capability
 --> main.kry:6:18
  6 |     let p: i64 = kryos_env_get(0)
    |                  ^^^^^^^^^^^^^^^^ requires `process`
  = note: add `@capabilities(process)` to the enclosing function or actor
```

Add `@capabilities(process)` and it checks clean. A non-`kryos_*` extern
(the arbitrary-C-library shape, which fails E0508 regardless) would need
`ffi` instead -- Chapter 11's capability model applies identically here, no
new mechanism, just a new source of gated calls. A real project also
declares this in `kryos.toml`'s `[capabilities] allowed = [...]` ceiling
(Chapter 17) alongside the per-function `@capabilities` -- both layers apply.

**One correction to the current `docs/19-language-reference.md`, verified
directly against this compiler while writing this chapter: it states "All
`extern` functions are implicitly `unsafe` - calling them requires an
`unsafe` block."** That is not what the checker enforces. The
`kryos_env_get` example immediately above compiles and runs with no `unsafe`
block anywhere -- only the capability gate applies to an extern *call*.
`unsafe { }` is required for exactly one thing in this language today (next
section), and calling an extern function isn't it. Don't restructure working
code to wrap extern calls in `unsafe` expecting it to matter -- it's a no-op
as far as the checker is concerned.

## The raw-memory builtins

A small family of global builtins gives you direct byte-level access to
heap memory outside the ARC system: `alloc(size)`, `free_bytes(ptr, size)`,
`ptr_read_i64(ptr, i)`, `ptr_write_i64(ptr, i, v)`, `ptr_byte_at(ptr, i)`,
`ptr_set_byte(ptr, i, b)`, `str_to_ptr(s)`, `buf_to_str(ptr, len)`. Every one
of them operates on a plain `i64` -- there is no `*T` pointer type involved,
just an integer you agreed to treat as an address.

**These require the `ffi` capability, at every one of them** -- both the two
functions that can produce a pointer (`alloc`, `str_to_ptr`) and every
function that dereferences one:

```kryos
@capabilities(ffi)
fn main() {
    let buf: i64 = alloc(24)
    ptr_write_i64(buf, 0, 10)
    ptr_write_i64(buf, 1, 20)
    ptr_write_i64(buf, 2, 30)

    let total: i64 = ptr_read_i64(buf, 0) + ptr_read_i64(buf, 1) + ptr_read_i64(buf, 2)
    println("sum: " + to_string(total))

    free_bytes(buf, 24)
}
```

```
sum: 60
```

`ptr_read_i64`/`ptr_write_i64` are slot-indexed, not byte-indexed -- index
`1` means the 8-byte slot starting at byte offset 8, index `2` means byte
offset 16, and so on, which is why this program can pack three `i64`s into a
24-byte buffer and address each one by a small integer rather than
computing byte offsets by hand. Drop the `@capabilities(ffi)` and every one
of these five calls fails independently:

```
error[E0505]: builtin `alloc` requires `ffi` capability
error[E0505]: builtin `ptr_write_i64` requires `ffi` capability
error[E0505]: builtin `ptr_read_i64` requires `ffi` capability
error[E0505]: builtin `free_bytes` requires `ffi` capability
```

Because both pointer *sources* (`alloc`, `str_to_ptr`) require `ffi`, there
is no way to obtain a pointer to dereference without already declaring it --
the surface is closed at its entry point, not just at each individual read
or write.

**None of these calls require an `unsafe` block.** This is a deliberate,
documented design point (not an oversight): the raw-memory builtins are
treated as "runtime plumbing" -- the same primitives `std::bytes` and the
stdlib's own string/buffer helpers are built on -- and are gated purely by
the `ffi` capability rather than by `unsafe { }`. If you're auditing a
dependency for what it can touch, `ffi` in its capability list is your
signal to look here; a missing `unsafe { }` block around a call to `alloc`
is not evidence that the code is somehow safer.

## What `unsafe { }` actually gates

Kryos does have a real `unsafe { }` block, and it does have one real,
enforced job: gating the dereference of a genuine `*T` raw pointer.

```kryos
fn main() {
    let x = unsafe { 40 + 2 }
    println("val " + to_string(x))

    unsafe {
        println("in unsafe")
    }
}
```

```
val 42
in unsafe
```

`unsafe { }` is semantically transparent -- it works as both a value
expression and a statement, on both backends, and everything inside it type
checks exactly as it would outside. What changes is that a raw-pointer
dereference (`*p` where `p: *T`) is *permitted* inside it and rejected
outside it:

```kryos
extern { fn raw_ptr() -> *i64 }   // ERROR: E0508, not a kryos_* symbol

@capabilities(ffi)
fn main() {
    let p: *i64 = raw_ptr()
    let v = *p              // <- E0500 outside unsafe
    println(to_string(v))
}
```

```
error[E0500]: dereference of raw pointer requires an `unsafe` block
 --> main.kry:6:13
  6 |     let v = *p
    |             ^^ here
```

Wrapping the dereference in `unsafe { *p }` satisfies this specific check.

**Read the example above again, though: `raw_ptr` is declared in a non-`kryos_*`
extern block, which means this exact program *also* fails E0508 the moment
you run `kryos check` on it** -- verified directly, the compiler emits both
errors together, E0508 on the extern declaration and E0500 on the
dereference, in the same run. This is the honest edge of the feature, worth
stating plainly rather than glossing over: **as of this compiler, there is
no supported, working way to obtain a genuine `*T`-typed value from real
Kryos code.** The only textual source for one is an `extern` function
declared to return `*T`, and every `extern` function that isn't a `kryos_*`
runtime symbol is rejected at check time before you can ever call it. Every
`kryos_*` symbol that genuinely works today -- the raw-memory builtins
above, and the entire `std::ffi` dynamic-library surface
(`compiler/stdlib/ffi.kry`: `dlopen`/`dlsym`/`malloc`/`read_i64`/`write_i64`
and the rest) -- deliberately represents every pointer as a plain `i64`, not
a `*T`. So the `*T`/`unsafe`/E0500 machinery is real, implemented, and
enforced by the checker exactly as documented -- it is just currently
unreachable from any code path that also compiles, because its one
documented pointer source is closed by a different, newer rejection rule.
If you're looking for the working equivalent of "a raw pointer I can read
and write," that's the previous section's `i64`-based raw-memory builtins,
which need no `unsafe` block at all -- not `*T`.

## Real C FFI: what to actually reach for

Given all of the above, here is the practical summary for someone who
opened this chapter wanting to call a real C library:

- **You can't, not yet.** `[build] link = [...]` in `kryos.toml` (linking an
  additional C library) is documented in `docs/13-ffi.md` as an intended
  design, but it is explicitly **not implemented** -- setting it has no
  effect. There is no supported way to pull in libsodium, SQLite via a
  user-facing extern, or any other third-party C library from `kryos build`
  today.
- **`kryos bindgen <header.h>` works and generates real declarations** --
  but they're subject to the same E0508 rejection the moment you try to
  compile against them, since bindgen only generates the `extern` block, not
  actual linking support. Useful for previewing a header's shape, not yet
  useful for shipping code.
- **`std::ffi` (dynamic library loading via `dlopen`/`dlsym`) is the real,
  working escape hatch** if you need to call into a shared library at
  runtime -- it's built entirely on `kryos_ffi_*` symbols with `i64`-typed
  signatures, the pattern this chapter verified works. See
  `compiler/stdlib/ffi.kry` for the full surface (`open`, `sym`, `call0..6`,
  `cstr`, `read_i64`/`write_i64`, ...).
- **For math (`sin`, `cos`, `sqrt`, `pow`) and most system needs, you almost
  certainly don't need FFI at all** -- they're ambient builtins already.

`docs/13-ffi.md` is the full reference for every one of these, including the
complete type-marshalling table (which scalar types cross an extern
boundary and which don't -- `str` isn't in it on purpose, convert explicitly
with `str_to_ptr`/`buf_to_str`) and the FFI-specific safety considerations
(wrong C types crash the process, string lifetimes only last the call,
foreign allocations aren't tracked by Kryos's ARC). The unsafe-code audit of
the *runtime's own* internals (not user Kryos code) -- every `unsafe` block
inside `kryos-rt`, the codegens, and the native stdlib, catalogued by
pattern with its invariants -- is
[`docs/17-unsafe-audit.md`](../../17-unsafe-audit.md); useful if you're
evaluating how much to trust the compiler's own foundations, not something
you'll need for writing ordinary Kryos programs.

## Common mistakes

**Reaching for `extern` when the ambient builtin already exists.** `sin`,
`cos`, `sqrt`, `pow` are already global, capability-free builtins -- an
`extern "C" { fn sin(x: f64) -> f64 }` block is not just unnecessary, it's a
guaranteed E0508.

**Hand-declaring a `kryos_*` symbol instead of calling the builtin.** Even
when the signature happens to check (scalar-only types), you're guessing at
an ABI the documented builtin already gets right -- `env_get(key)` instead
of `extern { fn kryos_env_get(...) }`, `alloc(n)` instead of a hand-rolled
`kryos_alloc_bytes` declaration you'd have to get the exact real signature
of.

**Expecting `unsafe { }` around an extern call, a raw-memory builtin, or
`std::ffi` to change whether it compiles.** It doesn't gate any of those --
only a literal `*T` dereference. Adding it where it isn't needed is
harmless but doesn't do what the language reference's "extern calls are
implicitly unsafe" line suggests.

**Assuming the raw-memory builtins are an ungated escape hatch because they
don't need `unsafe { }`.** They need `ffi`, checked identically to any other
gated builtin -- verify a dependency's `kryos.toml` capability list the same
way you would for `fs:write` or `net:http` before trusting it.

## Exercises

1. Take the `raw_mem_ok` example from this chapter and grow the buffer to
   hold five `i64` values instead of three. Update the `alloc` size and the
   slot indices, and confirm the sum is still correct.
2. Try declaring `extern "C" { fn strlen(s: str) -> i64 }` and run
   `kryos check` on it. Which error do you get, and why -- is it the
   non-`kryos_*` rejection, the str-signature rejection, or both?
3. Look up `kryos_ffi_dlsym`'s signature in `compiler/stdlib/ffi.kry`. Why
   does it take a `str` for the symbol name (unlike `kryos_env_get`, which
   only accepts `i64`) without tripping the str-signature rejection this
   chapter covered? (Hint: re-read the small compiler-verified allowlist
   noted in `docs/13-ffi.md`'s status note.)

## Summary

- Arbitrary C-library FFI (any non-`kryos_*` extern name) is rejected at
  compile time (`E0508`), not left to fail at link or runtime -- this
  replaced a previous state where such declarations silently compiled and
  then misbehaved unpredictably.
- A hand-declared `kryos_*` extern is accepted only with a scalar
  (i64/i32/f64/pointer)-only signature; a `str`/array/map/struct-typed
  hand-declaration is rejected (`E0508`) because it bypasses the runtime's
  real marshalling and segfaults. Prefer the documented builtin over
  hand-declaring the symbol either way.
- Calling any extern is capability-gated (`E0506`) -- a `kryos_*` name needs
  the capability of the builtin it backs (`process` for `kryos_env_get`);
  a non-`kryos_*` name would need `ffi`, but never reaches that check
  because it's already rejected by E0508.
- The raw-memory builtins (`alloc`, `free_bytes`, `ptr_read_i64`,
  `ptr_write_i64`, `ptr_byte_at`, `ptr_set_byte`, `str_to_ptr`, `buf_to_str`)
  all require the `ffi` capability and use plain `i64` addresses -- none of
  them require an `unsafe` block.
- `unsafe { }` is real and enforced for exactly one thing: dereferencing a
  genuine `*T` raw pointer (`E0500` outside it). As verified in this
  chapter, there is currently no working, non-rejected way to obtain a `*T`
  value at all -- its only documented source, a non-`kryos_*` extern
  returning `*T`, is itself rejected by E0508 -- so this machinery, while
  real, is presently unreachable from code that also compiles.
- `docs/19-language-reference.md`'s claim that all extern calls require
  `unsafe { }` does not match this compiler's actual enforcement, verified
  directly while writing this chapter -- only the `*T` dereference does.
- Real C-library linking (`[build] link = [...]` in `kryos.toml`) is
  aspirational, not implemented; `std::ffi`'s `dlopen`/`dlsym`-based dynamic
  loading is the working alternative today.

Next: [Idioms & pitfalls](20-idioms-and-pitfalls.md)
