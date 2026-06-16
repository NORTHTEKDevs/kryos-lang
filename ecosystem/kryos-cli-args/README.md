# kryos-cli-args

A declarative command-line argument parser for Kryos. You describe your CLI as
data -- flags, positionals, subcommands -- and the library handles parsing,
typed extraction, and `--help` generation.

The distinctive part is not the builder. Every language has one. The point is
that the parsing core is **pure `compute`**: it reads no environment and touches
no filesystem. Because reading an environment variable (`env_get`) requires the
`process` capability under Kryos's capability model, a subcommand handler that
reads env vars is *forced* by the compiler to declare `process`, while one that
only parses and prints stays at `[]`. Run `kryos manifest --caps` on your CLI
and you get a per-subcommand "what can this touch" map as compiler output, not
as documentation that rots.

## MVP scope

- Builder API: boolean flags, value flags, positional args, subcommands.
- `--help` generation from the same spec the parser uses (cannot drift).
- Typed extraction: `get_str`, `get_int`, `get_bool`, `positional_at`, all with
  spec-driven defaults.
- The `args()` builtin is the only input; the parsing core is pure `compute`.
- A demo CLI (`demo_greet.kry`) with two subcommands of differing capability
  surfaces.

## Out of scope (deferred)

- Shell completion generation.
- Env-var fallback layering (a flag defaulting to an env var).
- Config-file merge (that is project 13's job).

These are intentionally left out so the parsing core stays in `compute`. The
moment the library itself reads env vars or files, it would no longer be the
clean capability witness that is the whole point.

## Layout

```
kryos.toml          package manifest, [capabilities] allowed = ["process"]
src/spec.kry        the builder data model (FlagSpec, ArgSpec, Command, CliSpec)
src/parse.kry       parse_command: the pure parsing core over a [str] argv slice
src/extract.kry     get_str / get_int / get_bool / positional_at with defaults
src/help.kry        --help text generation from the spec
demo_greet.kry      two-subcommand demo CLI (say = compute, whoami = process)
tests/test_parse.kry  parsing + extraction assertions (6 @test functions)
tests/test_help.kry   help-generation + spec-lookup assertions (3 @test functions)
```

## Building a CLI

```kryos
use spec
use parse
use extract

fn build_say() -> Command {
    let mut c = command("say", "print a greeting")
    c = command_flag(c, flag_bool("shout", "upper-case the greeting"))
    c = command_flag(c, flag_value("name", "world", "who to greet"))
    c = command_positional(c, positional("message", true, "the greeting body"))
    return c
}
```

`parse_command(cmd, argv)` takes the argv slice *after* the program name and
subcommand token and returns a `ParseResult`:

- `res.ok` / `res.error` -- success flag and a fixed error message.
- `res.flag_vals` -- `map<str,str>` of supplied flags (absent flags are not
  inserted; the getters fall back to the spec default).
- `res.positionals` -- the positional tokens, in order.

Then read values with the typed getters:

```kryos
let name = get_str(res, cmd, "name")       // "world" if --name was omitted
let loud = get_bool(res, cmd, "shout")     // true iff --shout was present
let msg  = positional_at(res, 0, "")       // first positional, or ""
```

## The capability map (the headline feature)

The demo's two subcommands deliberately differ:

- `say` -- pure parsing and string assembly. Its handler `cmd_say` is
  `@capabilities()` (empty: pure compute).
- `whoami` -- reads `$USER` via `env_get`, which the compiler maps to the
  `process` capability. Its handler `cmd_whoami` is `@capabilities(process)`.
  It cannot claim to be pure: `env_get` forces the annotation, and dropping it
  is a compile error under `--strict-capabilities`.

```
$ kryos manifest --caps --format pretty demo_greet.kry
fn build_cli: []
fn build_say: []
fn build_whoami: []
fn char_from: []
fn cmd_say: []
fn cmd_whoami: [process]
fn shout_text: []
unannotated: 1
```

`cmd_say` is `[]`; `cmd_whoami` is `[process]`. The audit question "which
subcommand can read my environment" is answered by the compiler.

## Running

This project is built and tested with the in-repo toolchain. From the repo root:

```
kryos test --path ecosystem/kryos-cli-args
kryos run ecosystem/kryos-cli-args/demo_greet.kry -- say hello --name Ada --shout
kryos run ecosystem/kryos-cli-args/demo_greet.kry -- whoami
kryos run ecosystem/kryos-cli-args/demo_greet.kry -- --help
kryos manifest --caps --format pretty ecosystem/kryos-cli-args/demo_greet.kry
```

`kryos run` needs the runtime staticlibs on the link path; set `KRYOS_RT_LIB`
to a directory containing `libkryos_rt.a` and `libkryos_stdlib_native.a` if they
are not discovered automatically.

## Notes

- All argument forms are long flags (`--name value`, `--switch`) plus bare
  positionals. Short flags (`-n`) and `--name=value` are not parsed in the MVP.
- Error messages are fixed strings (`unknown flag: --x`, `flag --x requires a
  value`, `missing required positional argument`) so callers and tests can
  match on them exactly.
- The library is licensed Apache-2.0 (see `LICENSE`).
