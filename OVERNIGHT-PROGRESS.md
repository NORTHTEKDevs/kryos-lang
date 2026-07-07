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
2. [DONE] WASM capturing closures (heap-env ABI) — commit pending
   - [x] env-for-all: closure value = array [thunk_idx, caps...]
   - [x] synthesized thunk MirFunction per lambda (unpacks caps, calls lambda)
   - [x] call_indirect via env[0] thunk; 2nd scratch local for env-across-args
   - [x] ALL cases wasm==native: nocap, direct-cap, escaping(map/filter), apply, curry
   - [x] f64-capture -> honest error (not trap); wasm corpus 11/11; native green
3. [ ] Async cross-await state (CPS state-machine transform)

## Log
- Started from master @ bc6443d (8 gaps closed earlier this session).
