# 11 · Capabilities

After this chapter you will be able to read any Kryos program's
`@capabilities(...)` declarations and know exactly what it can touch on the
system -- files, the network, the environment, a subprocess -- without
reading a single line of its implementation. You will be able to write a
least-privilege program yourself, sandbox an untrusted code path inside a
trusted one with `deny!`, and know precisely where this security model's
proven guarantees end.

This is Kryos's headline feature -- the reason the language exists rather
than being "a systems language with ARC." Everything else in this book is a
better version of something another language already has. Capabilities are
not.

## Why capabilities

In most languages, any function can open a file, make a network request, or
spawn a process. A logging library that phones home. A template engine that
reads `/etc/passwd`. A math utility that shells out. You find out at runtime,
in production, usually from an incident.

Kryos inverts this. Every function's authority to touch the outside world is
part of its type signature, checked at compile time. If a function's code
transitively calls something gated -- `file_read`, `http_post`, `env_get` --
and the authority to do so was never declared, the program does not compile.
Not a warning: a build failure, with the exact missing capability named.

## Declaring capabilities

`@capabilities(...)` is an attribute on a function. Here is a complete,
working tool: it reads a notes file and prints it.

```kryos
@capabilities(fs:read)
fn load_notes(path: str) -> str {
    return file_read(path)
}

@capabilities(fs:read)
fn main() {
    let notes: str = load_notes("notes.txt")
    println(notes)
}
```

With a `notes.txt` containing `Buy milk` / `Call Sam` next to it, running
this prints:

```
Buy milk
Call Sam
```

`load_notes` needs `fs:read` because it calls `file_read`, a gated builtin.
`main` needs it too, because it calls `load_notes`. You only annotate the
functions the compiler tells you to -- interior helpers can stay unannotated
and their requirements are inferred (see the next section) -- but `main`'s
declaration is the one that matters most: it is the program's complete,
auditable authority list.

Pure computation needs nothing. `println`, string manipulation, arithmetic,
and building data structures require no capability at all -- only I/O,
network, process, and similarly dangerous operations are gated.

## Three enforcement modes

`kryos check`/`run`/`build` accept `--capabilities-mode=<mode>`, or you set
`[capabilities] mode` once in `kryos.toml`. There are three:

| Mode | Behavior |
|---|---|
| `inferred` (**the default**) | Deny-by-default at the boundary. `main` (and any other annotated function) must hold every capability its code transitively uses; unannotated interior helpers have their requirements inferred and bubbled up. `kryos new` scaffolds this. |
| `strict` (`--strict-capabilities`) | Every function is checked against exactly its own declaration -- no inference, no free pass for unannotated helpers. Maximum scrutiny, for security-critical libraries. |
| `permissive` (`--capabilities-mode=permissive`) | Only annotated functions are checked at all. An unannotated function is unconstrained. For scratch files and legacy code, not for anything real. |

Watch the same program behave differently under two of these modes. This
function reads a file but declares nothing:

```kryos
fn main() {
    let notes: str = file_read("notes.txt")   // ERROR: fs:read not declared
    println(notes)
}
```

Under the default (`inferred`):

```
error[E0505]: builtin `file_read` requires `fs:read` capability
 --> main.kry:2:22
  2 |     let notes: str = file_read("notes.txt")
    |                      ^^^^^^^^^^^^^^^^^^^^^^ requires `fs:read`
  = note: add `@capabilities(fs:read)` to the enclosing function or actor
```

Under `--capabilities-mode=permissive`, the exact same source compiles
clean -- `main` is unannotated, so in permissive mode it is unconstrained.
This is exactly why permissive mode is an opt-in escape hatch, not something
you leave on: it is the difference between "this program's authority is
fully documented" and "capabilities are decoration."

## The capability catalog

The full type list (`net`, `net:http`, `net:tcp`, `fs`/`io`, `fs:read`,
`fs:write`, `process`, `env`, `ffi`, `compute`, `crypto`, `term`, `db`,
`time`, `all`) and the exhaustive builtin-to-capability table live in
[`docs/10-capabilities.md`](../../10-capabilities.md). The ones you will
reach for constantly:

| Capability | Grants |
|---|---|
| `fs:read` | `file_read`, reading via `std::fs` |
| `fs:write` | `file_write`, `create_dir`, `remove_file` |
| `net:http` / `net:tcp` | HTTP(S) clients/servers / raw TCP, TLS, unix sockets |
| `process` | `env_get`/`env_set`, spawning subprocesses |
| `crypto` | `sha256`, `hmac`, `random_bytes` |
| `all` | Everything -- reserved for a trusted entry point; always visible in review |

## The sub-capability lattice

`fs:read` and `fs:write` are independent -- neither implies the other. This
function declares only `fs:read` but tries to write:

```kryos
@capabilities(fs:read)
fn save_backup(path: str, data: str) {
    file_write(path, data)   // ERROR: needs fs:write, not fs:read
}

@capabilities(fs:read)
fn main() {
    save_backup("backup.txt", "backup data")
}
```

```
error[E0505]: builtin `file_write` requires `fs:write` capability
 --> main.kry:3:5
  3 |     file_write(path, data)
    |     ^^^^^^^^^^^^^^^^^^^^^^ requires `fs:write`
  = note: add `@capabilities(fs:write)` to the enclosing function or actor
```

The rule runs one direction only: the coarse capability grants every
sub-capability underneath it (`@capabilities(io)` covers both `fs:read` and
`fs:write`; `@capabilities(net)` covers both `net:http` and `net:tcp`), but a
sub-capability never grants its sibling or its coarse parent. Declare the
narrowest one that covers what the function actually does -- that is the
entire point of having sub-capabilities instead of one blunt `fs`/`net` flag.

## Least privilege by example: a tool that overreaches

Here is the shape the task brief for this chapter asked for directly: a
small tool that legitimately needs `fs:read`, and a bug (or a malicious
dependency) that tries to also phone home over the network.

```kryos
use std::net::{http_post}

@capabilities(fs:read)
fn load_notes(path: str) -> str {
    return file_read(path)
}

fn sync_to_cloud(data: str) {
    http_post("https://example.com/notes", data, "text/plain")
}

@capabilities(fs:read)
fn main() {
    let notes: str = load_notes("notes.txt")
    sync_to_cloud(notes)   // ERROR: main only holds fs:read
    println(notes)
}
```

```
error[E0507]: call to `sync_to_cloud` requires capabilities [net] not granted to caller
 --> main.kry:15:5
  15 |     sync_to_cloud(notes)
     |     ^^^^^^^^^^^^^^^^^^^^ callee requires more capabilities
  = note: function `sync_to_cloud` has @capabilities(net) but caller lacks [net]
```

`sync_to_cloud` is never annotated -- under `inferred` mode its requirement
is computed from what it actually calls, and that requirement bubbles up to
`main`. `main` only declared `fs:read`, so the whole program is rejected at
compile time. This is the point: **the notes tool cannot exfiltrate the
notes even by accident**, because nobody with review access to `main`'s
declaration ever granted it network authority. (One unrelated papercut you
will hit writing this yourself: `http_post` is not a global builtin -- it
lives in `std::net` and needs `use std::net::{http_post}`, same as
`http_get`. The compiler's own error for the unimported name suggests
`http2_post` instead, which is a different function -- read the `did you
mean` note carefully before following it.)

## `deny!`: sandboxing inside a program that already has the authority

A single `@capabilities` declaration on `main` is coarse: it is the
program's *ceiling*, not a guarantee that every code path underneath uses
the minimum it needs at every point. `deny!(...)` narrows the capability set
for a lexical block, even inside a function that itself holds more. This is
how you sandbox a plugin, a third-party callback, or any code path you want
to trust less than the rest of the program -- without pulling it out into a
separately-privileged process.

Compare two versions of a plugin host that legitimately has `net` for other
reasons (say, fetching its own config) and also runs untrusted plugin logic.
Without `deny!`, the plugin call compiles clean, because the host's own
declaration covers it:

```kryos
use std::net::{http_post}

@capabilities(fs:read, net)
fn run_plugin(config_path: str) -> str {
    let config: str = file_read(config_path)
    return call_plugin(config)
}

fn call_plugin(config: str) -> str {
    http_post("https://example.com/sync", config, "text/plain")
    return config
}

@capabilities(fs:read, net)
fn main() {
    println(run_plugin("notes.txt"))
}
```

This compiles and runs -- `run_plugin` and `main` both hold `net`, so
`call_plugin`'s network use is authorized. Now wrap only the plugin
invocation in `deny!(net)`:

```kryos
use std::net::{http_post}

@capabilities(fs:read, net)
fn run_plugin(config_path: str) -> str {
    let config: str = file_read(config_path)
    deny!(net) {
        return call_plugin(config)   // ERROR: net denied in this block
    }
}

fn call_plugin(config: str) -> str {
    http_post("https://example.com/sync", config, "text/plain")
    return config
}

@capabilities(fs:read, net)
fn main() {
    println(run_plugin("notes.txt"))
}
```

```
error[E0507]: call to `call_plugin` requires capabilities [net] not granted to caller
 --> main.kry:7:16
  7 |         return call_plugin(config)
    |                ^^^^^^^^^^^^^^^^^^^ callee requires more capabilities
  = note: function `call_plugin` has @capabilities(net) but caller lacks [net]
```

Same function, same outer declaration, same caller -- the only change is the
`deny!(net) { ... }` block around the one call you want to distrust. Inside
that block, `net` no longer exists as far as the checker is concerned, even
though `run_plugin` itself is declared with it. This is least privilege
applied *within* a function, not just between functions: grant broadly at
the boundary where you must, then narrow again around the specific span of
code you trust the least.

## Capabilities through closures and containers

A closure is a value, and Kryos tracks the authority a closure *carries*
independent of where it ends up -- a struct field, an array element, a map
value, passed through several layers of forwarding functions. This closes an
obvious laundering path: wrapping a privileged operation in a closure and
handing it to an innocuous-looking helper does not make the authority
disappear.

```kryos
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}

fn zero_cap_tool(reader: fn() -> str) -> str {
    return reader()
}

@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("secret.txt")
    deny!(fs:read) {
        println(zero_cap_tool(reader))   // ERROR: reader carries fs:read
    }
}
```

```
error[E0110]: capability `fs:read` is denied in this block, but the block uses it
 --> main.kry:13:5
  13 |     deny!(fs:read) {
     |     ^^^^^^^^^^^^^^^^ here
error[E0507]: call to `zero_cap_tool` requires capabilities [fs:read] not granted to caller
 --> main.kry:14:17
  14 |         println(zero_cap_tool(reader))
     |                 ^^^^^^^^^^^^^^^^^^^^^ callee requires more capabilities
  = note: call to `zero_cap_tool` requires [fs:read] not granted to caller -- some
    of this authority is carried by a closure/fn-value ARGUMENT passed at this
    call site, not by `zero_cap_tool`'s own declaration
```

`zero_cap_tool` itself is correctly unannotated -- it is as generic as
`std::iter::map` -- but the checker still resolves what the specific closure
argument carries at this call site, and `deny!(fs:read)` still catches it.
The same tracing works when the closure is stashed in a struct field, an
array element, or a map value instead of a plain local (a plugin registry,
router table, or command-dispatch map, in other words). The full proof --
struct/array/map/nested variants, plus the precision cost this buys you
below -- lives in
[`docs/10-capabilities.md`](../../10-capabilities.md#closure-indirection-including-containers-is-sound-read-this-if-you-are-trusting-it-with-secrets).

## The honest boundary

Read this before you trust capabilities with a secret an adversary controls.

**The precision cost is real and larger than you would guess.**
[LEDGER item 41](../../../tools/loop/LEDGER.md) measured it directly: of 75
combinatorially-enumerated ways a *pure*, zero-authority closure can reach a
call site through a container, only 34 compile without complaint. The other
41 -- 55% -- are rejected and need `@capabilities(all)` at the caller, even
though the closure carries no real authority at all. This is the deliberate
cost of the fail-closed design in the sections above: when the checker
cannot statically prove a closure's provenance, it charges the caller the
maximum rather than guessing at the minimum. Sound, but inconvenient --
expect to hit this if you build a large plugin registry or router table.

**This is a development-time discipline, not a sandbox for adversarial
code.** From [`docs/capability-soundness.md`](../../capability-soundness.md):

> Zero known escapes is not zero escapes. The corpus is finite and
> human-authored; the shape nobody has thought of is exactly the one that
> matters, and no amount of green re-running will find it. Capabilities
> remain a strong DEVELOPMENT-TIME discipline, not a boundary to run
> untrusted code behind.

Use capabilities to make a codebase's authority legible, to catch an
accidental overreach at compile time (exactly the notes-tool example
above), and to review what a dependency can touch before you `kryos pkg
add` it. Do not use them as your only defense against code you do not trust
at all -- run genuinely adversarial code in a real OS-level sandbox or a
separate process, the same way you would for any other language's
"security" annotations. [`docs/capability-roadmap.md`](../../capability-roadmap.md)
tracks the longer-term design (capability-typed function values) that would
close the remaining precision gap without weakening the fail-closed default.

## Common mistakes

**Forgetting the capability on `main`.** Under the default `inferred` mode,
an unannotated `main` that transitively touches anything gated is rejected
with the exact missing capability named (`E0505`) -- add
`@capabilities(...)` to `main`, not to the builtin call site.

**`std::net` functions need an import.** `http_get`/`http_post` are not
global builtins the way `file_read` is -- they live in `std::net` and need
`use std::net::{http_post}`. Calling an unimported name gets you
`E0102: undefined variable`, and the compiler's own "did you mean
`http2_post`" suggestion is a *different, lower-level* function -- read
past the suggestion before applying it.

**Declaring the coarse capability out of habit.** `@capabilities(all)` or a
coarse `fs`/`net` compiles, but it is the broad escape hatch, not the
default -- and it is exactly as visible in code review as it sounds. Declare
the narrowest sub-capability that covers what the function does; reach for
coarse only at a genuinely trusted entry point.

## Exercises

1. Take the notes tool from the top of this chapter and add a second helper
   that calls `env_get`. Run `kryos check` and read the error -- which
   capability does it ask for, and why is `env_get` gated at all (see
   `docs/10-capabilities.md`'s `process` entry)?
2. Take the `deny!` example and move the `deny!(net)` block so it wraps
   `run_plugin`'s file read instead of the plugin call. Does it still
   compile? Why does denying `fs:read` there matter but denying `net`
   there would not have caught the original leak?
3. Try `--capabilities-mode=strict` against the notes tool from the top of
   this chapter. It should still pass -- explain why, given that
   `load_notes` is explicitly annotated already.

## Summary

- `@capabilities(...)` declares a function's authority; violating it is a
  compile error, not a runtime surprise.
- Three modes: `inferred` (default, deny-by-default with interior
  inference), `strict` (every function checked against its own
  declaration), `permissive` (only annotated functions checked -- scratch
  code only).
- Sub-capabilities (`fs:read`/`fs:write`, `net:http`/`net:tcp`) are
  independent; the coarse capability grants both, never the reverse.
- `deny!(...)` narrows authority for a lexical block, even inside a
  function that holds more -- the tool for sandboxing one code path inside
  an otherwise-trusted program.
- Capability tracking follows closures through containers (struct fields,
  arrays, maps), closing the obvious laundering path.
- The fail-closed design costs precision: 55% of enumerated pure-closure
  shapes through a container need `@capabilities(all)` (LEDGER item 41).
  Capabilities are a strong development-time discipline, not a boundary to
  run genuinely untrusted code behind.

Next: [Error handling](12-error-handling.md)
