# WebAssembly backend - the supported contract (verified 2026-07-04)

`kryos build --release --backend wasm` targets `wasm32-unknown-unknown` with a
**JS host contract** (browser or `node tools/wasm-host/run.mjs`) - not WASI.
The backend is an explicit subset: anything outside it fails **at compile
time** with a clear `unsupported in WASM` error naming the construct - it
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
| Arrays | literals `[1,2,3]`, indexing `arr[i]`, `push`, `len` - normal syntax, linear-memory backed (2026-07-02) |
| `match` on scalars | or-patterns (`1 \| 2 \| 3`), default arm, str-returning arms (2026-07-02) |
| Structs | literals + field access via slot encoding in linear memory (2026-07-02) |
| Governed-agent embed | the whole `demo/wasm` + `ecosystem/kryos-embed` Node host runs on this subset |
| All `examples/wasm_*.kry` | all 9 build and run via the node host (verified 2026-07-03) |

The wasm expansion gate lives at `spec/wasm-acceptance.sh`
(9 probe programs with exact-output `.expect` files + the full cargo suite);
it passes green on this commit.

## Compile-time rejected (use `--backend cranelift` or `llvm`)

Cross-backend probe corpus status, **re-measured 2026-08-14** by
`tests/wasm_differential_gate.sh` over `tests/harden-probes/` +
`examples/wasm_*.kry`: **61 / 62 compile on wasm, and all 61 produce output
identical to the native backends. 0 miscompiles.**

This section previously claimed 37/48 with maps and closures listed as hard
compile-time gaps. That was stale in the *understating* direction - maps and
HOF/closure-as-value probes now compile AND agree with native, verified by
running them under `node tools/wasm-host/run.mjs` and diffing against
`kryos run`. The roadmap items below that claimed to "unblock" them are
already done.

The single remaining gap:

- complex control flow / irreducible CFG (1 probe: 23, string-op-heavy loop)
 - correctly REFUSED at compile time.

> **How the "never a miscompile" guarantee is actually enforced.** It is not a
> convention; `emit_module` runs the emitted bytes through `wasmparser`'s
> validator and refuses to write a structurally invalid module. This was added
> because the guarantee was being broken: on 2026-08-14 probe 23 - 
> the very probe this document lists as an out-of-subset gap - compiled with
> **exit code 0** and produced a `.wasm` that could not instantiate at all
> (`CompileError: Compiling function #46 failed: expected 1 elements on the
> stack for return, found 0`). A build reporting success while writing an
> artifact that cannot load is strictly worse than a clear refusal, and a user
> would only have found out in a browser. The structural check now happens at
> compile time, where this document always said it did.

## Roadmap

1. Irreducible-CFG lowering to structured wasm blocks - unblocks probe 23, the
   last remaining gap.
2. Extend the differential leg beyond the probe corpus: wire
   `--backend wasm` in as a third comparison leg of `tools/diff-fuzz/` so
   randomly generated programs are diffed three ways (JIT / AOT / wasm),
   not just JIT-vs-AOT.
4. Target: the full 48-probe corpus as the standing wasm gate.
