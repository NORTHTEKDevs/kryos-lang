# WebAssembly backend — the supported contract (verified 2026-07-01)

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
| Strings (packed i64 handles) | literals, params/returns, `len(s)`, explicit `str_concat(a, b)` builtin |
| Arrays of scalars | explicit `array_new` / `array_get` / `array_set` builtins, `len` |
| Governed-agent embed | the whole `demo/wasm` + `ecosystem/kryos-embed` Node host runs on this subset |

## Compile-time rejected (use `--backend cranelift` or `llvm`)

| Feature | Error says |
|---|---|
| `+` string concatenation (operator form) | `rvalue 'string-concat'` — use the `str_concat` builtin |
| `push` / most stdlib builtins | supported builtin list is printed in the error |
| `match` / `switch` | use if/else chains |
| structs, enums, tuples, maps | aggregate types not lowered |
| array literals / `arr[i]` sugar | use the explicit array builtins |

## Known bugs (tracked)

- `println(to_string(f))` on f64 builds but traps at instantiation
  (host import typed i64 receives f64). Workaround: keep printed values i64.
- `examples/wasm_strings.kry` / `wasm_arrays.kry` header comments describe
  `+`-concat and array-literal support ("v0.2/v0.3") that the current gate
  rejects — treat the table above (probe-verified) as authoritative until the
  operator lowerings land.

## Roadmap (in priority order)

1. Operator lowerings onto the existing primitives (`+` concat → str_concat;
   array literals / `arr[i]` → array_new/get/set; `push`).
2. `match` lowering (decision tree → br_table / if-chains).
3. f64 host-import fix.
4. Aggregates (structs/enums/tuples) in linear memory — unlocks the full
   48-probe corpus as a wasm gate.
