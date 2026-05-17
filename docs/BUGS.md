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

(none currently tracked)
