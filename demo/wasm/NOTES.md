# WASM Demo Notes

## What runs

The same governed agent logic from `demo/native/main.kry` compiled to
`wasm32-unknown-unknown` via `kryos build --release --backend wasm` and
executed by `node tools/wasm-host/run.mjs`. The host harness (run.mjs)
provides the full `env` import surface; the module is NOT WASI.

## WASM backend v0.1 gaps (why demo/wasm/main.kry differs from demo/native/main.kry)

| Feature | Native | WASM (this demo) |
|---|---|---|
| Structs (AgentReply) | Yes | **No** — v0.1 rejects any struct type |
| @budget annotation | Yes | **No** — kryos_budget_push_usd hook not wired |
| to_string(i64) | Yes | **Added** — kryos_to_string_i64 host import (this iteration) |

### Struct limitation
`AgentReply { answer, source, calls_now, refused }` cannot be used in WASM v0.1
because the backend rejects any non-primitive type beyond str/i64/f64/bool/[T].
**Workaround:** the agent function returns a single `str` that encodes all
governance metadata (answer, source, tokens, calls/max). Callers parse the
string with `grep`/`contains` rather than struct field access.

### @budget limitation
The `@budget(calls = N)` compile-time annotation desugars to a
`kryos_budget_push_usd` runtime hook that is not yet wired in the WASM host
contract. The compile-time call ceiling is intentionally omitted on the WASM
build. The **application-level budget guard** (`if calls_spent >= max_calls`)
is preserved and fully functional — refused calls record no spend.

### to_string fix (this iteration)
`to_string(i64)` was previously unsupported in WASM v0.1 (the backend would
error with "call to `to_string` — supported builtins: ..."). This iteration
added `kryos_to_string_i64(v: i64) -> packed_str` as a host import, wired in:
- `compiler/crates/kryos-codegen-wasm/src/lib.rs` (import + emit_call handler)
- `tools/wasm-host/run.mjs` (JS implementation writing into linear memory)

## Governance properties verified

- (a) Within-budget call: answer text + `source=mock-llm-v1` visible in output
- (b) Over-budget call: `REFUSED:budget exhausted` prefix, `SPEND:0` (no mock LLM called)
- @capabilities(net:http) annotation present on agent_query and mock_llm — propagates to all callers
