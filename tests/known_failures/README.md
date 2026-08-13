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
| `generic_struct_closure_field_passthrough_f64.kry` | A generic struct field typed `fn() -> T` (a closure/fn-value field), returned by a BARE self-field passthrough method (`fn get_closure(self: Holder<T>) -> fn() -> T { return self.f }`), keeps the erased i64-return compiled copy at T=f64: calling the returned closure and formatting with `to_string()` prints the raw i64 bit pattern of the float (`4616752568008179712`) instead of `4.5`. Same class of bug as the CLAUDE.md gotcha #17 fix (`instance_ret_needs_monomorphization` covering Array/Tuple/map bare self-field passthrough) but that fix's type coverage does not extend to a `fn() -> T`-typed field. `T=str` at the identical call shape is unaffected. Reproduces identically on both backends (silent wrong value, not a JIT/AOT divergence). Found during the type-confusion red-team round. |
| `wasm_narrow_int_no_truncation.kry` | The `--backend wasm` codegen never truncates a narrow integer (`u8`/`i8`/`u16`/`i16`/`u32`/`i32`) to its declared width after arithmetic, on PLAIN LOCALS (not just struct fields) -- `let mut v: u8 = 250  v = v + 10` prints `260` on wasm vs. `4` on both native backends (which agree and wrap correctly, per CLAUDE.md gotcha #21). All 5 narrow widths tested diverge from both native backends on every line. `docs/wasm-contract.md` states the wasm backend "never miscompiles silently" and rejects anything outside its subset "at compile time" -- narrow-int arithmetic on locals IS accepted (compiles, runs) so this contradicts that claim; it is a silent wrong-answer miscompile, not a documented gap. Found during the toolchain-realworld red-team round. |
| `test_repl_jit_missing_rt_symbols.kry` | `kryos test` and `kryos repl` both crash the WHOLE PROCESS (Rust panic, rc=101 -- `can't resolve symbol kryos_calloc` from `cranelift-jit`'s own linker) on a plain struct literal, the most basic possible construct. `kryos check`/`kryos run` accept the byte-identical file and run it correctly. Root cause: both commands build an in-process `cranelift_jit::JITBuilder` via `kryos-codegen-cranelift/src/jit.rs`, which hand-registers a partial allowlist of ~120 `kryos_*` runtime symbols and is missing `kryos_calloc` (every struct/array construction lowers to it) -- `kryos run` never hits this because, per CLAUDE.md, it is "AOT + subprocess, not an in-process JIT" and links the full `kryos_rt.lib` static lib. Directly falsifies CLAUDE.md's claim that `kryos test` "Works with stdlib imports" -- `std::json`, `std::csv`, and `std::string::string_builder` all crash the same way (`std::math`, which allocates nothing, does not), and one variant (a file calling `std::json::stringify`) fails with a distinct JIT verifier error instead of a panic, same underlying gap. Because the crash is process-fatal, ONE struct-constructing test in a file loses every other test's result in that run, not just its own. Found during the toolchain-realworld red-team round. |

FIXED and folded into `tests/conformance/conf_spawn_closure_capture_lock.kry`:
`spawn_closure_shared_env_race.kry` (LEDGER item 7b) -- a closure captured
by `spawn` and called from multiple OS threads was a genuine DATA RACE (a
mutated-scalar capture's non-atomic call-then-writeback persistence
mechanism lost updates under contention: 10/10 failures on JIT, 7/10 on AOT
at 50 threads x 2000 calls). Fix: every call to a mutating closure's
underlying function is now serialized under a lock scoped to that
closure's own env allocation (one extra i64 "lock word" slot at the end of
the env, same ARC lifetime as the env itself -- no new allocation), applied
inside the `{name}_env` thunk that every call to a mutating closure value
goes through (the direct-call fast path is unconditionally disabled for
mutating closures already). See `docs/09-concurrency.md`'s spawn section.

DETECTED (not eliminated -- the underlying grammar ambiguity is a documented,
accepted limitation, see CLAUDE.md hard rule 1) and folded into
`tests/diagnostics_gate.sh` section 7: `closure_pipe_continuation_silent_wrong.kry`
(LEDGER item 9) -- a fresh line starting with `||` right after a newline
silently continued the PREVIOUS statement's expression as boolean-or instead
of starting a new statement, with zero diagnostic. The parser genuinely has
no newline awareness, so the merge itself is NOT fixed (still happens,
unchanged, for backward compatibility) -- but it is no longer SILENT: a new
`W0001` warning fires on the dangerous shape (the first `||` encountered
while building an expression, immediately after a newline), while staying
silent on an already-established multi-line `||` chain (`is_digit`-style,
shipped in this repo's own `examples/real/*.kry`) so real code doesn't
false-positive. Deliberately scoped to `||` only, not single `|` -- an
empirical sweep found single-`|` bitwise-or bit-packing (`examples/cdp_bot.kry`,
`examples/websocket_client.kry`) hits the identical "first occurrence,
newline-led" shape as a common, legitimate pattern, so warning there would
false-positive on real shipped code.
