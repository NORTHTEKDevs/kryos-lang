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

## The honest gap

By **default** (`kryos check` / `kryos build` without `--strict-capabilities`),
an *unannotated* function is unconstrained: the compiler infers and reports its
capabilities but does not *reject* it for failing to declare them. Enforcement
is therefore **opt-in** today. That is the single most important thing to know
about the current security posture, and it is why this page exists rather than
a claim that "Kryos is capability-safe" full stop.

**Use `--strict-capabilities` whenever it matters** — code that handles
untrusted input, embeds an agent, or runs third-party plugins. In that mode the
guarantee is real. The [`kryos-embed`](../ecosystem/kryos-embed/) SDK and the
governed-agent demos run under it.

## Roadmap to deny-by-default

Making strict checking the **standard** mode is a breaking change: every
existing unannotated program would need capability declarations. It is being
done deliberately, in order, rather than flipped in a point release:

1. **Examples exemplary** — done. 81/81 pass strict mode with least-privilege
   declarations, gated in CI.
2. **Standard library clean** — in progress (63/66 modules pass; the remaining
   three are internal fs/crypto/net wrappers pending correct propagation
   annotations).
3. **Ecosystem clean** — the `ecosystem/` packages annotated and gated.
4. **Flip the default in a signposted release** (targeted for a future beta),
   with `--no-strict-capabilities` as an opt-out for gradual migration and a
   one-command migration helper (`kryos check` already *reports* the exact
   capabilities a function needs, so annotating is mechanical).

Until step 4 lands, treat capability safety as **opt-in and reliable** rather
than **default and reliable**. Both are honest; only the second is the end
state, and it is not claimed before it is true.

## Migrating a program to strict mode

```bash
kryos check --strict-capabilities src/main.kry
```

Each error names the builtin and the capability it needs. Add an
`@capabilities(...)` annotation to the function (least privilege — declare the
precise sub-capabilities the errors name, not a coarse `all`), and re-check.
Because capabilities propagate, annotate helpers that call gated builtins and
declare the union on their callers. See
[docs/10-capabilities.md](10-capabilities.md) for the full model.
