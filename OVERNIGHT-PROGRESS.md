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


## BONUS (beyond the 3 features)
4. [DONE] wasm narrow-int casts (x as u8/i8/u16/i16) — uniform i64 int slots (98eaf88 prior)
5. [DONE] wasm single-level `for i in 0..n` range loops (98eaf88)
6. [VERIFIED] native language complete — advanced-feature probe (dyn traits, nested
   generics, generic methods, Result?/chain, closures-in-structs, recursive trees)
   all pass both backends; only turbofish-literal construct is a gap (matches Rust).

## REMAINING (documented residuals, NOT rushed overnight — regression risk too high)
- wasm NESTED loops: needs a relooper/structured-CFG rewrite (the translator is
  single-level). High-value but large+risky; use recursion or native for now.
- chan buffered() / try_receive(): needs fragile multi-backend runtime-builtin
  wiring for a concurrency edge; select is the documented non-blocking path.
- wasm f64/f32 captures: edge (i64/str/handle captures work).
These are honest, documented limitations — not silent-wrong.


## RESIDUALS (round 2 - user asked to do them)
R1. [DONE] wasm NESTED loops (1f98edd) - dispatch-relooper fallback (loop + pc-dispatch,
    branch-free terminators via select). Zero-regression: structured tried first, relooper
    only on error. Fixed inner-branch continue miscompile too. nested/triple/continue/break
    all wasm==native; corpus 10/10.
R2. [CLOSED-as-limitation] wasm f64/f32 closure captures. Bitcast plumbing
    (I64ReinterpretF64 store / F64ReinterpretI64 load) leaked across the direct-call
    AND HOF paths - reverted to the honest compile error. i64/str/handle captures +
    all HOFs work; native backends have full f64-capture parity. Documented in
    STABILITY 5.0 as a bounded wasm residual, not silent-wrong.
R3. [DONE] chan non-blocking try_receive + honest buffered docs.
    - Wired chan_try_recv (status 1/0/-1) + chan_last_recv builtins end-to-end:
      MIR builtin table, cranelift dispatch + JIT symbol/decl, LLVM codegen arms,
      type-checker sigs. Runtime primitives already existed (kryos_chan_try_recv_status_i64
      + kryos_chan_last_recv_i64); the queue is an unbounded Mutex<VecDeque> (already
      buffered - send never blocks).
    - stdlib try_receive() rewritten to genuinely non-block (was a blocking stub that
      always reported ok=true). This ALSO un-breaks select_cases() and try_acquire(),
      which busy-poll on try_receive and previously blocked forever.
    - Corrected the misleading "unbuffered" module docs (runtime is unbounded-buffered;
      capacity is an advisory hint, no hard backpressure bound - documented).
    - PARSER FIX: `use std::chan::{...}` now parses. `chan` is a reserved keyword, so
      the whole chan module was previously unimportable; added expect_path_segment so a
      use-path segment accepts a reserved-keyword module name (unambiguous in that
      position). Applies to chan/select/type/... as module names.
    - Verified: corpus chan_try_recv.kry (raw builtins, deterministic) + example
      chan_try_receive.kry (std::chan wrapper) both JIT==AOT correct. Native JIT +
      AOT suites, examples-gate (root 45/45), strict-caps 89/89, soundness all green.

## ALL ROUND-2 RESIDUALS RESOLVED (R1 done, R2 closed-as-limitation, R3 done).
