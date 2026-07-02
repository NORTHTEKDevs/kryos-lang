# Night-shift spec: WASM backend expansion

## Goal
Close the gap between the wasm backend's explicit subset and everyday Kryos:
operator string concat, array literal/index/push sugar, match lowering, the
f64 host-import bug, and (stretch) struct aggregates -- so the same programs
people write natively compile to wasm or fail for a REASON, not a gap.
docs/wasm-contract.md describes the verified current subset; update it as
features land.

## Acceptance check (the only truth)
`bash spec/wasm-acceptance.sh` -- exit 0 = done.
It requires: cargo build + FULL cargo suite green (native backends must not
regress!), and every probe in tests/wasm-probes/ compiling with
`--backend wasm` and matching its .expect byte-for-byte via
`node tools/wasm-host/run.mjs`.

## Feature order (easiest first -- bank wins each iteration)
See spec/features.json. Roughly: f64 println fix -> `+` concat operator ->
array sugar (`[..]`, `arr[i]`, `push`) -> string interpolation -> match
lowering -> structs in linear memory (stretch).

## Where the code is
- Backend: compiler/crates/kryos-codegen-wasm/src/lib.rs (single file; the
  unsupported-feature gate fn is near line 73; strings are packed
  (offset,len) i64 handles; arrays are linear-memory via host helpers).
- Host: tools/wasm-host/run.mjs (host imports live here; the f64 bug is an
  import typed i64 receiving f64 -- fix BOTH sides coherently).
- MIR input is the same as the native backends -- the wasm backend maps MIR,
  it does not re-lower AST. Reuse the native backends' lowering decisions.

## Hard rules
- NEVER weaken the cargo suite, native tests, or a probe/.expect to pass.
- NEVER touch spec/, .github/, demo/, ecosystem/ (docs/wasm-contract.md is OK).
- Build with `cargo build --release -j 4` only. Kill kryostokens.exe strays
  each iteration (taskkill //F //IM kryostokens.exe).
- Local commits only; NEVER push. No live network calls.
- Read repo CLAUDE.md before writing any .kry (no semicolons; elif; string
  interpolation braces).
- Partial progress is fine: commit whatever compiles + moves a probe from
  FAIL to PASS. End every session with <promise>NEXT</promise>,
  <promise>DONE</promise> (only if the gate exits 0), or
  <promise>BLOCKED</promise>:<reason>.
