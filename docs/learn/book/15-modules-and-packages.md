# 15 · Modules and packages

After this chapter you will be able to split a program across multiple
files, import from the standard library and from other projects on your
machine, understand the one rule that governs every `use` statement in
Kryos (one flat namespace, no aliasing), and read/write a `kryos.toml`
manifest with the confidence to know what each section actually enforces.

## The `use` statement and module resolution

A Kryos file is a module. `use path::{name1, name2}` brings specific items
from another module into scope; `path` uses `::` as its separator. You have
already been writing this every time you wrote `use std::result::{Ok, Err}`
or similar -- `std` is just the module namespace the standard library lives
under.

Given a project with a sibling file, `use` resolves it by filename relative
to `src/`:

```
myproject/
    kryos.toml
    src/
        main.kry
        greet.kry
```

<!-- docs-example: skip -->
```kryos
// src/greet.kry
pub fn greet(name: str) -> str {
    return "Hello, " + name + "!"
}
```

<!-- docs-example: skip -->
```kryos
// src/main.kry
use greet::{greet}

fn main() {
    println(greet("world"))
}
```

Output (this is a two-file example -- the checker that gates this book only
verifies single, self-contained blocks, so both files above are marked
skip; the layout was built and run for real against
`compiler/target/release/kryos.exe` and this is its actual output):

```
Hello, world!
```

`use greet::{greet}` resolves `greet` to `src/greet.kry`, then imports the
`pub fn greet` it declares. A directory can be its own module too:
`use ml::transformers` looks for `src/ml/transformers.kry` first, falling
back to `src/ml/mod.kry` if `ml` is a directory module. Only items marked
`pub` are importable from outside the file that declares them.

## The rule that governs everything else: one flat namespace, no aliasing

Every name you import -- regardless of which module it came from -- lands
in a single flat namespace for the importing file. There is no import
aliasing:

<!-- docs-example: skip -->
```kryos
use std::csv::{parse as p}   // ERROR
```

```
error[E0009]: unexpected token 'as', expected ','
 --> greet.kry:1:22
  1 | use std::csv::{parse as p}
   |                      ^^ here
```

This is a parse error, not a lint -- there is no way to rename an import on
the way in. The direct consequence: **two modules that each export a
same-named item cannot both be imported into the same file.**
`std::csv::parse` and `std::json::parse` both exist; importing both fails,
because `parse` can only mean one thing at a time in this file's namespace:

<!-- docs-example: skip -->
```kryos
use std::csv::{parse}
use std::json::{parse}   // ERROR
```

```
error[E0205]: duplicate function `parse` imported from multiple modules
  = note: a function named `parse` was already imported; Kryos has no import
    aliasing -- import disjoint names selectively so only one is in scope
```

Two modules whose selected names genuinely do not overlap import together
without any conflict -- `use std::csv::{parse}` alongside
`use std::json::{stringify}` compiles clean, since nothing collides.
Resolve a real collision by module-qualifying the call
(`json::parse(...)`) where the language allows it, or restructure so both
modules' items are not needed unqualified in the same file.

**The same collision can happen through a struct you never named.** The
resolver pulls in every struct an imported module defines, not just the
names you selected -- so two modules that each define a struct with the
same name collide even when your `use {...}` lists are disjoint. This is
exactly why `std::chan`'s wait-group and once-cell types are named
`ChanWaitGroup`/`ChanOnce` instead of the shorter `WaitGroup`/`Once` that
`std::sync` already uses for its own types -- so importing both `std::chan`
and `std::sync` in the same file works.

## `kryos.toml`

Every project has a `kryos.toml` manifest at its root. `kryos pkg init`
generates one:

```bash
kryos pkg init
```

```toml
[package]
name = "myproject"
version = "0.1.0"
edition = "2026"

[dependencies]

[capabilities]
allowed = []

[build]
target = "native"
optimization = "dev"
```

`[package]` is identity metadata. `[dependencies]` lists other packages
your project needs -- see below. `[capabilities] allowed = [...]` is the
project-wide capability ceiling, checked the same way `@capabilities(...)`
is checked on `main` (Chapter 11) -- listing `fs:read` here without your
code declaring it on `main` does not grant anything by itself; the two
checks are independent layers, and both need to agree for a gated call to
succeed. `[build]` sets the default target and optimization level that
`kryos build` uses when you do not pass `--release`/`--target` explicitly.

## Local path dependencies

The simplest way to share code between two projects on your own machine is
a path dependency. Given two projects side by side:

```
projects/
    myapp/
        kryos.toml
        src/main.kry
    mylib/
        kryos.toml
        src/mylib.kry
```

A library's entry point file is named after the package itself --
`mylib`'s package is named `mylib` in its `kryos.toml`, so its importable
entry point is `src/mylib.kry`, not `src/lib.kry`. (This matters: Kryos
verifies that the file it resolved a `use` to has the *exact-case* on-disk
name the import requested, specifically so a project that works on a
case-insensitive filesystem (Windows, macOS) does not silently break on a
case-sensitive one (Linux, CI) -- naming the entry file anything other than
the package name is exactly the mismatch that check exists to catch.)

<!-- docs-example: skip -->
```kryos
// mylib/src/mylib.kry
pub fn greet(name: str) -> str {
    return "Hello, " + name + "!"
}
```

From `myapp/`, add and install the dependency:

```bash
kryos pkg add ../mylib
kryos pkg install
```

`pkg add` writes the dependency into `kryos.toml`:

```toml
[dependencies.mylib]
path = "../mylib"
```

`pkg install` resolves it, writes `kryos.lock`, and drops a small redirect
file at `myapp/.kryos/deps/mylib.redirect` recording the path. The compiler
walks up from whatever file it is compiling looking for
`.kryos/deps/<pkg>.redirect`, and uses it to resolve `use mylib::{...}` to
`<dep_root>/src/mylib.kry`. From `myapp/src/main.kry`:

<!-- docs-example: skip -->
```kryos
use mylib::{greet}

fn main() {
    println(greet("world"))
}
```

```bash
kryos run
```

Output (again a multi-file, multi-project example -- both blocks above are
marked skip for the same reason as this chapter's first example; this was
run for real, `kryos pkg add`/`kryos pkg install`/`kryos run` in sequence,
against the two-project layout on disk):

```
Hello, world!
```

## The registry: `kryos pkg add`/`install`, checksums, and lockfiles

For a published (not local-path) dependency, `kryos pkg add <name>` writes
a version constraint into `kryos.toml`, and `kryos pkg install` fetches it.
Under the hood, `install` `git clone --depth 1`s the registry's current
default-branch HEAD and copies out `packages/<name>/<version>/` -- it does
not download a tarball. Every install -- including a cache hit -- computes a
canonical `sha256:<hex>` checksum over the fetched `kryos.toml` plus every
`src/**.kry` (and `stdlib/**.kry`) file, in deterministic sorted-path
order, and compares it against the checksum recorded for that exact version
in the registry index. A mismatch, or a missing checksum, is rejected and
the offending cache entry deleted -- a package that gets mutated on disk
after a previous install, or a compromised/force-pushed registry that
silently changes an already-published version, cannot be silently reused.
`kryos.lock` records the verified checksum once a package passes.

**`kryos.lock` pins.** If it already exists and covers every dependency in
`kryos.toml`, `install` fetches exactly what the lock says and does not
touch the registry index at all -- the same contract `npm ci`/
`cargo install --locked` give you. Adding a brand-new dependency to
`kryos.toml` that the lock does not cover yet still triggers a fresh
resolve for the whole graph, but any *already-locked* package that drifts
as a side effect of that resolve is reported with an explicit
`warning: ... drifted from the committed kryos.lock` rather than silently
overwritten. `kryos pkg update` is the deliberate, explicit re-resolve --
reach for it instead of expecting `install` to upgrade anything on its own.

**`pkg add name --category c` / `--category rust`** declares a foreign (C
or Rust) dependency instead of a Kryos one. Foreign dependencies are not
auto-installed -- `kryos pkg install` prints the command you need to run
with the appropriate external tool, and calling into them from your code
needs the `ffi` capability (Chapter 19 covers FFI in depth).

## `kryos pkg`: the full command surface

| Command | What it does |
|---|---|
| `kryos pkg init` | Create a new project (`kryos.toml`, `src/main.kry`). |
| `kryos pkg add <name-or-path>` | Add a dependency -- registry name, local `path`, or `--category c`/`rust` for foreign deps. |
| `kryos pkg remove <name>` | Remove a dependency. |
| `kryos pkg install` | Resolve and fetch every dependency, honoring `kryos.lock` when it already covers the graph. |
| `kryos pkg update` | Explicitly re-resolve to the latest compatible versions. |
| `kryos pkg lock` | Regenerate the lockfile from `kryos.toml` without changing dependency selection logic. |
| `kryos pkg publish` | Package and publish the current project to the registry. |
| `kryos pkg show` / `info` | Show a package's capability badge / registry info. |
| `kryos pkg audit` | Diff capability escalation between the two latest registry versions of a package -- run this before upgrading a dependency you don't fully trust. |
| `kryos pkg search` | Search the registry. |
| `kryos pkg outdated` | List locked packages that have a newer compatible version available. |

## Semver constraints

| Constraint | Meaning |
|---|---|
| `"1.2.3"` | Exact version only |
| `"^1.0"` | Same major, `>= 1.0` (the default recommendation) |
| `"~1.2"` | Same major.minor, `>= 1.2` |
| `">=1.0"`, `"<2.0"`, etc. | Ordinary comparison |
| `"*"` | Any version |

When more than one available version satisfies a constraint, Kryos picks
the highest matching one.

## Common mistakes

**Trying to alias an import.** `use m::{name as other}` is a parse error,
full stop -- there is no renaming on the way in. Import the plain name, or
avoid the collision another way (see below).

**Not noticing a struct-name collision when your `use {...}` lists look
disjoint.** The resolver pulls in every struct from an imported module
regardless of your selective list. If two modules you both need happen to
define a same-named struct, the fix is picking a module pair that avoids
it (as `std::chan`/`std::sync` do), not renaming -- renaming isn't
available.

**Naming a path-dependency's entry file `lib.kry` out of habit from other
languages.** Kryos resolves `use mylib::{...}` to `src/mylib.kry` -- named
after the package, not a fixed `lib.kry` convention. A mismatched case (or
name) either fails outright or, on Windows/macOS, resolves anyway and then
fails later on a case-sensitive CI machine.

**Expecting `kryos pkg install` to upgrade a locked dependency.** It won't,
by design, once `kryos.lock` covers the graph -- that's what makes it safe
to run in CI. Use `kryos pkg update` when you actually want newer versions.

## Exercises

1. Split a two-function program (one function calling the other) into two
   files under `src/`, with the second function `pub` and imported via
   `use`. Confirm `kryos run` still produces the same output as the
   single-file version.
2. Deliberately import `std::csv::{parse}` and `std::json::{parse}` in the
   same file and read the real `E0205` error. Fix it by importing only the
   one you need and calling the other through its full module path where
   the language permits, or by splitting the two uses into separate files.
3. Create a local path dependency exactly as in this chapter's worked
   example, but name the library's entry file `src/lib.kry` instead of
   `src/mylib.kry`. Run `kryos run` from the consuming project and read
   the case-mismatch error.

## Summary

- `use path::{name}` resolves a sibling file (`src/<name>.kry`) or a
  directory module (`src/<name>/mod.kry`); only `pub` items are
  importable.
- Every import lands in one flat namespace for the file -- there is no
  import aliasing (`as` after an import name is a parse error), so two
  modules exporting the same name cannot both be imported, and this
  extends to same-named structs pulled in implicitly.
- `kryos.toml` has `[package]`, `[dependencies]`, `[capabilities] allowed`
  (the project-wide capability ceiling, independent of but layered with
  `@capabilities` on `main`), and `[build]`.
- A local path dependency (`kryos pkg add ../other-project`) resolves
  through a `.kryos/deps/<pkg>.redirect` file to
  `<dep_root>/src/<pkg-name>.kry` -- the entry file must be named after the
  package, not a fixed `lib.kry` convention, and the compiler enforces
  exact-case resolution so a project that works on Windows/macOS does not
  silently break on Linux CI.
- A registry dependency is checksum-verified on every install (including
  cache hits), and `kryos.lock`, once it covers the full dependency graph,
  pins exactly what gets fetched -- `kryos pkg update` is the explicit,
  deliberate way to move it forward.

Next: [The standard library tour](16-stdlib-tour.md)
