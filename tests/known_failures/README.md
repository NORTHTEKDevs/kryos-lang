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
| `test_repl_jit_missing_rt_symbols.kry` | `kryos test` and `kryos repl` both crash the WHOLE PROCESS (Rust panic, rc=101 -- `can't resolve symbol kryos_calloc` from `cranelift-jit`'s own linker) on a plain struct literal, the most basic possible construct. `kryos check`/`kryos run` accept the byte-identical file and run it correctly. Root cause: both commands build an in-process `cranelift_jit::JITBuilder` via `kryos-codegen-cranelift/src/jit.rs`, which hand-registers a partial allowlist of ~120 `kryos_*` runtime symbols and is missing `kryos_calloc` (every struct/array construction lowers to it) -- `kryos run` never hits this because, per CLAUDE.md, it is "AOT + subprocess, not an in-process JIT" and links the full `kryos_rt.lib` static lib. Directly falsifies CLAUDE.md's claim that `kryos test` "Works with stdlib imports" -- `std::json`, `std::csv`, and `std::string::string_builder` all crash the same way (`std::math`, which allocates nothing, does not), and one variant (a file calling `std::json::stringify`) fails with a distinct JIT verifier error instead of a panic, same underlying gap. Because the crash is process-fatal, ONE struct-constructing test in a file loses every other test's result in that run, not just its own. Found during the toolchain-realworld red-team round. |

FIXED and folded into `tests/harden-probes/probe_wasm_shortcircuit_loop_strcat.kry`
(so `wasm_differential_gate.sh` covers it permanently): `wasm_shortcircuit_loop_strcat.kry`
(LEDGER, wasm-backend wave, 2026-08-27) -- a short-circuit `&&`/`||` if/else
condition INSIDE A LOOP where both branches reassign a `mut str` local by
concatenation made `kryos build --backend wasm` refuse to write a
structurally invalid module ("type mismatch: expected i64 but nothing on
stack"), blocking `examples/showcase/wordscope.kry`'s wasm leg. ROOT CAUSE
was NOT the short-circuit lowering itself: the structured control-flow
translator correctly detects this CFG shape is beyond what it can express
and falls back to the dispatch relooper (`emit_relooper` in
`kryos-codegen-wasm`), which emits an `if pc==i {...}` case for EVERY block
position unconditionally -- including a dead, zero-incoming-edge "drop
locals; return" epilogue block MIR appends after this function's real
`return`, a block only ever reached by the relooper's blanket emission,
never dynamically. `wasmparser` validates every code path statically
regardless of reachability, so a bare `return` (no value) inside a non-void
function failed type-checking even though the path can never execute. Fixed
by pushing a placeholder of the function's declared return type before a
valueless `Return` in `emit_relooper_terminator`
(`compiler/crates/kryos-codegen-wasm/src/lib.rs`), mirroring the identical
fallback `emit_function` already uses for a body that falls off the end.
PROVEN BOTH WAYS: reverting the fix and rebuilding reproduces the exact
original error (`type mismatch: expected i64 but nothing on stack (at
offset 0x70e)`) on the minimal repro; restoring it, both the repro AND
`examples/showcase/wordscope.kry` build clean and produce output
byte-identical to native via `node tools/wasm-host/run.mjs`.
`wasm_differential_gate.sh`: 65/65 compiled programs agree with native (was
62/63 before this fix -- the repro moved from a compile-refusal count into
the compiled-and-agreed count). `tests/run_examples_e2e.sh` layer 1b now
hard-fails (`fail=1`) on a wordscope wasm build failure instead of treating
it as a known, disclosed skip.

BONUS (same fix, same session): this also closed the wasm-contract's own
documented "last remaining gap" -- `tests/harden-probes/probe_23_string_ops.kry`
(complex control flow / irreducible CFG) hit the identical dispatch-relooper
`Return(None)` bug and is now correctly accepted and byte-identical to
native too (confirmed by reverting the fix and reproducing the same error
class at a different byte offset, 0x92c). See `docs/wasm-contract.md`.

FIXED and folded into `tests/no_double_free.sh` (4 new cases):
`rt3_fmt_audit_crash/enum_struct_array_rebuild_double_free.kry` (had no row
here or in `docs/BUGS.md` to begin with -- this session's own wave brief
carried the repro and observed output directly) -- a struct FIELD typed
Enum (or a nested Struct holding one) got no compensating owner when copied
into a rebuilt array element on Cranelift/JIT, in TWO distinct missing-arm
gaps: `RValue::Struct`'s non-`@copy` field-init match (a fresh struct
literal copying a field off a shared array-element alias, e.g. `Task {
priority: t.priority, .. }`) and `emit_struct_deep_copy_inner`'s per-field
match (the `__kryos_struct_index_clone` helper `push(dest, t)` uses to give
a re-pushed whole-struct alias its own independent copy) -- both handled
Array/Str/Map fields but fell through to a bare pointer share for
Struct/Enum, `_ => val` / `_ => field_val`. The source struct's field and
the rebuilt array's field ended up pointing at the SAME enum box with zero
compensating owner; each side's later independent drop then freed it once
-> `KRYOS-FREE-DIAG: double free` (correct printed output up to that
point -- output alone would have missed this). AOT (LLVM) was never
affected (it materializes an enum field as an inline aggregate value, not a
heap alias, per CLAUDE.md gotcha #23/item 15's documented backend
divergence) -- confirmed live, zero diagnostics on the unmodified pre-fix
AOT binary. Fix: added the missing `MirType::Enum` arm to
`emit_struct_deep_copy_inner` (mirrors the existing `MirType::Struct` arm,
recursing into `emit_enum_deep_copy`) and a `MirType::Struct`/`MirType::Enum`
arm to `RValue::Struct`'s non-`@copy` field match (a `kryos_struct_retain`
owner-count bump, matching the existing convention already used for an
Array-of-Struct/Enum ELEMENT a few lines above in the same function).
`compiler/crates/kryos-codegen-cranelift/src/codegen.rs` only. PROVEN BOTH
WAYS: pre-fix binary shows 3x `KRYOS-FREE-DIAG` on the original repro and
3/4 of the new gate cases go RED (`enum_field_struct_rebuild`,
`_two_fields`, `nested_struct_enum_field_rebuild`); post-fix, zero
diagnostics on the repro (both `kryos run` and `kryos build --release`) and
all 4 gate cases GREEN. The 4th case (`enum_field_struct_rebuild_under_spawn`,
added per the wave's own "check under spawn" instruction) was ALREADY clean
pre-fix on 8/8 runs -- spawn's own capture mechanism (a prior, separate
fix, LEDGER "Cranelift shared one box for a loop-local enum captured by
spawn") deep-clones the captured array independently at spawn time and MIR
elides `new_tasks`'s own end-of-main drop once it is moved into the spawn
capture, so this exact shape never exercised two independent drops of the
same box either way -- kept as a non-regression guard, not a proof of this
fix. Original repro file deleted per rule 9 (folded into the gate script
above, not left as a standalone fixture).

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

DETECTED (not eliminated -- same accepted-limitation status as item 9 above)
and folded into `tests/diagnostics_gate.sh` sections 7b/7c: `rt3_fmt_audit_crash/
fmt_launders_asi_trap.kry` (Wave W0001, this session; had no row here or in
docs/BUGS.md to begin with) -- W0001 covered only `||`, so a fresh line
starting with `-` (subtraction) or `[` (index access) right after a newline
still silently continued the PREVIOUS statement's expression with zero
diagnostic (`let a = 5` then `-1` gave `a=4`; `let x = arr` then `[0]` gave
`x=arr[0]`), and `kryos fmt` LAUNDERED all three shapes (including the
already-detected `||` one) into clean, canonical, permanently-merged source
with NO warning at all -- because `format_source` parsed via the
diagnostic-discarding `kryos_parser::parse()`, not `parse_with_diagnostics()`,
so fmt could not have surfaced W0001 even for the shape it already covered.
Fix, same "detect, don't change the grammar" policy as item 9: W0001 now
also fires on the first newline-led `-`/`[`/`(` encountered while building
an expression (an established multi-line chain -- `matrix[0]` then `[1]`,
or an operator-TRAILING `a - / b` (operator ending the line) subtraction chain -- stays silent, same
heuristic as `||`'s `is_digit` case); `kryos fmt` now calls
`kryos_fmt::source_has_ambiguous_continuation` before formatting and REFUSES
(skips, leaves the file untouched) any file carrying a live W0001, the same
policy it already used for an un-anchorable comment. Single `|` remains
deliberately uncovered for the reason above; `*`/`&` share the identical
grammar collision and are also not yet covered (see the W0001 explain
article). Verified false-positive-free by re-running the original item-9
corpus sweep methodology across the full repo (`examples/`, `tests/`,
`compiler/stdlib`, `compiler/self-host`, `stdlib/`, `ecosystem/`, 1106
files): only the known repro fired, on all three shapes, and nothing else.

FIXED and folded into `tests/diagnostics_gate.sh` section 9:
`diag_e0009_misattributed_span_in_loop.kry` -- an unescaped `{` opening a
string interpolation (CLAUDE.md hard rule 4) whose content was not a valid
Kryos expression (e.g. a stray `\` from unescaped embedded-JSON quoting) sent
`kryos check`/`run` to report E0009 "unterminated string literal" on an
unrelated, correctly-closed string statement 6 lines later, instead of the
true bad line. Root cause: the lexer's interpolation-tracking sub-loop
delegated to the general tokenizer with no awareness it was inside `{...}`;
a byte that could not start any valid token (the stray `\`) fell through as
a silent, diagnostic-less `Error` token, and the loop kept consuming tokens
-- including recursing into `scan_string` on the following `"`, which could
swallow real, unrelated source (here: an entire `while` loop body) until it
happened to close on the loop's own `}`, corrupting the token stream. Fix:
the interpolation loop now recognizes an `Error`-kind token immediately and
reports E0009 at that exact byte, with a note pointing at the real cause (a
bare `{` starts interpolation; escape it `{{`/`\{` for a literal brace).

FIXED (already, by a prior commit) and folded into `tests/run_examples_e2e.sh`
layer 1 (`DIFFERENTIAL`) as `examples/showcase/taskstore.kry`:
`rt3_fmt_audit_crash/taskstore_stack_overflow.kry` (2026-08-27 taskstore
wave; had no row here or in `docs/BUGS.md` to begin with) -- a plain
real-world CLI task tracker printed correct output through "--- after
completing #2 ---" and then died with `kryos: stack overflow (unbounded
recursion?)`, rc=253, on `kryos run` (JIT/Cranelift only). ROOT CAUSE:
NOT a new bug -- the SAME corruption the "enum-in-struct array rebuild"
wave above (commit `e4b0d70`) already fixed, just left unverified against
this specific real-program repro (that wave's own closing note explicitly
listed `taskstore_stack_overflow.kry` as "left untouched, out of scope").
`complete_task()`'s rebuild loop (`push(new_tasks, Task { id: t.id, ...,
priority: t.priority })` / `push(new_tasks, t)`) is exactly the
enum-typed-struct-field rebuild pattern that wave's two missing-arm gaps
(`emit_struct_deep_copy_inner`, `RValue::Struct`'s non-`@copy` field
match) double-freed with zero compensating owner. Confirmed same
mechanism directly: `KRYOS_BOX_DIAG=1 KRYOS_FREE_DIAG=1` on the
pre-`e4b0d70` binary shows the double-free/use-after-free diagnostics on
this program too; the STACK OVERFLOW (rather than an immediate crash-at-
free, as `enum_struct_array_rebuild_double_free.kry` showed) is a
downstream symptom of the same corrupted allocator state -- taskstore's
longer run (more tasks, a save/reload round-trip, more struct rebuilds
after the corrupting double-free) gives the corrupted heap more chances to
have a freed block's memory reused and handed back into the recursive
struct/array drop routine, which then recurses on a cycle formed out of
stale/overlapping freed memory instead of a real self-reference. PROVEN
BOTH WAYS, live, fresh (`git revert --no-commit e4b0d70`, full `cargo
build --release` from `compiler/`, rerun, `git revert --abort`, rebuild,
rerun -- per rule 2/3, this crate links into `kryos_rt`/`kryos-cli`):
commit reverted -> `kryos run examples/showcase/taskstore.kry` prints
through "--- after completing #2 ---" then `kryos: stack overflow
(unbounded recursion?)`, rc=253 (exact match to the wave brief's observed
output); commit restored -> rc=0, full correct output on `kryos run` AND
`kryos build --release`, zero `KRYOS_BOX_DIAG`/`KRYOS_FREE_DIAG` lines.
No code change was needed this session -- only proving the connection and
closing the loop `e4b0d70`'s own closing note left open. Folded into
`tests/run_examples_e2e.sh` layer 1's differential (JIT vs AOT,
byte-identical stdout + rc=0 required) as `taskstore`, the observable the
bug actually corrupted (a full-program crash), rather than re-adding a
diagnostic-only `no_double_free.sh` case that would not have caught the
overflow symptom specifically. The file's one hardcoded path
(`scratchpad/rt3/taskstore_data.txt`, specific to a prior session's scratch
directory) was changed to a plain relative `taskstore_data.txt` so the
program is self-contained and safe to run from any cwd, matching every
other showcase program's convention. Wiring it into `strict_caps_examples.sh`
(every showcase program must pass `kryos check --strict-capabilities`) surfaced
one more real gap while doing so, unrelated to the crash: `save_store`/
`load_store` called `file_write`/`file_read` with no `@capabilities` of their
own (fine under the default `inferred` mode, where only `main` needs to
declare and helpers are inferred, but `strict` requires every function to
self-declare) -- fixed by adding `@capabilities(fs:write)` /
`@capabilities(fs:read)` to those two functions, least-privilege per function
rather than reusing `main`'s combined `fs:read, fs:write`.
