# kryos-eval-replay

Deterministic, budget-bounded replay of recorded agent transcripts.

Agent eval frameworks (inspect-ai, LangSmith) replay transcripts but enforce
spend with external counters that live *outside* the replay. kryos-eval-replay
puts the ceiling *inside* the runner: the replayer derives its token/call
envelope from the recorded transcript's own totals and enforces it with the
same runtime primitive the `@budget(...)` function attribute lowers to. So
re-running a recorded transcript **provably cannot exceed the original's
token/call envelope** -- a divergent re-run is refused *before* the offending
model call -- and every run reports a `ComputeCost` delta versus the original.

"Re-run this incident transcript safely" becomes a language guarantee, not a
convention.

## The guarantee

A transcript is an ordered list of turns; each model call records its request,
the response, and token usage. The recorded totals (N calls, T tokens) are the
replay budget.

- **Faithful replay** lands exactly on the envelope and succeeds with a **zero
  cost delta**.
- **A divergent re-run that issues an extra call** is refused **BEFORE that
  call is issued** (the call ceiling is checked pre-call).
- **A divergent re-run whose turn is more verbose than recorded** trips the
  **token ceiling** the moment that turn is charged, halting before the next
  call.
- A divergent re-run that stays **under** the envelope succeeds with a
  **negative** cost delta.

## How it works

### The budget frame, derived at run time

`@budget(tokens = N, calls = M)` is the attribute form, but `N` and `M` must be
compile-time literals. A replay ceiling has to be derived from the transcript
loaded at run time, so the runner pushes the frame itself with the *identical*
primitive the attribute lowers to:

```
depth = kryos_budget_push(transcript_tokens, transcript_calls)   // entry
...                                                              // replay loop
kryos_budget_pop_to(depth)                                       // every exit
```

(`compiler/crates/kryos-mir/src/lower.rs` injects exactly this pair for the
attribute; `compiler/crates/kryos-rt/src/budget.rs` is the thread-local frame
stack.) The frame's `try_call` reserves one call *before* each turn and refuses
the `(calls + 1)`th; `charge_tokens` charges actual usage after each turn. The
frame is popped on every exit path, including the divergence-throw paths, so the
stack is always balanced. `tests/test_replay.kry::budget_attr_refuses_extra_call`
separately proves the *literal attribute* form spans a loop and refuses the
`(N+1)`th call.

### Mock mode (hermetic, deterministic)

`replay(transcript)` / `replay_run(recorded, run)` return the recorded responses
verbatim under the transcript-derived frame. No network, no clock, no
randomness -- output is byte-identical run to run. Timestamps are never read or
compared.

### Live mode

`replay_live(recorded, cfg)` re-issues each turn against a real model
(`std::llm::chat`) under the same transcript-derived envelope. Honest caveat:
live mode enforces the envelope with explicit pre-call / post-call **counters and
its own throws**, not the thread-local frame, because the current Cranelift JIT
segfaults when a `chat()`-originated budget exception is caught while a manually
pushed frame is active (a JIT exception-handling limitation, not a logic flaw --
mock mode never catches a network-call exception, so it uses the real frame).
The guarantee is identical; only the mechanism differs. See the header of
`src/live.kry`.

## Layout

```
kryos.toml              package + capability manifest (compute, io, ffi, net)
src/transcript.kry      Turn / Transcript model + permissive JSON loader + totals
src/budget_frame.kry    extern hooks for the runtime @budget frame + thin wrappers
src/replay.kry          mock replay, ReplayReport, cost delta (the core)
src/live.kry            live-model replay under the same transcript-derived envelope
tests/test_replay.kry   7 @test functions + a house-style main()
demo_replay.kry         mock replay end to end (faithful / divergent / cheaper)
demo_live.kry           live replay vs a local in-process mock model
fixtures/transcript.json a sample recorded transcript
```

## Transcript format

```json
{
  "model": "claude-sonnet-4-6",
  "turns": [
    { "role": "user",      "request": "What is 23% of 4400?", "response": "", "input_tokens": 0, "output_tokens": 0 },
    { "role": "assistant", "request": "", "response": "I'll use the calculator.", "tool": "calc", "input_tokens": 58, "output_tokens": 14 },
    { "role": "tool",      "request": "calc(0.23*4400)", "response": "1012", "input_tokens": 0, "output_tokens": 0 },
    { "role": "assistant", "request": "", "response": "23% of 4400 is 1012.", "input_tokens": 71, "output_tokens": 11 }
  ]
}
```

A turn counts as a model **call** when it has `role: "assistant"` or any recorded
token usage; `user` / `tool` rows are context and consume no call. Missing
fields default to `""` / `0`. This transcript's envelope is **2 calls / 154
tokens**.

## Run it

```bash
kryos test --path ecosystem/kryos-eval-replay
kryos run  ecosystem/kryos-eval-replay/demo_replay.kry
kryos run  ecosystem/kryos-eval-replay/demo_live.kry
```

`kryos test` (actual output):

```
running 7 @test functions
  PASS  parse_transcript_totals
  PASS  faithful_replay_zero_delta
  PASS  divergent_extra_call_refused_before_call
  PASS  token_divergence_trips_ceiling
  PASS  cheaper_run_negative_delta
  PASS  replay_is_deterministic
  PASS  budget_attr_refuses_extra_call
Tests: 7 passed, 0 failed, 0 skipped, 7 total
```

`demo_replay.kry` (actual output):

```
recorded envelope: 2 call(s) / 154 tokens

[1] faithful replay
    replay[claude-sonnet-4-6]: 2 call(s), 154 tokens; delta vs original = 0 call(s) / 0 tokens
    OK: cost delta is zero vs original

[2] divergent replay (one extra call) under the recorded 2-call budget
    refused: replay diverged: call 3 would exceed the recorded envelope of 2 call(s) -- refused BEFORE the call

[3] cheaper divergent replay (1 call, 25 tokens) under the recorded budget
    replay[claude-sonnet-4-6]: 1 call(s), 25 tokens; delta vs original = -1 call(s) / -129 tokens
    OK: stayed under budget; negative cost delta recorded
```

Both backends agree: `demo_replay.kry` and `demo_live.kry` produce identical
output under `kryos run` (Cranelift JIT) and `kryos build --release` (LLVM AOT).

## MVP scope

- Transcript format: ordered turns with tool calls + recorded responses (JSON).
- `replay` / `replay_run` under a budget derived from the transcript's own
  totals; throws if a divergent run exceeds them.
- Mock-model mode (replay recorded responses) and live-model mode.
- Cost-delta report (replay vs original).
- Tests with a transcript that over-spends on divergence.

## Out of scope (deferred)

- Semantic diffing of outputs.
- Multi-agent transcripts.
- UI.
- Per-token USD pricing (the cost delta tracks calls + tokens; `money_usd` is
  left 0 because rates vary by provider/model).

## Notes / honest risks

- The **token** ceiling is detected *after* a turn's response is charged (it is
  the last axis whose true cost is only known post-response), so a token
  overspend halts the loop *before the next call* rather than before the
  offending call. The **call** ceiling is the pre-call guarantee.
- Live mode uses counters rather than the thread-local frame; see "Live mode"
  above. This is the documented Cranelift exception-handling limitation, not a
  weakening of the envelope.
- `demo_live.kry` binds `127.0.0.1:19237` for an in-process mock server and
  SKIPs cleanly if the port is taken.

## License

Apache-2.0. See [LICENSE](./LICENSE).
