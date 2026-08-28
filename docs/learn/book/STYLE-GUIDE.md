# Kryos style guide

This is idiomatic-Kryos guidance for people WRITING Kryos code -- naming,
formatting, capability annotations, error-handling idioms, and stdlib
patterns. It is a different document from
[`STYLE.md`](STYLE.md), which is the voice/structure bible for people
WRITING this book. If you're authoring a chapter, read `STYLE.md`. If
you're writing a `.kry` file (including a chapter's example code), read
this page.

Every example on this page was checked against the reference compiler
(`compiler/target/release/kryos.exe check --capabilities-mode=permissive`,
`KRYOS_STDLIB_DIR=$PWD/compiler/stdlib`) on 2026-08-28.

## Formatting: `kryos fmt` is the arbiter

Don't hand-format. Run `kryos fmt` and accept its output -- it enforces
4-space indentation, 80-column wrapping on function parameter lists, and
one blank line between top-level declarations, and it's idempotent
(running it twice produces no further diff). Style debates that would be
bikeshedding in another language are not a decision Kryos code makes by
hand.

**New since the W0001-extension wave: `kryos fmt` refuses to touch a file
that contains a live ambiguous newline-led continuation.** If a `.kry`
file has a fresh line starting with `||`, `-`, `[`, or `(` in the specific
shape that trips [`W0001`](../../error-codes.md#w0001----ambiguous-newline-led-continuation),
`kryos fmt` prints:

```
skipped path/to/file.kry (contains an ambiguous newline-led continuation --
run `kryos check path/to/file.kry` to see the W0001 warning; file left
untouched)
```

and leaves the file byte-for-byte unchanged, instead of silently
re-emitting it in canonical formatting. This is deliberate: a plain
AST-based re-print would bake the parser's "always continue" reading into
clean, warning-free-looking source and destroy the one clue (the original
line break) that anything was ambiguous. Resolve the `W0001` first --
restructure the ambiguous line per its fix -- then run `kryos fmt` again.

## Naming

- **Functions, variables, module names, and struct fields:** `snake_case`.
- **Types** (`struct`, `enum`, `trait`): `PascalCase`.
- **Top-level constants** (a module-level `let`, since there is no
  `const` keyword -- see the [glossary](../../glossary.md#const-does-not-exist)):
  `SCREAMING_SNAKE_CASE`. This matches the stdlib's own convention
  (`compiler/stdlib/agent.kry`'s `ALIGNMENT_STRICT`, `STATE_RUNNING`, ...).

```kryos
let MAX_RETRIES: i64 = 3

struct UserAccount {
    display_name: str,
    login_count: i64,
}

fn format_greeting(display_name: str) -> str {
    return "Hello, " + display_name
}

fn main() {
    let account: UserAccount = UserAccount { display_name: "Ada", login_count: 1 }
    println(format_greeting(account.display_name))
    println(to_string(MAX_RETRIES))
}
```

Output:

```
Hello, Ada
3
```

## Capability annotations

- **Annotate the entry point, not every helper.** Under the default
  `inferred` mode, an unannotated `main` that transitively calls a gated
  builtin is rejected with the exact capability set named in the error;
  interior helper functions don't need their own `@capabilities(...)` --
  the checker infers their requirement from what they call and folds it
  into whichever annotated boundary calls them.
- **Declare the narrowest sub-capability that covers what you actually
  call**, not the coarse family. `@capabilities(fs:write)` if you only
  ever call `file_write`; reach for coarse `io`/`fs` only when you
  genuinely need both `fs:read` and `fs:write` on the same function. See
  the sub-capability table in `CLAUDE.md`'s "Builtins available
  everywhere" section for which builtin needs which sub-cap.
- **Only reach for `--strict-capabilities` in CI/audit contexts**, where
  you want every function -- not just `pub` boundaries -- to carry its own
  explicit grant. Day-to-day development uses the default `inferred`
  mode; forcing every helper to declare capabilities it already gets from
  its caller is noise, not safety, at that mode.
- **A misspelled `deny!()` name is silent** (`W0500`) -- it protects
  nothing and still compiles. Treat that warning as a hard error in any
  code where the `deny!` block is load-bearing for a real trust boundary.

```kryos
@capabilities(fs:write)
fn main() {
    file_write("out.txt", "hi")
}
```

## `Result<T, E>` vs. `throw`

Use **`Result<T, E>`** for a failure the caller is expected to handle as
part of normal control flow -- a parse that might fail, a lookup that
might miss, a validation that might reject. Always annotate the full
`Result<T, E>` (and `Option<T>`) on the function signature; a bare
`Result`/`Option` with no type arguments erases the payload to `i64`.

```kryos
use std::result::{Result, Ok, Err}

fn parse_port(s: str) -> Result<i64, str> {
    let n: i64 = parse_int(s)
    if n < 1 {
        return Err("port must be positive")
    }
    if n > 65535 {
        return Err("port out of range")
    }
    return Ok(n)
}

fn main() {
    match parse_port("8080") {
        Ok(p) => println("port: " + to_string(p)),
        Err(e) => println("invalid: " + e),
    }
}
```

Output:

```
port: 8080
```

Use **`throw`** for a precondition violation the caller isn't expected to
route around -- a missing required config file at startup, an invariant
that should never break in correct usage. A `throw` unwinds to the
nearest `try`/`catch` and stringifies its payload at the throw site;
nothing catches it, `kryos: uncaught exception: <msg>` prints to stderr
and the process exits 101. Remember `try`/`catch` only intercepts
`throw` -- a runtime PANIC (index out of bounds, `file_read` on a missing
file used without a prior `file_exists` check, div-by-zero) is process-
fatal and cannot be caught.

```kryos
fn require_config(path: str) -> str {
    if file_exists(path) == 0 {
        throw "missing required config file: " + path
    }
    return file_read(path)
}

@capabilities(fs:read)
fn main() {
    try {
        let cfg: str = require_config("nonexistent.toml")
        println(cfg)
    } catch e {
        println("startup failed: " + e)
    }
}
```

Output:

```
startup failed: missing required config file: nonexistent.toml
```

A rule of thumb: if you'd write a `match` on the result at the call site
in normal, non-exceptional code, it's a `Result`. If reaching that code
path at all means something is already wrong, it's a `throw`.

## Struct vs. tuple enum variant

Kryos enums only have **tuple variants** (`Variant(T)`, `Variant(T, U)`,
...) -- there is no struct-style variant (`Variant { field: T }`, `E0110`
if you try). For a variant that needs more than one or two positional
values, prefer a small named struct as the payload over an unlabeled
multi-field tuple variant once field meaning stops being obvious from
position alone.

```kryos
enum Shape {
    Circle(f64),
    Rectangle(f64, f64),
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.14159 * r * r,
        Shape::Rectangle(w, h) => w * h,
    }
}

fn main() {
    let c: Shape = Shape::Circle(2.0)
    println(to_string(area(c)))
}
```

Output:

```
12.56636
```

`Rectangle(f64, f64)` is fine here because `width, height` is an obvious,
conventional order. If a variant grows a third or fourth positional field,
or the order isn't obvious from the type alone, switch to a struct payload
(`Rectangle(RectDims)` with `struct RectDims { width: f64, height: f64 }`)
rather than asking a reader to remember argument position.

## Stdlib patterns

- **Building a string in a loop:** never `s = s + chunk` -- that's O(n²)
  (a full string clone per iteration). Use
  `std::string::string_builder()` and call `.append(...)` per chunk,
  `.build()` once at the end.

  ```kryos
  use std::string::{string_builder}

  fn join_words(words: [str]) -> str {
      let sb = string_builder()
      let mut i = 0
      while i < len(words) {
          if i > 0 {
              sb.append(" ")
          }
          sb.append(words[i])
          i = i + 1
      }
      return sb.build()
  }

  fn main() {
      let words: [str] = ["fast", "and", "correct"]
      println(join_words(words))
  }
  ```

  Output:

  ```
  fast and correct
  ```

- **`push(arr, v)` always gets reassigned.** `push` grows the shared
  buffer in place and returns the array handle -- always write
  `arr = push(arr, v)`, never read a pre-push alias afterward. This is
  the same pattern for every collection built on `push`
  (`std::heap`/`queue`/`stack`/`deque`).

  ```kryos
  fn main() {
      let mut nums: [i64] = []
      nums = push(nums, 1)
      nums = push(nums, 2)
      nums = push(nums, 3)
      for n in nums {
          println(to_string(n))
      }
  }
  ```

- **Don't fabricate a stdlib function name.** If you're not sure a
  function exists, check `docs/19-language-reference.md`'s stdlib list or
  run `kryos doc` against the module, or write a thin wrapper around a
  documented builtin instead of guessing a plausible name -- `E0204`
  ("module has no export by that name") exists specifically because this
  is a common mistake, including for AI-assisted code generation.
- **Import only the names you use, and watch for a same-named collision.**
  Kryos's flat namespace means `use std::csv::{parse}` and
  `use std::json::{parse}` in the same file is a compile error (`E0205`),
  and importing a name that shadows a global builtin another imported
  module relies on internally can break that module. Prefer selective
  imports over a glob import (`use std::os::*`) in real code, even though
  glob imports are legal, so a future colliding addition to either module
  fails at the specific `use` line instead of silently at a distant call
  site.

## Next

- This repo's root `CLAUDE.md` gotcha list covers runtime pitfalls (the
  ones that compile clean and misbehave) rather than style -- read it
  alongside this page.
- [`docs/error-codes.md`](../../error-codes.md) for the full diagnostic
  reference this guide's examples were checked against.
