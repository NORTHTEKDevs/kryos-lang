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
3. [DONE] Async cross-await state — commit pending
   - [x] ROOT CAUSE: the CPS await-split (apply_split_at_awaits) was broken
     (treated i64 param as a state struct) AND unnecessary (thread-per-task
     executor keeps each task's stack across await).
   - [x] FIX: disabled await-split by default (run.rs + config default);
     scaffolding kept dormant, opt-in via KRYOS_ENABLE_AWAIT_SPLIT.
   - [x] multi-await + cross-await locals now compile+run correct JIT==AOT;
     interleave A0 B0 A1 B1 preserved; async_io concurrent. native corpus green.

## ALL THREE DONE.

## Log
- Started from master @ bc6443d (8 gaps closed earlier this session).
