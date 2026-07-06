# Capability enforcement: current state and roadmap

Kryos's defining feature is a compile-time capability system: a function's
authority over the outside world (network, filesystem, other processes, the
environment, the database, the terminal) is part of its type, checked and
propagated through the call graph. This page states honestly where enforcement
is **on by default**, where it is **opt-in**, and the plan for closing that gap.

## What is enforced today

- **The capability model is real and precise.** Each builtin maps to the exact
  authority it needs (`file_read` → `fs:read`, `http_get` → `net:http`,
  `tcp_connect` → `net:tcp`, `exec` → `process`, ...). Coarse capabilities
  grant their sub-capabilities; the reverse is rejected. Declaring less than
  you use is a compile error (`E0505`); a caller that does not declare what a
  callee needs is a compile error (`E0507`).
- **Self-scoped operations are ambient, by design.** `exit`/`abort` (terminate
  your own process) and clock reads / `sleep` are *not* gated — they cannot
  read your data, reach another system, or take unauthorized action. Gating
  them would be noise, not safety. (An agent that stalls or exits is bounded by
  `@budget`, a separate mechanism.) This mirrors how Rust's `process::exit` and
  WASI's `proc_exit` treat self-termination.
- **Under `--strict-capabilities`, enforcement is complete and load-bearing.**
  Every public example and showcase program (81/81) carries least-privilege
  `@capabilities` declarations and passes strict checking in CI. Stripping a
  program's annotations produces violations — the checker genuinely enforces,
  it is not decorative.

## Deny-by-default is here for new projects

There are now three enforcement modes (`permissive` < `inferred` < `strict`),
selectable with `--capabilities-mode` or `[capabilities] mode` in `kryos.toml`.
**`kryos new` scaffolds `mode = "inferred"`, so a fresh project is
deny-by-default from the first line of code** — both `kryos check` and
`kryos build` reject an unannotated `main` that (directly or transitively) uses
a gated builtin.

`inferred` mode is what makes deny-by-default *ergonomic*: you declare authority
once at the boundary (`main`, or a `pub` API function), and the compiler infers
every interior helper's capability set from its call graph. A ~15k-line program
needs one annotation on `main`, not one per function (the self-host compiler
itself is inferred-clean with a single `@capabilities(fs:read, fs:write,
process)` on `main`). Enforcement is sound across every path authority can take:
direct builtin calls, method/static dispatch, and gated builtins passed as
first-class values.

The `strict` mode (`--strict-capabilities`) remains for maximum scrutiny — every
function auditable in isolation. 81/81 examples pass it in CI.

## The global default is now `inferred`

As of v1.0.0-beta.3 the **bare-compiler default** is `inferred`: a loose
`kryos run foo.kry` / `kryos check` / `kryos build` with no flag and no
`kryos.toml` is deny-by-default. A program that reads a file, hits the network,
or reads the environment from an unannotated `main` is rejected at compile time.
`--capabilities-mode=permissive` (on `check` and `build`) or
`[capabilities] mode = "permissive"` opts out.

Staged plan, done in order:

1. **Examples exemplary** — done. 81/81 strict; 106/106 `inferred`.
2. **New projects deny-by-default** — done. `kryos new` → `mode = "inferred"`.
3. **Self-host + stdlib capability-clean** — done. The self-host compiler is
   inferred-clean with a single boundary annotation on `main`; the stdlib
   authority wrappers are annotated so their capabilities propagate.
4. **Flip the global default to `inferred`** — done (beta.3).

### Nothing is checked permissively anymore

The `ecosystem/` packages were the last surface checked under `permissive`.
Every package entry point now declares its capabilities and the ecosystem gate
runs under the compiler default (inferred): 253/253 clean, with a handful of
intentional negative fixtures (`leaky_io`, `leak_config`, ...) excluded by
design because they exist to demonstrate a leak. **The compiler, `kryos new`
projects, the examples, the self-host compiler, and the ecosystem packages are
all deny-by-default.** The only code checked permissively is the internal
test-harness fixtures that verify compilation/runtime rather than capability
hygiene, and illustrative doc snippets.

## Migrating a program

```bash
kryos check --capabilities-mode=inferred src/main.kry   # deny-by-default
kryos check --strict-capabilities src/main.kry          # every fn must declare
```

Each error names the builtin and the capability it needs. In `inferred` mode you
usually only annotate `main` — the error names the exact (transitive) set, e.g.
`call to `do_install` requires capabilities [fs:read, fs:write]`. Add
`@capabilities(fs:read, fs:write)` to `main` and re-check; interior helpers need
nothing. In `strict` mode, annotate each function that calls a gated builtin.
See [docs/10-capabilities.md](10-capabilities.md) for the full model.
