# 17 · Building and testing real programs

After this chapter you will be able to take a program past a single
`.kry` file: lay out a real project, write `@test` functions that exercise
your library code, declare the capabilities the project actually needs, and
run the same source through the fast development loop and the optimized
release build without changing anything about how you wrote it.

This chapter does not introduce new language syntax -- everything here is
the toolchain and project conventions that turn the language features from
Chapters 1-16 into a program someone else can build, test, and run.

## Project layout

`kryos pkg init` creates the shape every Kryos project follows:

```bash
kryos pkg init
```

```
initialized Kryos project `proj2`
  created kryos.toml
  created src/main.kry
  created .gitignore
  created README.md
```

```
proj2/
    kryos.toml
    .gitignore
    README.md
    src/
        main.kry
```

`src/main.kry` is the entry point; `kryos run`/`kryos build` with no
arguments, invoked from the project root, find it automatically -- you only
pass a filename explicitly for a scratch file outside a project. Add
sibling modules under `src/` and import them the way [Chapter
15](15-modules-and-packages.md) covered; add a `tests/` directory for
`@test` files, which is where `kryos test` looks by default.

## A real project, end to end

Here is a small but complete example: a word-counter tool with its logic
split into a library module, tested independently of the file I/O that
wraps it. This is a three-file project (`src/wordcount.kry`,
`src/main.kry`, `tests/wordcount_test.kry`) -- the checker that gates this
book's other chapters only verifies single, self-contained blocks, so all
three fences below are marked skip; the whole project was built and run for
real, with `notes.txt` containing `buy milk call sam finish the kryos book`
next to it, and every command's real output is pasted where shown.

<!-- docs-example: skip -->
```kryos
// src/wordcount.kry
pub fn count_words(text: str) -> i64 {
    let parts: [str] = split(trim(text), " ")
    return len(parts)
}
```

<!-- docs-example: skip -->
```kryos
// src/main.kry
use wordcount::{count_words}

@capabilities(fs:read)
fn main() {
    let text = file_read("notes.txt")
    println("words: " + to_string(count_words(text)))
}
```

<!-- docs-example: skip -->
```kryos
// tests/wordcount_test.kry
use wordcount::{count_words}

@test
fn test_count_words_basic() {
    assert_eq(count_words("the quick brown fox"), 4)
}

@test
fn test_count_words_single() {
    assert_eq(count_words("hello"), 1)
}
```

```bash
kryos test
```

```
running 2 @test functions

  PASS test_count_words_basic (0.1ms)
  PASS test_count_words_single (0.0ms)

Tests: 2 passed, 0 failed, 0 skipped, 2 total
Time:  0.003s
```

```bash
kryos run
```

```
words: 8
```

Notice `tests/wordcount_test.kry` imports `wordcount` the exact same way
`src/main.kry` does -- a test file is just another module under the
project, free to `use` anything under `src/`. This is the pattern worth
defaulting to: keep pure logic (`count_words`) in its own module with no
capability requirements, and put I/O (`file_read`) in `main` or a thin
wrapper around it. The pure module is trivially testable with no
capabilities annotation at all; only the I/O-touching entry point needs
`@capabilities(fs:read)`.

## `@test` and `kryos test`

A function marked `@test` takes no arguments, returns nothing, and signals
failure through `assert`/`assert_eq`/`panic` -- there is no separate
assertion framework to learn. `kryos test` (run from the project root)
discovers every `@test` fn under `tests/` (or a file/directory you pass
explicitly), compiles each, and runs it through the Cranelift JIT for fast
startup.

```bash
kryos test                       # run everything under tests/
kryos test wordcount             # run only tests whose name contains "wordcount"
kryos test test_count_words_basic --exact
kryos test --list                # list discovered tests without running them
kryos test --format json         # machine-readable output, for CI
```

A failing assertion reports which one and stops that test, but the run
continues on to the next `@test` function -- unlike a runtime **panic**
(division by zero, an out-of-bounds index), which is process-fatal by
design and aborts the whole `kryos test` run, leaving any tests after it
unexecuted. Prefer `assert`/`assert_eq` over triggering a real panic inside
a test body for exactly this reason: an assertion failure is a clean,
isolated red; a panic takes the rest of the suite down with it.

## Capabilities in a real project

[Chapter 11](11-capabilities.md) covered `@capabilities(...)` on individual
functions. A project layers one more check on top: `kryos.toml`'s
`[capabilities] allowed = [...]` is the project-wide ceiling, independent
of (and enforced alongside) whatever `main` itself declares. `kryos pkg
init` starts you with an empty ceiling:

```toml
[capabilities]
allowed = []
```

For most day-to-day development this does not get in your way -- the
per-function `@capabilities` check from Chapter 11 is what `kryos check`/
`run`/`build` actually enforce by default. The manifest-level list matters
most as a project-wide audit trail: `kryos audit` reports every function's
declared capabilities in one pass, which is the tool to reach for before
trusting (or shipping) a project you did not write every line of:

```bash
kryos audit
```

```
kryos audit
scanned 3 files
note: audit is a report, not a substitute for `kryos check`/`kryos build`.

== Parse failures (audit could NOT analyze these files) ==
  (none -- every scanned file lexed and parsed cleanly)

== Capability violations (kryos check would reject) ==
  (none -- every file passes the same inferred-mode capability check `kryos check` runs)

== Capability inventory (declared annotations only) ==
  fs:read: 1 function
    - main

== Extern blocks ==
  (no extern blocks)

== Secret patterns ==
  (none detected)
```

One line answers "what can this program touch": `fs:read`, on `main`,
nowhere else -- exactly the word-counter's real requirement, and nothing
more.

## Run, build, ship

The same project, three ways, exactly as [Chapter 1](01-hello.md)
introduced for a single file:

```bash
kryos run                    # Cranelift, compile + execute, fast loop
kryos build                  # Cranelift, writes a binary
kryos build --release        # LLVM, optimized, what you ship
```

```bash
kryos build --release
```

```
words: 8
```

(No visible build-step output on success; the binary is written silently.)
`kryos build --release` names the output binary after the package
(`proj2.exe` on Windows, `./proj2` on Unix) unless you override it with
`-o`. Running it directly, with no compiler in the loop at all, reproduces
the exact same output `kryos run` gave you during development -- the
capability check, the module resolution, and the program's logic are all
already baked into the binary; nothing about shipping the release build
changes the program's behavior versus what you tested against `kryos run`.

`kryos fmt` formats every `.kry` file in the project in place; `kryos fmt
--check` is the CI-safe dry run that exits non-zero if formatting would
change anything, without touching your files:

```bash
kryos fmt --check
```

A clean project prints nothing and exits `0`.

## Common mistakes

**Running `kryos test` and getting "no tests discovered".** `kryos test`
looks under `tests/` by default -- a `@test` fn sitting in `src/` next to
your regular code is not picked up unless you point at it explicitly
(`kryos test --path src`). Keep tests under `tests/`, and `use` your `src/`
modules from there, as this chapter's example does.

**Declaring `@capabilities` on `main` but forgetting the manifest ceiling
matters too.** For everyday work the per-function check (Chapter 11) is
what actually blocks a bad build; the `kryos.toml` list mostly matters when
you are reviewing (`kryos audit`) or intentionally locking a project down
further than the code itself already is.

**Testing a function that also does I/O.** A pure function like
`count_words` needs no capability and tests instantly under the JIT; a
function that also calls `file_read` needs the same `@capabilities`
declaration a test would have to satisfy too. Split I/O out into a thin
wrapper (as `main` does here) so the bulk of your logic stays trivially
testable.

## Exercises

1. Add a third `@test` function to `wordcount_test.kry` that checks an
   empty string. Run `kryos test` and see whether `count_words("")`
   behaves the way you expect -- if not, decide whether that is a bug in
   `count_words` or a wrong expectation in the test, and fix whichever one
   is actually wrong.
2. Add a second module, `src/greet.kry`, with a `pub fn` of your own, and
   a matching `tests/greet_test.kry`. Confirm `kryos test` picks up both
   test files in the same run.
3. Run `kryos audit` on a project where you deliberately give `main` a
   capability it does not use (e.g. add `@capabilities(fs:read, net)` to
   a `main` that never calls a networking function). Does the audit's
   capability inventory distinguish "declared" from "actually used"? What
   does that tell you about `kryos audit`'s value versus `kryos check`'s?
4. Run `kryos build --release` and then delete `src/` entirely before
   running the compiled binary directly. Confirm it still works -- explain
   why, in terms of what `kryos build` actually produced.

## Summary

- `kryos pkg init` scaffolds `kryos.toml` + `src/main.kry`; `kryos
  run`/`build` with no arguments, from the project root, find `main.kry`
  automatically.
- Keep pure logic in its own module (no capability needed, trivially
  testable) and I/O in a thin wrapper around it (`@capabilities` only
  where it is actually needed) -- this is the single biggest lever for
  making a project easy to test.
- `@test` functions take no arguments, return nothing, and use
  `assert`/`assert_eq`/`panic` to signal failure; `kryos test` discovers
  them under `tests/` by default and runs each through the Cranelift JIT.
  An assertion failure isolates to that test; a raw panic aborts the whole
  run.
- `kryos.toml`'s `[capabilities] allowed = [...]` is a project-wide
  ceiling layered on top of per-function `@capabilities` (Chapter 11);
  `kryos audit` is the tool for reviewing what a project actually declares
  before you trust or ship it.
- `kryos run`/`build`/`build --release` are the same source through three
  compilation modes -- the release binary's behavior matches what you
  already tested under `kryos run`, with no separate "does it still work
  when shipped" step needed.
- `kryos fmt --check` is the CI-safe formatting gate; a clean project
  prints nothing and exits `0`.

Next: [The backends: Cranelift/LLVM/wasm](18-backends.md)
