# 18 · The backends: Cranelift, LLVM, wasm

After this chapter you will be able to pick the right backend for the job you
are doing -- fast iteration, a shipped release binary, or a browser target --
explain why the same source produces different-speed code on each, read the
intermediate representations (`--emit-mir`, `--emit-llvm`) the compiler will
show you when something goes wrong, and know exactly which language features
you give up the moment you add `--backend wasm`.

## Recap: one source, three code generators

Chapter 1 introduced the shape: `.kry` source is valid input to all three
backends without changing a line, because backend is a compilation target,
not a language dialect. This chapter is the deep dive on what each backend
actually does with your code, and -- since "the same behavior on every
backend" is a design goal, not a law of nature -- where the three can
observably diverge.

Every program goes through the same front half of the pipeline regardless of
backend: lexer, parser, type checker, ownership analysis, capability checker,
then MIR (mid-level IR) lowering. MIR -- SSA-style basic blocks with explicit
terminators -- is the single point of truth both code generators consume; the
AST never reaches Cranelift or LLVM directly. Backend choice only affects
what happens *after* MIR. The full pipeline diagram and MIR's instruction set
are in [`docs/15-codegen.md`](../../15-codegen.md) -- this chapter summarizes
the parts that change your day-to-day workflow and links there for the
exhaustive version (every runtime function's signature, the uniform 8-byte
slot model every value is stored in, how each expression form lowers).

## Cranelift: `kryos run`, `kryos build` -- the debug backend

```bash
kryos run hello.kry              # compile + execute, no binary written
kryos build hello.kry            # compile, write a binary, don't run it
```

Cranelift is a fast, JIT-oriented code generator with no external tool
dependency -- it links entirely inside the `kryos` binary. It trades runtime
optimization for compile speed: this is what you want while you are actively
changing code, because the edit-compile-run loop is the thing you pay for
constantly, and a slower binary you run once during development costs
nothing. `kryos run` doesn't even write the executable to disk; it compiles
straight to memory and executes as a subprocess.

## LLVM: `kryos build --release` -- the release backend

```bash
kryos build hello.kry --release
```

Swaps in LLVM: the compiler emits LLVM IR, then shells out to `llc` (IR to
object code) and your system linker (`cc`/`clang`/`link.exe` -- object code
to executable) to produce the final binary. This is slower to compile because
LLVM runs real optimization passes, but it is what you ship -- the resulting
binary runs faster than Cranelift's output for the same source. `--release`
requires `llc` and `clang` on `PATH`; without them you get a named, specific
failure rather than a silent fallback:

```
llc not found -- install LLVM to compile with --release
clang not found -- install LLVM/Clang to link the binary
```

## wasm: `kryos build --backend wasm` -- the browser target

```bash
kryos build hello.kry --backend wasm -o hello.wasm
```

Targets `wasm32-unknown-unknown` under a **JS host contract**: the produced
module expects to run in a browser or under `node tools/wasm-host/run.mjs`,
not a standalone WASI runtime -- there is no WASI support. This is the one
backend where "same source, same behavior" is not unconditionally true, so
its contract gets its own section below.

## Worked example: one program, three backends, one answer

```kryos
struct Point {
    x: i64,
    y: i64,
}

fn main() {
    let p: Point = Point { x: 3, y: 4 }
    println(to_string(p.x + p.y))
}
```

Run through Cranelift:

```bash
kryos run point.kry
```

```
7
```

Build and run through LLVM:

```bash
kryos build point.kry --release -o point
./point
```

```
7
```

Build and run through wasm (via the bundled Node host):

```bash
kryos build point.kry --backend wasm -o point.wasm
node tools/wasm-host/run.mjs point.wasm
```

```
7
```

Same source file, zero changes, three separate code generators, one answer.
This is the guarantee backend independence is buying you: you develop against
Cranelift because it's fast to iterate on, and the release/wasm builds are
not a second implementation you need to separately trust -- they're the same
MIR run through a different lowering.

## Inspecting what the compiler is doing

Two flags dump intermediate representations instead of (or alongside)
compiling to a binary. Both work on the `add` function below:

```kryos
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

fn main() {
    println(to_string(add(3, 4)))
}
```

`--emit-mir` prints the backend-agnostic MIR for every function -- useful for
seeing how the compiler lowered your code *before* either backend touches it:

```bash
kryos build add.kry --emit-mir
```

```
fn add(a: i64, b: i64) -> i64 {
    ...
    bb0:
        _2 = add _0, _1
        return _2
}
```

`--emit-llvm` (only meaningful with `--release`) prints the LLVM IR text
after codegen:

```bash
kryos build add.kry --release --emit-llvm
```

```llvm
define internal i64 @add(i64 %_0, i64 %_1) {
bb0:
  %_2 = add i64 %_0, %_1
  ret i64 %_2
}
```

Reach for `--emit-mir` when you're debugging a miscompile and want to know
whether the bug is in lowering (shared by both backends) or in one specific
backend's codegen -- if the MIR looks right but the compiled behavior is
wrong on only one backend, the bug is backend-specific. `docs/15-codegen.md`
has the full key for reading LLVM IR mnemonics (`alloca`, `getelementptr`,
`icmp`, ...) if you're not already familiar with them from elsewhere.

## Cross-compilation: same LLVM backend, a different target triple

`--target=<triple>` cross-compiles through the LLVM backend to a target other
than your host. Cranelift is host-only -- there is no `--target` for the
debug backend, because Cranelift generates code for the machine running the
compiler and nothing else.

```bash
kryos build --target=help          # list every recognized triple
kryos build src/main.kry --release --target=x86_64-unknown-linux-musl -o app
```

```
Known target triples:

  x86_64-unknown-linux-gnu             Linux on x86_64 (glibc)
  x86_64-unknown-linux-musl            Linux on x86_64 (musl, fully static)
  aarch64-unknown-linux-gnu            Linux on ARM64 (glibc)
  aarch64-unknown-linux-musl           Linux on ARM64 (musl, fully static)
  x86_64-pc-windows-gnu                Windows on x86_64 (MinGW)
  x86_64-pc-windows-msvc               Windows on x86_64 (MSVC)
  aarch64-pc-windows-msvc              Windows on ARM64 (MSVC)
  x86_64-apple-darwin                  macOS on x86_64
  aarch64-apple-darwin                 macOS on ARM64 (Apple Silicon)
  wasm32-unknown-unknown               WebAssembly (JS host: browser or node tools/wasm-host/run.mjs; WASI not supported)

You may also pass an arbitrary LLVM triple; it will be forwarded as-is.
```

Each triple needs the matching linker/sysroot installed on your host (a musl
triple needs `musl-gcc`, a MinGW triple needs `x86_64-w64-mingw32-gcc`, and so
on) -- `docs/18-cross-compilation.md` has the full per-triple linker table
and the arbitrary-triple escape hatch for embedded targets like
`riscv64gc-unknown-linux-gnu`. One correction to that page as you read it:
its WASM callout still describes the backend as excluding structs, enums,
maps, closures, and `to_string()` -- that was accurate when the page was
written and is stale now (see the next section); trust the "Verified working"
table below over that page's WASM paragraph specifically, everything else on
that page is current.

## The wasm backend's actual contract, verified

`docs/wasm-contract.md` is the living, re-verified source of truth for what
compiles to wasm -- read it directly before shipping something to the browser
target, since this is the backend most likely to have moved since any given
snapshot of this chapter. As of the verification this chapter was written
against, the differential gate (`tests/wasm_differential_gate.sh`) reports
**65/65** probe programs compiling on wasm with output identical to both
native backends -- structs, maps, closures, `match`, and `to_string()` all
included, closing what an earlier design doc (and `docs/18-cross-compilation.md`
above) described as hard gaps.

What's still refused -- always **at compile time**, never a silent
miscompile:

```kryos
fn main() {
    let x: f64 = 3.7
    println(to_string(round(x)))
}
```

```bash
kryos build round.kry --backend wasm -o round.wasm
```

```
error: codegen (wasm) failed: wasm codegen: WASM backend v0.1 does not yet
support: call to `round` - supported builtins: println, len, str_concat,
array_new/get/set, to_string. Use --backend cranelift or --backend llvm for
full feature coverage.
error: compilation failed with 1 error
```

The same source runs fine on Cranelift (`kryos run round.kry` prints `4`) --
`round` just isn't implemented for wasm yet. A handful of other specific
builtins are in the same state: the global (unimported) `split`, `to_lower`,
`char_from`/`chr`, and array **index-assignment** (`arr[i] = v` -- reading
`arr[i]` and `push`ing both work, only the write form doesn't). `wasm-contract.md`
tracks the exact, current list -- it moves as gaps close, so check it rather
than memorizing this chapter's snapshot.

**The refusal is enforced, not just documented.** `emit_module` runs every
byte of a compiled wasm module through `wasmparser`'s own validator before
writing it to disk, and refuses to write a structurally invalid module. This
exists because the guarantee was once violated for real: a probe with
irreducible control flow compiled with exit code 0 and produced a `.wasm`
file that could not even be instantiated in a browser -- a build reporting
success while shipping a broken artifact, worse than a clear refusal. That
specific bug is fixed (the dispatch-relooper now emits a valid placeholder
for an unreachable epilogue), but the validator stays as a permanent backstop
regardless of what future bug might otherwise slip past it.

**A structurally valid module is not the same guarantee as a semantically
correct one**, and `wasm-contract.md` is explicit about a real miscompile
this exact distinction let through: `str == str` on wasm used to compile
clean, validate clean, and run -- but compared the packed `(offset, len)`
*handle* rather than the string's actual bytes, so a heap-built string (from
`+`, `substr`, a function return) never equalled an equal-content literal
even though nothing about the module looked wrong. Fixed by routing string
equality through a dedicated host import that compares real bytes. The
lesson generalizes: if you're auditing the wasm backend for a new gap,
"it validates" is not "it's correct" -- diff the actual output against
`kryos run`/`kryos build --release` on the same source, which is exactly what
the differential gate automates.

## When to choose which

- **`kryos run` while you're writing code.** Fast compile, no binary
  clutter, no LLVM dependency. This is your entire edit loop until the
  feature works.
- **`kryos build --release` for anything you ship.** Slower to compile,
  faster to run, needs `llc`/`clang` on the host doing the building (not the
  host running the binary, once it's built and cross-compiled).
- **`kryos check` constantly, both of the above rarely.** It runs the full
  front half of the pipeline -- type checking, ownership, capabilities --
  with no codegen at all, so it's the fastest signal for "does this still
  compile" and doesn't care which backend you'll eventually target.
- **`--backend wasm` once you're specifically targeting a browser or a JS
  host**, after confirming your program doesn't touch this chapter's short
  refused-builtins list. Not a general-purpose substitute for Cranelift or
  LLVM -- it's a third target with its own, narrower, growing feature
  surface.
- **`--target=<triple>` only with `--release`.** Cranelift can't cross
  compile; if you pass `--target` without `--release`, the compiler tells you
  so and does not silently fall back to the host.

## Common mistakes

**`--target` without `--release`.** Cranelift is host-only, so a bare
`kryos build` with `--target` set produces a clear warning, then fails at the
link step because there's no local toolchain for the foreign target:

```bash
kryos build hello.kry --target=x86_64-unknown-linux-gnu -o hello
```

```
warning: --target=x86_64-unknown-linux-gnu requires --release (LLVM backend). Cranelift only produces code for the host. Add --release to cross-compile.
error: linking failed: linker not found: could not find a linker for target 'x86_64-unknown-linux-gnu'; searched for: cc, gcc, clang
error: compilation failed with 1 error
```

Add `--release` and install the target's linker (`docs/18-cross-compilation.md`'s
table names the exact package per triple).

**A misspelled or unrecognized target triple.** The compiler warns instead
of silently accepting garbage, then forwards it to LLVM anyway (some triples
LLVM knows that this compiler's known-good list doesn't) -- if it really
doesn't exist, LLVM's own error follows immediately after:

```bash
kryos build hello.kry --release --target=totally-bogus-triple -o hello
```

```
warning: target `totally-bogus-triple` is not in the known-good list. Run `kryos build --target=help` to see supported triples. Forwarding to LLVM.
error: codegen (llvm) failed: clang compilation failed:
error: unknown target triple 'totally-bogus-triple'
```

**Assuming `kryos run` failing means your program is broken.** Chapter 1's
gotcha applies here too: Cranelift supports a narrower codegen surface than
LLVM. If `kryos run` fails in codegen (not in `check` -- a `check` failure is
a real language error on both backends) but `kryos build --release` succeeds,
the issue was backend-specific, not a bug in your program. Prefer `--release`
for anything Cranelift can't yet handle.

**Expecting `wasm-contract.md`'s gap list to be exhaustive or permanent.**
It's a living, re-verified document, not a fixed spec -- treat a "refused"
builtin as "not yet," check the doc's current state before assuming a
feature is permanently out of reach, and don't build a mental model of the
wasm backend from a static memory of what it supported months ago (this
chapter's own gap list included -- re-check `wasm-contract.md`, not this
page, if you're deciding whether to ship something to wasm).

## Exercises

1. Take the `Point` example from this chapter's worked section and add a
   third field. Build and run it through all three backends -- confirm the
   sum still matches.
2. Run `kryos build add.kry --emit-mir` on a function with an `if`/`elif`/
   `else` chain. Find the basic blocks the branches lowered to.
3. Try building a program that calls `to_lower` (from `std::string`) with
   `--backend wasm`. Read the refusal, then check whether `wasm-contract.md`
   still lists it as unsupported by the time you try this.
4. Run `kryos build --target=help` and cross-compile `hello.kry` to a target
   other than your host with `--release`. If you don't have the target's
   linker installed, read the specific error it gives you instead of a
   generic failure.

## Summary

- All three backends share the same front-end pipeline (lex, parse, type
  check, ownership, capabilities, MIR lowering) -- only what happens after
  MIR differs.
- Cranelift (`kryos run`/`kryos build`) is the fast debug backend, host-only,
  no external tools. LLVM (`kryos build --release`) is the optimizing release
  backend, needs `llc`/`clang`, and is what you ship. wasm
  (`--backend wasm`) targets a JS host (browser or `node tools/wasm-host/run.mjs`),
  not WASI.
- `--emit-mir` dumps the shared MIR; `--emit-llvm` (release only) dumps LLVM
  IR text -- both are debugging tools for "what did the compiler actually do
  with my code."
- Cross-compilation (`--target=<triple>`) goes through LLVM only; Cranelift
  cannot target anything but the host.
- The wasm backend refuses unsupported constructs **at compile time**, with
  a wasmparser validation pass as a hard backstop against ever writing a
  structurally invalid module -- but "validates" is not the same guarantee as
  "computes the right answer," so trust the differential gate's output
  comparison, not just a clean build, when auditing it.
- `docs/wasm-contract.md` is the current, re-verified state of the wasm
  backend's feature surface -- check it directly rather than trusting any
  static snapshot, this chapter included.

Next: [FFI & unsafe](19-ffi-and-unsafe.md)
