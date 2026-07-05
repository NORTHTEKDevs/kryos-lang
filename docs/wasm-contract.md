# WebAssembly backend — the supported contract (verified 2026-07-04)

`kryos build --release --backend wasm` targets `wasm32-unknown-unknown` with a
**JS host contract** (browser or `node tools/wasm-host/run.mjs`) — not WASI.
The backend is an explicit subset: anything outside it fails **at compile
time** with a clear `unsupported in WASM` error naming the construct — it
never miscompiles silently. Every row below was verified by compiling and
running a probe program on this commit.

## Verified working

| Feature | Notes |
|---|---|
| i64 / f64 / bool compute | arithmetic, comparisons, casts |
| Functions + recursion | `fib(15)` verified end-to-end |
| Closures | capture-by-value, calls verified (`|x| x + n`) |
| `if` / `elif` / `else`, `while` | full support |
| `println` / `to_string` of i64 | via host imports |
| Strings (packed i64 handles) | literals, params/returns, `len(s)`, `+` concat (plain and chained), interpolation with i64 parts (`"n is {n}"`) |
| f64 output | `println(to_string(f))` via a dedicated f64 host import (2026-07-02) |
| Arrays | literals `[1,2,3]`, indexing `arr[i]`, `push`, `len` — normal syntax, linear-memory backed (2026-07-02) |
| `match` on scalars | or-patterns (`1 \| 2 \| 3`), default arm, str-returning arms (2026-07-02) |
| Structs | literals + field access via slot encoding in linear memory (2026-07-02) |
| Governed-agent embed | the whole `demo/wasm` + `ecosystem/kryos-embed` Node host runs on this subset |
| All `examples/wasm_*.kry` | all 9 build and run via the node host (verified 2026-07-03) |

The wasm expansion gate lives at `spec/wasm-acceptance.sh`
(9 probe programs with exact-output `.expect` files + the full cargo suite);
it passes green on this commit.

## Compile-time rejected (use `--backend cranelift` or `llvm`)

Cross-backend probe corpus status: **37 / 48** of `tests/harden-probes/`
compile on wasm (was 7/48 before the harden-probe expansion).
The remaining 11 gaps are all explicit compile errors, never miscompiles:

- maps (`rvalue map` — 5 probes: 04, 17, 24, 27, 39)
- closures and higher-order functions passed as `fn(T) -> U` values —
  currying, HOF filter/fold, closures capturing structs (5 probes: 09, 15, 20, 21, 40)
- complex control flow / irreducible CFG (1 probe: 23, string-op-heavy loop)

## Roadmap

1. Maps via host-backed handles (mirror the array helpers) — unblocks 5 probes.
2. First-class function values as wasm table entries — unblocks closures-as-values
   and all HOF probes (5 probes).
3. Irreducible-CFG lowering to structured wasm blocks — unblocks probe 23.
4. Target: the full 48-probe corpus as the standing wasm gate.
