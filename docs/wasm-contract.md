# WebAssembly backend — the supported contract (verified 2026-07-02)

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

The wasm expansion gate lives at `spec/wasm-acceptance.sh`
(6 probe programs with exact-output `.expect` files + the full cargo suite);
it passes green on this commit.

## Compile-time rejected (use `--backend cranelift` or `llvm`)

Cross-backend probe corpus status: **7 / 48** of `tests/harden-probes/`
compile and agree with the JIT on wasm (was 0/48 before the expansion).
The remaining gaps are aggregate-heavy constructs, all still explicit
compile errors, never miscompiles:

- enums (incl. Option/Result) and enum payloads
- maps
- struct mutation through collections; aggregate array elements
- closures capturing structs; higher-order functions over aggregates;
  currying
- `?` operator; `if let`; tuple matching

## Roadmap

1. Enums + Option/Result as tagged slot records (same linear-memory encoding
   as structs) — unlocks `?`, `if let`, and most of the remaining corpus.
2. Maps via host-backed handles (mirror the array helpers).
3. Aggregate element mutation (write-back through slot pointers).
4. Target: the full 48-probe corpus as the standing wasm gate.
