# Kryos Showcase — verified working

Everything on this page was executed and asserted green on 2026-07-01 with the
in-tree compiler (`kryos run` Cranelift JIT; `kryos build --release` LLVM AOT
where noted). Re-run any row yourself — each links to a runnable artifact.
Full sweep: **309 artifacts checked, 181 executed green**; the remainder are
servers/GUI/network demos that need a live environment (they compile; they
block on ports/windows by design) or library-only modules with no entry point.

## The flagship: one governed agent, four host environments

The same governed agent — `@capabilities` authority verified at compile time,
`@budget` refusal **before** spend, provenance on every answer — proven in:

| Host | Mechanism | Check |
|---|---|---|
| Native Kryos binary | JIT + AOT | `bash demo/native/check.sh` |
| WebAssembly (Node host) | `--backend wasm` | `bash demo/wasm/check.sh` |
| C program | C ABI DLL + link recipe | `bash demo/cabi/check.sh` |
| Python / Go / Node apps | [kryos-embed](ecosystem/kryos-embed/README.md) SDK | `bash ecosystem/kryos-embed/check.sh` |

Each check asserts three governance properties end to end: a within-budget
call returns the answer **with its source**; an over-budget call is refused
with the spend counter still at zero; a function calling the LLM without
declaring the capability **fails to compile** (`E0505`/`E0507` under
`--strict-capabilities`). The embed hosts additionally refuse to load an
agent whose authority manifest exceeds the host's allow-set — before a single
symbol is bound.

## Real programs (examples/showcase/, all JIT-green)

Interpreters & tools: `bytecode_vm` `brainfuck` `rpn_calc` `calc` `parser`
`regex` `template` · Apps: `todo_app` `kvdb` `tiny_kv` `ssg` (static-site
generator) `markdown` `kdoc` · Data: `stats_pipeline` `word_frequency` `csv`
`regression` `budget_analyst` · Systems: `worker_pool` `dir_walker`
`agent_runtime` `cli_tool` · Servers (compile + bind; skipped in CI as they
block): `web_server` `rest_api` `tcp_echo` `mcp_server`.

Root examples additionally cover FFI (`ffi_libc`, Windows CRT), a ray tracer,
a neural net, JSON tooling, and the kitchen-sink `all_features.kry`
(30 language sections in one file, including the `|>` pipeline operator).

## Compiler correctness evidence

- **Cross-backend agreement:** 48 adversarial probes (`tests/harden-probes/`)
  — aggregates through collections, closures, enums-in-enums, maps, generics,
  deep nesting — byte-identical stdout on Cranelift JIT and LLVM AOT.
- **Suite:** full cargo test suite green (includes 136 native `--release`
  build-and-run tests with exact stdout assertions).
- **Recent hard fixes** (all regression-tested in
  `compiler/crates/kryos-test-runner/tests/native/`): in-place mutation of
  aggregates inside collections on both backends; `|>` result-type inference;
  `try`/`catch` as a value expression; catch-binding lifetime.

## Ecosystem (44 packages under `ecosystem/`)

Governance: `kryos-capi` (C exports with compiler-verified capability
manifests) · `kryos-embed` (agents in foreign hosts) · `kryos-policy` ·
`kryos-audit-trail` · `kryos-cost-ledger` · `kryos-fs-jail` ·
`kryos-pii-guard` · `kryos-injection-guard` · `kryos-plugin-sandbox` ·
Agent infra: `kryos-agent-loop` · `kryos-rag` · `kryos-llm-router` ·
`kryos-llm-structured` · `kryos-mcp-governed` · `kryos-tool-broker` ·
Plus schema/config/logging/tracing/checkpoint/replay utilities — each with
its own tests (executed in the same sweep).

## Honest boundaries

- LLM calls in every demo above are **deterministic mock stubs** — the point
  is proving governance and portability, not a model round-trip. Live
  providers plug in via `std::llm` / `std::http`.
- The C-ABI DLL recipe is Windows-verified (zig cc); Linux/macOS use the same
  IR-patch route with `dlopen` (documented, not yet CI-verified).
- C# host binding is a documented P/Invoke recipe, untested (no .NET SDK on
  the build machine).
- `kryos build --backend wasm` is a JS-host contract (browser/Node), not WASI.
