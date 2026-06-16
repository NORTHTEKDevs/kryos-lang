# kryos-pipeline-supervisor

Budget-bounded, one-for-one restart supervision for actor stages.

OTP (Erlang/Elixir) supervises crashed workers and restarts them under a
*max-restart-intensity* ceiling. Kryos cannot out-Erlang Erlang on distribution
or hot-reload (it has neither). The narrow win it *can* take: make the restart
ceiling a **language budget frame** instead of a hand-rolled counter. A
crash-looping stage provably cannot exceed its restart budget the same way a
runaway agent cannot exceed its token budget -- it is the same `@budget` frame
primitive, on its `calls` axis.

```
kryos test --path ecosystem/kryos-pipeline-supervisor   # 11 @test gates, green
kryos run  ecosystem/kryos-pipeline-supervisor/demo_supervisor.kry
kryos run  ecosystem/kryos-pipeline-supervisor/e2e_spawn.kry        # real threads
```

## The two things OTP does not have

1. **The restart ceiling is a real `@budget(calls = N)` frame.** A supervisor
   opens a restart window with `budget_open(N)` -- the SAME thread-local frame the
   `@budget(calls = N)` attribute injects (`kryos-rt/src/budget.rs`). Every restart
   is reserved through `reserve_restart()`, which calls the runtime's
   `kryos_budget_try_call`: 0 (and decrements every active frame) while restarts
   remain, 1 once the frame is exhausted. The supervisor never maintains its own
   ceiling counter; it asks the frame. The `@budget`-attribute form
   (`reserve_restart_or_throw`) makes the (N+1)th reservation *throw* -- a language
   property, not a check the loop could forget. Both are tested.

2. **The supervisor holds zero ambient authority.** `kryos manifest --caps
   src/supervisor.kry` reports `capabilities: []` for every function -- the restart
   engine is pure compute. The supervised stage's authority is whatever the stage
   itself declares; supervising it adds none.

## How abnormal exit is observed (the honest part)

kryos-rt has **no actor link/monitor primitive**, and a Kryos `throw` that escapes
a spawned thread aborts the whole process -- a stage cannot panic-and-be-caught
across the thread boundary. Per the spec's pre-build verification, a supervised
stage therefore signals an abnormal exit **in band**: its outcome is a value, and
the reserved marker `fail_sentinel()` (`i64::MIN`) means "this stage exited
abnormally."

- In the deterministic core, a unit is a `fn(attempt: i64) -> i64` that returns
  `fail_sentinel()` to signal a crash. This is what the `@test` gates exercise
  (the `@test` JIT harness miscompiles `spawn {}`, so the gates are spawn-free).
- In `e2e_spawn.kry`, a real spawned worker sends `fail_sentinel()` on a dedicated
  **status channel** and returns; the driver-thread supervisor `recv`s that status
  -- the observable exit -- and restarts on FAIL. Proven on both backends
  (`kryos run` and `kryos build --release`).

No payload may equal `fail_sentinel()`.

## API

```
// --- restart.kry : the budget-frame primitive + the abnormal-exit signal ---
fn fail_sentinel() -> i64                  // the reserved "crashed" marker (i64::MIN)
fn is_fail(v: i64) -> bool
fn budget_open(max_restarts: i64) -> i64   // push a restart window; returns frame depth
fn budget_close(depth: i64)                // pop the window (self-healing)
fn reserve_restart() -> bool               // true if a restart is permitted; false if exhausted
fn reserve_restart_or_throw()              // hard-ceiling form: throws "@budget" on exhaustion
fn restarts_remaining() -> i64             // restarts left in the active window (-1 if none)

// --- supervisor.kry : one-for-one bounded restart ---
struct SupervisorReport {
    stage: str, succeeded: bool, gave_up: bool,
    attempts: i64, restarts: i64, result: i64,
    message: str, ledger: [LedgerEntry]
}
fn supervise(name: str, task: fn(i64) -> i64, max_restarts: i64) -> SupervisorReport
fn report_ok(r: SupervisorReport) -> bool
```

`supervise` runs the unit; on each FAIL it reserves a restart from the budget
frame. If a restart is permitted it is recorded to a hash-chained **cost ledger**
(`kryos-cost-ledger`, project 28) and the unit is re-run with the next attempt
number; if the frame is exhausted the supervisor gives up with a clear message --
it does not spin. The first invocation is the initial run and spends no restart
budget. One-for-one only (restart the failed unit); trees / all-for-one /
rest-for-one / hang detection are out of scope.

`report.ledger` is the tamper-evident restart audit trail: one entry per restart,
each `ComputeCost{api_calls = 1, tokens_used = <failed attempt>}`. Verify it with
`ledger_verify(report.ledger)` (`-1` == clean).

## Composes with

- **kryos-actor-pipeline (25)** -- the `Stage` actor abstraction this supervises
  (spawned-thread worker drained by channels). The supervised contract adds a
  status channel because the base `Stage` has no failure-signalling surface.
- **kryos-cost-ledger (28)** -- the hash-chained ledger each restart is recorded to.

Both are path-dependencies in `kryos.toml`.

## Done-criteria evidence

`kryos test --path ecosystem/kryos-pipeline-supervisor`:

```
running 11 @test functions
  PASS test_attribute_budget_frame_spans_restart_loop   # (N+1)th restart throws @budget
  PASS test_attribute_budget_allows_exactly_n
  PASS test_attribute_budget_resets_per_call
  PASS test_transient_recovers                          # fails twice / 3-budget -> recovers
  PASS test_permanent_trips_budget                      # always fails -> gives up at ceiling
  PASS test_recovers_at_budget_edge                     # uses the full budget, recovers
  PASS test_one_over_budget_gives_up                    # one past the budget -> give up
  PASS test_zero_budget_is_one_shot
  PASS test_healthy_stage_no_restart
  PASS test_ledger_records_each_restart                 # audit chain verifies clean
  PASS test_windows_do_not_leak
Tests: 11 passed, 0 failed, 0 skipped, 11 total
```

`kryos run e2e_spawn.kry` (real OS threads + channels):

```
supervising spawned stage 'transient' (restart budget = 3)
  attempt 1: stage CRASHED (FAIL signalled on status channel)
    -> restart 1 authorized by budget frame; recorded to ledger
  attempt 2: stage CRASHED (FAIL signalled on status channel)
    -> restart 2 authorized by budget frame; recorded to ledger
  attempt 3: stage exited NORMALLY
    results: [1, 4, 9, 16]
  RESULT: 'transient' recovered after 2 restart(s)
supervising spawned stage 'permanent' (restart budget = 3)
  ... 3 restarts, then: restart budget EXHAUSTED -- supervisor gives up (no spin)
  RESULT: 'permanent' gave up (gave_up=true)
=== e2e PASS ===
```

## Honest limits

- **Abnormal exit is in-band, not a monitor.** A stage that genuinely panics
  (`throw` escaping its thread) aborts the process; there is nothing to supervise.
  Supervision works only for stages that *report* failure on their result/status
  channel. This is the spec-sanctioned downgrade given kryos-rt has no link/monitor.
- **One-for-one only.** No supervision trees, no rest-for-one/all-for-one, no
  distribution, no hot reload, no hang detection.
- **The `@test` gates are spawn-free** (the JIT test harness miscompiles `spawn {}`);
  the threaded proof is `e2e_spawn.kry` under `kryos run`/`kryos build --release`.
- The cost ledger hash is a pure-Kryos polynomial hash (tamper-EVIDENT, not
  cryptographic) -- inherited from kryos-cost-ledger; see its README.
