# Overnight build progress — finishing the language

Goal: build the three deferred features, each verified on both backends + committed.
Method: sequential in-session (headless night-shift is unreliable on this machine —
Defender CPU pin + MCP leak, per prior logs). No commit unless green.

Acceptance check per feature: a `.kry` regression test compiles + runs correct on
JIT AND AOT, and native corpus + strict-caps + soundness + ecosystem stay green.

## Queue
1. [DONE] unsafe {} / E0500 enforcement (commit pending)
   - [x] E0500 constant + Expr::UnsafeBlock AST + parser + checker unsafe_depth
   - [x] raw-ptr deref outside unsafe -> E0500; inside -> ok; transparent codegen
   - [x] all 8 exhaustive-match crates updated; native+units+gates green
   - [x] tests: native/unsafe_blocks.kry + showcase/unsafe_violation_overreach.kry
2. [ ] WASM capturing closures (heap-env ABI)
3. [ ] Async cross-await state (CPS state-machine transform)

## Log
- Started from master @ bc6443d (8 gaps closed earlier this session).
