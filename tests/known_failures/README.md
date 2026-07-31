# Known failures

Minimal repros for bugs that are currently OPEN. Each is described in
[../../docs/BUGS.md](../../docs/BUGS.md).

These files are deliberately **outside** the `tests/conformance/conf_*.kry`
glob, so `run_conformance.sh` stays a real signal rather than a suite with
expected-red entries in it. Run them by hand:

    kryos run tests/known_failures/NAME.kry                                  # Cranelift
    kryos build tests/known_failures/NAME.kry --release --backend llvm -o /tmp/x && /tmp/x

When one is fixed, fold it into the conformance test it belongs to and delete
the file here.

| File | Bug |
|---|---|
| `global_array_reassign_corrupt.kry` | A top-level `let mut [T]` global reassigned (to a fresh value) by one function and then read/pushed by a DIFFERENT function reads back an all-zero array header (`corrupt array header ... cap=0, data=0x0`) on LLVM AOT. Works fine when only ever mutated via `push` from a single call chain rooted at the global's own initializer. Expected `len=3`; panics instead. Found while root-causing ledger item 2 (bootstrap tokenize -1 crash). |
| `assert_shadow_uncatchable.kry` | `std::test::assert`'s 2-arg form is permanently shadowed by the compiler's own hardcoded `assert` intrinsic (`process::abort()`-based) -- the stdlib body (a real `throw`, meant to be catchable) never runs. `try { assert(false, "boom") } catch (e) { .. }` never reaches `catch`; the process just aborts. Found while building `examples/showcase/secret_agent.kry` (ledger item 2c). |
