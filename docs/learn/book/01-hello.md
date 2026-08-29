# 01 · Hello Kryos & the Toolchain

After this chapter you will have Kryos installed, have written and run a real
program three different ways, and understand the one mental model that
explains every `kryos` subcommand you will use for the rest of this book:
**one source file, three backends, capabilities enforced on top of all of
them.**

## Install and verify

Install Kryos with the quick installer for your OS, or build from source. The
exhaustive per-OS matrix (package managers, prebuilt binaries, building
`llc`/`clang` for the LLVM backend) lives in
[`docs/01-getting-started.md`](../../01-getting-started.md) -- this chapter
just gets you to a working `kryos` command.

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.ps1 | iex

# From source
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release
```

Verify:

```bash
kryos --version
```

```
kryos 1.0.0
```

If you built from source, the binary is at `compiler/target/release/kryos`
(`kryos.exe` on Windows) -- put it on your `PATH`.

## Your first program

Create `hello.kry`:

```kryos
fn main() {
    println("Hello, Kryos!")
}
```

Run it:

```bash
kryos run hello.kry
```

Output:

```
Hello, Kryos!
```

Every Kryos program is a `.kry` file with a `fn main()` -- that is the entry
point, and `kryos run` is how you compile-and-execute in one step during
development. `println` needs no import; it is a global builtin, like
`to_string` and `len`.

## Four ways to turn source into a running program

Kryos gives you four commands over the exact same source file, because "run
this while I iterate" and "produce an optimized binary I can ship" are
different jobs with different tradeoffs. Try each of these against the
`hello.kry` above.

### `kryos run` -- fast loop, no binary

```bash
kryos run hello.kry
```

Compiles through the **Cranelift** backend and executes immediately (as a
subprocess, not an in-process JIT). No binary is written to disk. This is
what you reach for while writing code: Cranelift trades some runtime
optimization for fast compile times.

### `kryos build` -- a real binary, still Cranelift

```bash
kryos build hello.kry
./hello        # hello.exe on Windows
```

Same Cranelift backend as `run`, but this time it writes a native executable
to disk instead of running it immediately. Useful for a quick binary without
waiting on LLVM.

### `kryos build --release` -- optimized, LLVM

```bash
kryos build --release hello.kry
./hello
```

Switches to the **LLVM** backend: full optimization, real ahead-of-time
compilation, linked with your system's C linker (`cc`/`clang`/`link.exe`).
This is what you ship. It takes longer to compile than Cranelift and needs
`llc`/`clang` installed, but produces the fastest binary.

### `kryos build --backend wasm` -- WebAssembly

```bash
kryos build hello.kry --backend wasm -o hello.wasm
```

Compiles the *same source* to a `.wasm` module you can load in a browser or
run with `node tools/wasm-host/run.mjs` (WASI is not supported -- the wasm
target expects a JS host, not a standalone WASI runtime). Experimental
relative to the two native backends, but the language surface is identical:
nothing about `hello.kry` changed to target it.

### `kryos check` -- type-check only, no codegen

```bash
kryos check hello.kry
```

Runs the full type checker, ownership analysis, and capability checker
without generating any code. Exits `0` with no output when everything is
clean -- this is the fast feedback loop for "does this compile" while you are
editing, and it is the exact command CI runs against every code block in this
book (`tools/docs-examples/check.py`).

## The mental model: one source, three backends, capabilities on top

Most languages tie "what you can express" to "how it gets compiled." Kryos
deliberately does not: `hello.kry` is valid input to Cranelift, to LLVM, and
to the wasm backend without changing a single line. **Backend is a
compilation target, not a language dialect.** You pick Cranelift for
iteration speed, LLVM for shipped performance, wasm for the browser -- the
same way you would pick `-O0` vs `-O2` in a C compiler, not the way you would
pick between two different languages.

Sitting above all three backends is Kryos's capability system: every
function's `@capabilities(...)` annotation is checked at compile time,
*before* codegen runs, identically regardless of which backend you eventually
target. A program rejected for trying to read a file without declaring
`fs:read` is rejected the same way whether you are about to `run` it, `build`
it, or ship it to wasm. Chapter 11 is the deep dive; for now, the shape to
remember is: **backend decides how fast the code runs, capabilities decide
what the code is allowed to do, and neither one affects the other.**

## Common mistakes

**Semicolons.** Kryos statements end at the newline, not at `;`:

```kryos
fn main() {
    let x: i64 = 5;  // ERROR: Kryos has no semicolon statement terminator
    println(to_string(x))
}
```

```
error[E0009]: unexpected `;`
 --> mistake.kry:2:19
  2 |     let x: i64 = 5;
    |                   ^ here
  = note: Kryos does not use semicolons to terminate statements
```

Drop the `;`. If you need to break a long expression across lines, end the
line with an operator, `(`, `[`, `{`, or `,` -- never start the next line
with one of those (Chapter 20 covers the full list of what a leading token
can silently merge into the previous statement).

**`elif`, not `else if`.** Idiomatic Kryos -- and the self-hosted compiler's
own source -- writes `elif`:

```kryos
fn main() {
    let x: i64 = 5
    if x > 10 {
        println("big")
    } elif x > 0 {
        println("small")
    } else {
        println("zero or negative")
    }
}
```

Output:

```
small
```

`else if` also parses -- the grammar accepts both spellings -- but every
example in this book, the standard library, and the compiler's own source
uses `elif`. Write what the codebase you will be reading writes.

**Forgetting `fn main()`.** `kryos check` accepts a file with no `main` --
it is valid as a *library* module. `kryos run`/`build` need an entry point:

```
error: no `main` function found — binary programs require a main() entry point
```

If `kryos check` passes but `kryos run` fails this way, you wrote a module,
not a program -- add `fn main()`.

## Exercises

1. Change `hello.kry` to print your own name instead of "Kryos", then run it
   with `kryos run`.
2. Build it three ways -- `kryos build`, `kryos build --release`,
   `kryos build --backend wasm` -- and confirm all three outputs exist on
   disk.
3. Delete the closing `}` of `main` and run `kryos check`. Read the error.
   Put the brace back.
4. Add a semicolon to the end of the `println` line and run `kryos check`.
   Confirm you get `E0009`.

## Summary

- `.kry` source files; the entry point is `fn main()`.
- `kryos run` -- Cranelift, compile-and-execute, no binary written. Your
  edit loop.
- `kryos build` -- Cranelift, writes a binary.
- `kryos build --release` -- LLVM, optimized, what you ship.
- `kryos build --backend wasm` -- same source, WebAssembly output.
- `kryos check` -- type/ownership/capability checking with no codegen; the
  fastest feedback loop, and what this book's own examples are graded
  against.
- Backend and capabilities are independent axes: backend affects speed,
  capabilities affect what is allowed, and every backend enforces the same
  capability rules.

Next: [Values & types](02-values-and-types.md)
