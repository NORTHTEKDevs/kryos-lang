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
| `closure_pipe_continuation_silent_wrong.kry` | The parser has no newline-awareness at all; a fresh statement starting with `\|\|` (the closure literal opener, which doubles as boolean-or) silently merges into the PREVIOUS statement's trailing expression when types allow, with zero diagnostic. `let c: bool = a` followed by `\|\| b` on the next line parses as `let c: bool = (a \|\| b)`, printing `true` instead of the intended `false`. Also the true root cause of what CLAUDE.md used to (mis)describe as "a closure that is the tail value of a block cannot capture that block's earlier let bindings" -- see CLAUDE.md gotcha #1/#11. Found during the closures/fn-values/captures hardening wave. |
| `closure_mutated_capture_scalar_gaps.kry` | Mutated-SCALAR-capture persistence (unlike the already-generalized struct case) only covers exactly one mutated capture whose body's tail value IS that identifier. Two mutated scalars, one scalar + one mutated struct, and a solitary mutated scalar whose closure returns something OTHER than that identifier (a common "stateful factory" idiom) all silently lose persistence -- frozen at their first-call value on every later call, no diagnostic, identical on both backends. Found during the closures/fn-values/captures hardening wave. |
| `closure_curried_generic_aot_crash.kry` | A curried (2-level) generic closure return -- `fn f<T>(a: T) -> fn(T) -> fn(T) -> T` -- fails to build on AOT/LLVM (`load %T, ptr %_1_arg`, an unresolved generic type parameter reaching IR emission) while `kryos run`/JIT runs it correctly. `pending_lambda_ret_hint` only stages a concrete signature for the DIRECTLY-returned lambda, not recursively for an inner lambda returned by that lambda. Found during the closures/fn-values/captures hardening wave. |
| `spawn_closure_shared_env_race.kry` | A closure captured by `spawn` and called from multiple OS threads is a genuine DATA RACE: `Instruction::Spawn`'s Function/Shared arm `kryos_arc_retain`s the closure's env box instead of snapshotting it (every other heap capture kind -- str/array/map/struct/enum -- deep-copies), so concurrent callers share one env allocation; a mutated-scalar capture's non-atomic call-then-writeback persistence mechanism (gotcha #11) then loses updates under contention. 10/10 failures on JIT, 7/10 on AOT at 50 threads x 2000 calls. Found during the spawn/actors/channels/sync hardening wave. |
| `lowercase_struct_literal_parse_fail.kry` | A struct whose name starts with a LOWERCASE letter cannot be constructed via struct-literal syntax (`Name { field: value }`) at all -- misdiagnosed as two `undefined variable` errors pointing at the struct/field names instead of a real parse error. `struct Counter { .. }` (capitalized) with the identical body works. Not documented as a hard rule anywhere. Found during the modules/imports/namespace-resolution hardening wave (parser/grammar bug, out of that wave's scope). |
