# Known Bugs

## Resolved

### String-in-struct ownership leak across function returns (v0.4)

**Status**: Fixed. Tracked by regression tests in
`compiler/crates/kryos-test-runner/tests/e2e/ownership/struct_with_strings_return_run.kry`
and `struct_with_strings_stress_run.kry` (1000-iter loop).
`examples/showcase/agent_runtime.kry` was rewritten to return the planner's
`Action` struct directly (with str fields) instead of using `[str]`
out-parameter slots; both JIT and `kryos build --release` are verified.

Original symptom (v0.4-era): a function returning a struct whose `str`
field was assigned from a previously-moved local would surface garbage
values, e.g. `len(action.arg_s) = 7305790164731371552` and empty string
content.

Root cause: partial-move tracking for struct field reads
(`partial_moved_locals` in `kryos-mir::lower`) did not extend to the case
where a non-copy struct local was moved wholesale into a return position
after a field had separately been moved out via field access. The current
MIR lowering tracks partial moves explicitly and the ownership analyzer
emits the correct drop / no-drop combination at scope exit.

If a new reproducer ever surfaces, please attach it to a fresh entry below
under "Active".

## Active

### Concurrency + struct receivers: two conformance tests hang on `build --release`

**Status**: Open. Reproduced on `master` at `ac45392c` (Ubuntu 25.10,
glibc 2.43, clang 21) — not introduced by a recent change.

Two of the 41 conformance programs fail on the LLVM/AOT backend only; both
build cleanly, run past their first output, and then never terminate (killed
at a 120s timeout). `kryos run`/Cranelift passes both.

- `tests/conformance/conf_spinlock_mutex.kry` — repeatedly prints
  `kryos: uncaught exception in spawned thread: sync error: lock on dropped
  mutex`, then hangs. The mutex box is freed while spawned threads still hold
  a reference to it: `fn lock(self: SpinLock) -> SpinLock { .. return self }`
  makes `let l = lock.lock()` a second owner of a struct whose receiver
  ownership is not tracked. This is the same struct-representation issue
  documented at length in `CLAUDE.md` gotcha #22 (the struct-method receiver
  leak) — the leak is what keeps struct sharing safe today, and closing it
  needs a representation/ABI change, not an incremental patch.

- `tests/conformance/conf_errors_concurrency.kry` — prints
  `[actor error] Adder.add: uncaught exception: negative` and then hangs. An
  exception thrown inside an actor message handler is reported but the actor's
  mailbox loop never unwinds or shuts down, so the program's join never
  completes. Actor-handler exception propagation appears to have no termination
  path.

Both are true deadlocks, not slow runs: `conf_spinlock_mutex` stops producing
output after 8 lines and was still alive after 8 minutes.

Both are blocking for a production release: the first is a use-after-free
reachable from ordinary `Mutex` use, and the second means an unhandled
exception in an actor deadlocks the process rather than failing it.

**Secondary issue, same root:** `tests/conformance/run_conformance.sh` runs each
program with NO timeout, so these two do not make the suite fail -- they make it
HANG. A CI job invoking the runner blocks until the platform's step limit rather
than reporting a failure. The runner should wrap each backend invocation in a
`timeout` so a deadlock is reported as a failure.

Everything else in the suite passes on both backends (39/41).
