# Kryos Ecosystem Plan

> Generated 2026-06-14, grounded in the real Kryos primitives (verified against
> the compiler + stdlib source, not invented). Honesty ratings are deliberate:
> "truly-novel" = no mainstream language/runtime does this as a first-class
> integrated feature; "partial" = exists elsewhere but Kryos integrates it
> better at the language level; flags note what needs language work first.
>
> Per-project build specs: `projects/NN-*.md` (each is self-contained -- open
> one in a fresh Claude session and build it). Deep per-primitive analysis:
> `unlocks/*.md`.

## The wedge (Kryos's purpose-built domain)

**Kryos is the language for trustworthy AI agent software.** Every function
declares what resources it may touch (`@capabilities`), how many tokens/calls
it may spend (`@budget`), what it costs (`std.cost`), where each output came
from (`std.tracked`), and how certain it is (`std.probable`). These five
governance axes are compile-time or runtime *language* properties, not
framework conventions bolted on top.

- **Mojo** owns AI compute kernels (GPU/SIMD).
- **Rust** owns memory safety.
- **Kryos** owns **AI agent governance + safe execution** -- the layer between
  a model and the real world.

One-line pitch: *Write AI agents where runaway loops, silent capability
escalation, unattributed outputs, and unbounded spend are compile errors or
runtime throws -- not postmortem discoveries.*

Why this is defensible (and not another benchmark race): raw speed is table
stakes (Kryos is already within 1.45x of Rust, beats it on some). This wedge
is a *capability* no other general-purpose language has assembled in one place,
and it is exactly timed to the moment the industry is scrambling to make
AI-written and agent-executed code safe to run.

## The five signature axes (the "Kryos-specific functions")

| Axis | Mechanism | Novelty | Honest limit today |
|---|---|---|---|
| **Spend** | `@budget(tokens=N, calls=M)` attribute -> MIR-injected runtime frame; every `chat()` pre-charges; exhaustion THROWS inside the body | **truly-novel** | `usd`/`energy` axes not wired (money/energy fields are 0.0) |
| **Authority** | `@capabilities(net,io,ffi,...)` compile-time, cross-fn propagation (E0507) + attenuation (E0503); compiler builds a per-fn capability map | partial; the **compiler-extracted per-function manifest is truly-novel** | OPT-IN today (unannotated fns unconstrained); deny-by-default + runtime enforcer + sub-caps are PLANNED |
| **Provenance** | `std.tracked` `Tracked<T>` lineage chain in the type | partial | propagation is manual; no compiler warning on unwrap |
| **Confidence** | `std.probable` `Probable<T>` confidence in the type | partial | no operator overloading yet (needs generic impls); values asserted not calibrated |
| **Cost** | `std.cost` `ComputeCost{wall,tokens,calls,usd,energy}` as a composable value | partial | usd/energy require caller-supplied pricing |

Plus: budget-aware `std.llm` clients in the stdlib (zero deps), dual backend
(Cranelift JIT + LLVM AOT, all axes work on both), and an experimental wasm32
target (not yet struct/stdlib-capable -- do not overstate).

## Repository layout

Each buildable ecosystem project is a standalone Kryos project (its own
`kryos.toml`) living flat at **`ecosystem/<project-name>/`** -- e.g.
`ecosystem/kryos-agent-loop/`, `ecosystem/kryos-rag/`,
`ecosystem/kryos-audit-trail/`, `ecosystem/kryos-bench-governed/`. New projects
go in the same flat shape (no `builds/` subfolder). Compiler-native features
(like project 01, a `kryos-cli` subcommand) live under `compiler/`, not
`ecosystem/`. Shared planning docs: `ecosystem/ECOSYSTEM.md` (this file),
`ecosystem/projects/` (per-project specs), `ecosystem/unlocks/` (primitive
analyses).

## Project roadmap (build order)

10 of 12 are buildable on Kryos **today**; 2 need language work. Build along
the dependency chain. Each `NN` links to `projects/NN-*.md`.

### Foundation (build first)
- **01 kryos-manifest CLI** (truly-novel, S, today) -- **DONE 2026-06-15.**
  Shipped in the compiler repo (kryos-lang), not as an `ecosystem/` Kryos project,
  because it is a `kryos-cli` Rust subcommand. `kryos manifest --caps <path>`
  serializes the compiler's per-function capability map (`CapabilitySet`) to JSON
  (`kryos-manifest-v1`). Flags: `--format json|pretty`, `--strict`, `--output`,
  `--deny <cap>[,...]` (exit 1; `all` trips any). Walks top-level fns + impl/trait
  methods; warns on unparseable files. 18 integration tests (6 golden snapshots).
  Source: `kryos-lang` commit `c3aeac1` on `master`,
  `compiler/crates/kryos-cli/{src/commands/manifest_cmd.rs,tests/manifest*}`.
  The keystone: unlocks registry badging, CI capability-diff gates, playground
  sandboxing.
- **02 kryos-governed-agent stdlib extension** (S, today, dep 01) -- the 5
  missing bridges (`tracked_cost`, `tracked_merge`, `tracked_to_citation`,
  `budget_remaining`, ...) that fuse the axes into one ergonomic surface.
- **10 kryos-calibration** (S, today) -- adds `CalibrationSample` + `ece()` to
  `std.probable` so confidence becomes a *measured* quantity (independent).

### Flagship libraries (the wedge made real)
- **kryos-embed** (BUILT, checked green) -- deploy a governed Kryos agent
  INSIDE software written in other languages: C-ABI DLL + WASM with a
  compiler-backed authority manifest (`agent.caps.json`); Python/Go/Node
  hosts read the manifest and refuse over-privileged agents before binding;
  budget refusal before spend; private data never leaves the host process.
  `bash ecosystem/kryos-embed/check.sh` = PASS 4/0 (C# recipe-only).
- **03 kryos-agent-loop** (M, today, dep 02) -- governed multi-turn tool-use:
  every LLM call + tool dispatch logged with cost/latency as the default.
- **04 kryos-rag** (M, today, dep 02,03) -- RAG where every answer's citations
  are a first-class `Tracked` value, not post-hoc metadata.
- **09 kryos-mcp-governed** (M, today, dep 01,02,03) -- MCP server template
  where tools are `@capabilities`-annotated Kryos fns; refuses to register a
  tool that exceeds its declared surface. (Ties into existing kryos-mcp-template.)

### Infra (ecosystem credibility)
- **05 kryos-registry capability badging** (truly-novel, M, today, dep 01) --
  every package ships a machine-readable capability badge; see net-vs-compute
  before install. Builds on the existing kryos-registry.
- **06 kryos-playground capability-gated sandbox** (M, today, dep 01,05) -- the
  online REPL uses the language's own static analysis as its sandbox policy.

### Apps / proof points
- **07 kryos-bench-governed** (M, today, dep 02,03) -- AI benchmark harness
  where the budget is a `@budget` attribute (pre-call refusal), not a shell
  timeout.
- **08 kryos-audit-trail** (S, today, dep 02,05) -- `to_json(decision)` on a
  `Tracked<str>` emits EU AI Act Article-13-shaped audit records, per-value.

### Needs language work first (highest-leverage compiler investments)
- **11 strict-capabilities / deny-by-default** (truly-novel, M) -- remove the
  `has_annotated_scope()` opt-in guard + add `--strict-capabilities`. This is
  what turns "capability-proven" from a documentation claim into an actual
  security property. ~2-3 days. **Do this to make the wedge honest.**
- **12 kryos-plugin-sandbox** (L, dep 11) -- load untrusted `.kry` plugins
  compiled to wasm; the capability annotation *is* the import section.
  Needs 11 + full wasm type coverage.

## Language work to prioritize (honest prerequisites)
1. **`--strict-capabilities` / deny-by-default** (project 11) -- the single
   change that makes the authority axis real. Highest ROI.
2. **Generic impl blocks** -- unlocks operator-level confidence propagation on
   `Probable<T>` and method syntax on `Tracked<T>`.
3. **wasm backend struct/enum/closure/stdlib support** -- unlocks the plugin
   sandbox + edge tier (currently scalars/arrays/CF only, no CI).
4. **`@budget(usd=)` + USD charge path** -- makes the spend ceiling a real
   billing cap, not a documented 0.0 gap.
5. **Sub-capabilities** (`fs:read`, `net:http`, `db:read|write`) -- finer
   least-privilege; needed for practical plugin attenuation.

Do NOT block projects 01-10 on items 2-5; they are buildable today.

## How to use this
- Pick a project, open `projects/NN-*.md` in a fresh Claude session, build the
  MVP scope first.
- Sequence suggestion: 01 -> 02 -> (03, 10 in parallel) -> 04/05 -> the rest.
  Then 11 to make "capability-proven" honest, then 12.
- The whole slate is designed to compound: 01's manifest feeds 05's badging
  feeds 06's sandbox; 02's bridges feed 03/04/07/08/09.
