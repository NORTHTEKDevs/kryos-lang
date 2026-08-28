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

Cross-backend probe corpus status, **re-measured 2026-08-27** by
`tests/wasm_differential_gate.sh` over `tests/harden-probes/` +
`examples/wasm_*.kry`: **65 / 65 compile on wasm, and all 65 produce output
identical to the native backends. 0 miscompiles.**

This section previously claimed 37/48 with maps and closures listed as hard
compile-time gaps. That was stale in the *understating* direction - maps and
HOF/closure-as-value probes now compile AND agree with native, verified by
running them under `node tools/wasm-host/run.mjs` and diffing against
`kryos run`. The roadmap items below that claimed to "unblock" them are
already done.

**Formerly the single remaining gap in the probe corpus (probe 23, complex
control flow / irreducible CFG, string-op-heavy loop) -- CLOSED 2026-08-27**
as a side effect of the short-circuit-in-loop ICE fix below: probe 23 falls
back to the same dispatch relooper and hit the identical `Return(None)` /
non-void-function stack-mismatch bug, reproduced and confirmed by reverting
the fix (same error class, different offset: `type mismatch: expected i64
but nothing on stack (at offset 0x92c)`). It now compiles and matches native
exactly -- see `wasm_differential_gate.sh`'s 65/65 count above, which was
61/62 (probe 23 the one refusal) before this fix.

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

**A compile-time ICE found 2026-08-17, FIXED 2026-08-27 -- short-circuit
`&&`/`||` inside a loop, reassigning a `mut str` local in both if/else
arms:** `examples/showcase/wordscope.kry`'s WASM leg used to fail to build --
`kryos build --backend wasm` refused to WRITE the module at all (the
validator caught it, exactly as designed -- the "clean refusal" case, not a
miscompile) with `type mismatch: expected i64 but nothing on stack`. ROOT
CAUSE was not the short-circuit lowering: this CFG shape is beyond what the
structured control-flow translator can express, so it correctly falls back
to the dispatch relooper (`emit_relooper`), which emits an
`if pc==i {...}` case for EVERY block position unconditionally -- including
a dead, zero-incoming-edge "drop locals; return" epilogue block MIR appends
after this function's real `return`, reached only by the relooper's blanket
emission, never dynamically. `wasmparser` validates every path statically
regardless of reachability, so a bare `return` (no value) inside a non-void
function failed type-checking even though unreachable at runtime. Fixed by
pushing a placeholder of the function's declared return type before a
valueless `Return` in `emit_relooper_terminator`
(`compiler/crates/kryos-codegen-wasm/src/lib.rs`), mirroring the identical
fallback `emit_function` already uses for a body that falls off the end.
Regression pinned at `tests/harden-probes/probe_wasm_shortcircuit_loop_strcat.kry`
(covered by `wasm_differential_gate.sh`); `wordscope.kry`'s wasm leg in
`tests/run_examples_e2e.sh` now hard-fails on a build regression instead of
skipping a known gap.

**Narrow-int (`u8`/`i8`/`u16`/`i16`/`u32`/`i32`) truncation, re-verified
2026-08-27:** the plain-local case was already fixed 2026-08-14
(`tests/harden-probes/probe_narrow_int_wrap.kry`). The struct-FIELD case
(`s.field = s.field + n` on a narrow-typed field) was never independently
gated against wasm -- the existing native conformance test for this shape
(`tests/conformance/conf_narrow_struct_field_store.kry`) calls `exit()`,
which wasm rejects at compile time, so it silently never ran on wasm at all.
Manually verified live: wasm already wraps struct-field arithmetic
correctly, byte-identical to both native backends, across every narrow
width, a two-adjacent-narrow-fields layout, and a narrow-field-next-to-a-
wide-field layout (the exact shape that corrupted a neighbor on the old AOT
bug) -- wasm represents every struct field as its own full i64 slot, so
there is no narrower-than-i64 storage for a value to overflow the way LLVM's
native struct layout once did. No fix needed; gated going forward at
`tests/harden-probes/probe_narrow_struct_field_wrap.kry`.

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

1. ~~Irreducible-CFG lowering to structured wasm blocks - unblocks probe 23~~
   DONE 2026-08-27 as a side effect of the dispatch-relooper `Return(None)`
   fix -- probe 23 now compiles and agrees with native.
2. Extend the differential leg beyond the probe corpus: wire
   `--backend wasm` in as a third comparison leg of `tools/diff-fuzz/` so
   randomly generated programs are diffed three ways (JIT / AOT / wasm),
   not just JIT-vs-AOT.
4. Target: the full 48-probe corpus as the standing wasm gate.
