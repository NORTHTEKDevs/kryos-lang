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

The single remaining gap in the probe corpus itself:

- complex control flow / irreducible CFG (1 probe: 23, string-op-heavy loop)
 - correctly REFUSED at compile time.

**Additional gaps found building a real program (`examples/showcase/wordscope.kry`,
2026-08-16), outside the probe corpus above and not previously listed here --
all correctly REFUSED at compile time, not silent:**

| Rejected | Notes |
|---|---|
| `split(s, sep)` (the GLOBAL builtin, no import) | refused (`does not yet support: call to split`) -- **`use std::string::{split}` (the stdlib-level wrapper) DOES work on wasm**; only the unimported global builtin is refused. Tokenize by hand with `char_code`/`substr` if you need the global form's exact semantics, or `use` the stdlib one. |
| `to_lower(s)` (`std::string`) | refused (`kryos_builtin_to_lower`) -- lowercase ASCII by hand via a literal lookup table + `char_code`/`substr`. |
| `char_from(n)` / `chr(n)` | refused -- these two names alias the same codepoint constructor (see `core-builtins.md`), and wasm does not implement either. |
| `round(f: f64)` | refused -- no float-rounding builtin on this backend yet. |
| `arr[i] = v` (array INDEX ASSIGNMENT) | refused (`kryos_array_set`) -- note the asymmetry: array LITERALS, `arr[i]` READ, `push`, and `len` are all in the "Verified working" table above; only the WRITE form is unsupported. |

**A compile-time ICE found the same session, distinct from the gaps above --
short-circuit `&&`/`||` inside a loop, reassigning a `mut str` local in both
if/else arms:** `examples/showcase/wordscope.kry`'s WASM leg (added to
`tests/run_examples_e2e.sh` this session) does not build --
`kryos build --backend wasm` refuses to WRITE the module at all (the
validator catches it, exactly as designed -- this is the "clean refusal"
case, not a miscompile) with `type mismatch: expected i64 but nothing on
stack`. Isolated to a minimal repro at
`tests/known_failures/wasm_shortcircuit_loop_strcat.kry`: all three of (a
short-circuit `&&`/`||` condition, an `if`/`else` inside a `while` loop, a
`mut str` local reassigned by concatenation in BOTH arms) are required --
removing any one compiles clean. Not fixed this session (a genuinely deep
codegen investigation, out of scope for a docs-and-showcase wave); tracked
as an OPEN item in `tools/loop/LEDGER.md`. Workaround: nest two single-
condition `if`s instead of one `&&`, or accumulate into an `[i64]` buffer
and build the string once outside the loop.

**Semantic-correctness caveat on the "never a miscompile" guarantee below:**
the `wasmparser` structural-validity check (next paragraph) proves the emitted
module is a well-formed wasm binary -- it cannot and does not prove the
module computes the right ANSWER. A real semantic miscompile was found and
fixed this same session, invisible to that validator: the `==` operator on
`str` compiled cleanly and ran, but compared the packed `(offset, len)`
HANDLE rather than the string's content, so a heap-built string (concat,
substr, a function return) never equalled an equal-content literal even
though the bytes matched -- silently `false`, exit code 0, a structurally
valid module the whole way. Fixed by routing `str == str` / `str != str`
through a new `kryos_string_eq` host import that compares actual bytes
(`kryos-codegen-wasm/src/lib.rs`, `tools/wasm-host/run.mjs`) instead of a
bare `I64Eq` on the packed value. If you are auditing this backend for a new
gap, do not stop at "it validates" -- diff the OUTPUT against `kryos run`/
`kryos build --release` on the same source, which is exactly what
`tests/wasm_differential_gate.sh` automates.

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
