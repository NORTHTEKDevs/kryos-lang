# Kryos completion loop — design

**Date:** 2026-08-11
**Status:** approved
**Goal:** drive Kryos to a professionally finished, honestly documented, benchmarked
state — and, critically, give that goal a **terminating condition** it has never had.

## Why this exists

Kryos works. It bootstraps itself 16/16, passes 62/62 conformance on two backends,
runs 91 examples under strict capabilities, self-hosts, and targets native + wasm.

What it has never had is a definition of done. The operating mode has been "hunt
adversarially until nothing is found," which is unbounded against an infinite search
space: every wave finds N defects, fixes them, and the next wave finds N more. Real
progress that never terminates reads as no progress at all.

The second problem is documentation drift, and it is not cosmetic. On 2026-08-10 all
three of these were true at once:

- the README claimed **one** documented capability residual; there were **twelve**;
- the LEDGER ranked item 10 as the highest-priority OPEN escape; it had been fixed;
- items 11(a) and 16 were fixed, committed to the working tree, and left uncommitted
  for days while the LEDGER described them as done.

Every one of those is a **status line that nobody re-ran**. That is the failure this
system is built to make structurally impossible.

## The invariant

> **A node is green only if its acceptance command exited 0 at the current HEAD.**

Nothing is ever marked done by assertion. When HEAD moves, green nodes become `stale`
and must re-verify. Status is *derived*, never typed. This single rule is the design.

## Components

```
tools/loop/GRAPH.md       node definitions: id, title, deps, acceptance command
tools/loop/graph-run.sh   status | next | verify <node> | verify-all
tools/loop/STATE.json     written ONLY by verify runs:
                          status, command, exit code, commit, timestamp
```

`GRAPH.md` is the source of truth for *what* must be true. `STATE.json` records only
*what was observed*, and only ever by a real execution. A human (or an agent) editing
`STATE.json` by hand is the one thing that breaks the system, so the runner rewrites it
wholesale on every verify.

## Node graph

| Node | Depends on | Acceptance |
| --- | --- | --- |
| `docs-truth` | — | README + LEDGER escape counts equal `escape_status.sh` output, compared by script |
| `escape-instrument` | — | Committed routing table: where each open escape actually reaches the checker |
| `escape-root` | `escape-instrument` | `escape_status.sh` reports 0 escaping, AND gates 2 + security_gate + ir_signatures green |
| `threat-model` | `escape-root` | `docs/THREAT-MODEL.md` exists, README links it, no unqualified "capability-safe" claim remains |
| `gates-full` | `escape-root` | `kryos-loop.sh gates 2` green AND `test_bootstrap.sh` 16/16 |
| `bench` | `gates-full`, `threat-model` | Benchmarks re-run; `BENCHMARKS.md` carries measured numbers + machine + commit |
| `release-ready` | all of the above | Every node green in one pass at one commit |

`escape-instrument` exists because skipping it cost two failed fixes on 2026-08-10 (a
`&`/`*` decomposer passthrough that did not close item 37, and a `TupleLiteral`
addition that did not close items 32/38). Both were reasoned, plausible, and wrong.
Instrument where the call actually routes *before* editing.

`bench` runs last so the published numbers describe the compiler that ships.

## Definition of done

**Every node green in a single uninterrupted `verify-all` at one commit.**

Not "no more bugs findable" — that is unbounded and unreachable. This is a state that
can actually be entered, observed, and defended.

## Failure policy: three strikes, then document

A node that fails its acceptance after **3 genuine attempts** converts to a
`documented-limitation` node whose acceptance becomes: the limitation is written into
`docs/THREAT-MODEL.md` and the README, and a repro test is committed.

This is what makes the loop terminate rather than grind. It is also the honest
professional outcome: a known, documented, reproducible limitation is a shippable
state; an undocumented one is not. Shipping with a stated threat model is what every
serious language does.

## What this deliberately does not do

- It does not try to make capabilities a sandbox against a hostile program author.
  That is a materially larger product and would be a deliberate decision, not a
  side effect of never declaring victory.
- It does not fan out subagents. Subagent self-reports need independent
  re-verification anyway, and the last six-wave workflow mis-attributed a real
  compiler regression to machine contention across every wave.
- It does not run headless overnight. Gate runs take ~25 minutes here and this
  machine is documented as unreliable under headless process storms.

## Known operating constraints (encoded in the runner)

- Never rebuild the compiler while a gate run is using the binary.
- `cargo` must run from `compiler/`, not the repo root; a wrong-directory build
  prints an error and still exits 0 through a pipe, which silently tests a stale
  binary.
- `ir_signatures` is the canary for capability false positives — it is what caught
  the pipe fix over-rejecting `5 |> padd(10)`.
- Never put backticks in a `git commit -m`; bash command substitution eats them.
  Use `-F <file>`.
