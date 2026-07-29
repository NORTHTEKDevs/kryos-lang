# Kryos production ledger

The queue survives context loss; a session does not. Update this file in the
SAME commit as the work. Anything not written here is lost.

Ranked by SERIOUSNESS FOR THE INTENDED USE CASE, not by which gate is red.
Kryos is deployed as capability-attenuated infrastructure for agent tooling,
so: (breaks the capability/trust model) > (silent wrong answer) > (blocks a
green CI) > (leak) > (papercut). A silent wrong answer outranks a crash — a
crash announces itself. A trust-model hole outranks both: nothing above it in
the stack can be sound if the boundary leaks.

---

## THE LOOP

```
preflight -> select -> REPRODUCE -> bisect -> fix -> prove -> gate -> push -> ledger
```

Run `tools/loop/kryos-loop.sh preflight` first, every time. Then:

1. **SELECT** the top unblocked item below.
2. **REPRODUCE** before forming any hypothesis. `kryos-loop.sh repro <file>`.
   *No theory is allowed before a reproduction exists.* Three attributions were
   wrong this way in one session — the `@copy` arm, a "merge interaction", and
   the `param_src` branch — each from reading code instead of measuring it.
3. **BISECT MECHANICALLY.** Commit-level (`git cherry-pick` onto a worktree) or
   program reduction (cut the input until the symptom vanishes). Never
   hand-edit a hypothesis in and call the result evidence: one such edit
   silently removed more than intended and produced a confident wrong answer.
4. **FIX.**
5. **PROVE BOTH WAYS.** The test must FAIL without the fix. A gate that cannot
   fail is not a gate. Verified in both directions or it does not count.
6. **GATE** with `kryos-loop.sh gates 3`.
7. **PUSH IMMEDIATELY.** 12 commits sat unpushed once and a second agent
   independently reimplemented one of the fixes.
8. **LEDGER** — record the outcome *and everything ruled out*.

### Non-negotiables

- A self-reported "fixed" is not evidence. Only fresh command output is.
- Every gate can be green while data is silently wrong — that exact thing
  happened here (an `@copy` corruption passed conformance, no_double_free,
  bootstrap, examples, strict-caps, the e2e gate AND the IR gate). When a fix
  touches ownership, add a **value assertion**, not just a crash check.
- Measure leaks at two scales. One number proves nothing.
- Backends agreeing means the defect is in MIR; diverging means read the IR.

---

## OPEN — ranked

### 1. Struct assigned from a local doesn't retain its heap fields — CRANELIFT ONLY  `BLOCKS CI`
`tests/known_failures/struct_in_tuple_return.kry`.

**Narrowed by the loop's own `repro` on its first run, and it changes the next
step:** the backends now DIVERGE. AOT/LLVM prints `out=3` — **correct** — and
JIT/Cranelift prints `out=1`. Both printed `out=0` before the partial fix, so
that MIR guard fully fixed LLVM and only half-fixed Cranelift. The remaining
defect is therefore in the **Cranelift backend**, not in shared MIR lowering.
1 double-free left (`array DOUBLE-FREE rc=0 len=0 cap=4`), down from 2.

This is exactly why `repro` runs both backends before anything else: the
previous ledger entry pointed at MIR ownership, which is now the wrong place
to look.

Traced exactly: the callee is correct and the tuple carries the value out
intact; the loss is at `p = p2`. A struct assigned from another local aliases
that local's heap fields and takes no reference — `retain_for_ty` is `None` for
`Struct` and `emit_param_source_retain` only fires for params. `p2` is a
tuple-destructured *named* local, so its scope-end Drop frees the array `p` now
points at.

**Ruled out** (each tested, do not retry):
- the guarded struct reassignment release (`release_struct_heap_fields_if_ne`) —
  disabling it entirely changes nothing
- adding the retain to the `param_src` branch of the reassignment path — no
  effect; that branch is not the one taken for `p = p2`
- **deep-copying a NON-`@copy` struct at the Cranelift assignment site**
  (dropping the `copy_structs.contains(sname)` gate at codegen.rs ~3851).
  Double-frees went 1 -> **0**, but the value got WORSE: `out` 1 -> 0. So the
  box aliasing is real and this addresses it, but a struct deep copy calls
  `kryos_array_clone` on the array field, which is a REFCOUNT BUMP rather than
  a copy — the "independent" block still shares the buffer, and the accumulated
  elements are still lost. Any fix here has to give the destination a genuinely
  independent array, or leave the box shared and fix the Drop instead.

**Confirmed mechanism:** at codegen.rs ~3851 Cranelift deep-copies a struct on
assignment ONLY when it is `@copy`; a non-`@copy` struct is a bare pointer
alias. `p` and `p2` therefore name ONE malloc'd block, and `p2`'s scope-end
Drop frees it under `p`. That is the documented "JIT aliases, AOT copies"
divergence (gotcha #23) turning into data loss.

**Next step:** the two obvious repairs are in tension — copying the box loses
the accumulated array (see the ruled-out entry), and leaving it shared keeps
the premature Drop. So attack the DROP instead: `p2` should not free a box that
`p` now names. Look at whether the assignment can mark the source consumed
(`dropped_locals`) for the struct case, which is what suppresses a scope-end
Drop elsewhere in the lowering.

**Acceptance:** repro prints `out=3` on both backends with 0 double-frees, then
`compiler/self-host/stage1_mini_parser.kry` reaches `rc=0`, then `selfhost-stage1`
goes green — the last red CI job.

### 2. Cranelift shares one box for a loop-local aggregate captured by `spawn`
`tests/known_failures/spawn_loop_capture.kry` — JIT prints `30 30 30 30`, AOT
prints the four distinct values. **Silent wrong answer.** Independently
reproduced. Likely the per-iteration box is hoisted out of the loop in the
capture-boxing path.

### 3. Struct-argument leak — ~86MB per 1M calls
`tests/mem/struct_arg_leak.kry`. Passing a struct with HEAP FIELDS across any
call boundary leaks its body. **Not** method-specific — a free function leaks
identically (that correction is measured; the older "method receiver" framing
was wrong). Flat for contrast: scalar-only struct through a method, and the
same struct's fields read directly. Needs the receiver/box representation work;
7 incremental attempts have failed, so treat it as a design change.



### 5. `comptime { }` runs at RUNTIME while the docs sell compile-time
Fix the docs (hours). Real compile-time evaluation is months and should not
gate 1.0.

### 5. `[dyn Handler]` reports a confusing `E0100` instead of `E0110`
Array-literal element unification ignores the annotated `dyn` element type.

### 6. Docs status sections drift because they are hand-maintained
`docs/BUGS.md` said "none currently tracked" while two tests deadlocked.
Generate from real test output.

---

## CLOSED — with the evidence that closed it

| Item | Evidence |
| --- | --- |
| sret fn-value ABI | struct through a fn value returned garbage on `--release`; fixed by consulting `func_sig_aggs`. Guessing from the LLVM type string broke `Result.and_then` — enums are aggregate-shaped but returned directly |
| `bool` -> builtin ABI | `json_bool(false)` built JSON `true`; `i1` against an `(i64)` declaration left the upper 63 bits undefined |
| narrow-int args | same class for `char_from`/`char_at`; latent only because 32-bit x86-64 ops zero the upper half |
| read-only builtin args leaked | 15 builtins missing from the borrow allowlist; the consume mark is path-insensitive so one `to_upper(s)` suppressed the drop on every path. 78MB/800k -> flat |
| network `KryosString` allocator | six sites freed under a layout they were never allocated with; 360k TCP round trips 18.7MB -> 0.1MB |
| computed string -> user fn leaked | 35.4MB/400k -> 4.0MB; needed the `@copy` str-field copy as prerequisite |
| `-g` emitted an undefined string global | `kryos build -g` was broken on every platform |
| spawn wrapper `byval` ABI | System V only; closed both concurrency blockers |
| **raw-memory capability escape** | a zero-capability program read `TOPSECRET-APIKEY` via `str_to_ptr`+`ptr_byte_at` and dereferenced +4096 without faulting. Closed with a trusted-computing-base split: raw memory requires `ffi` at DIRECT USE in user code and the requirement does not propagate, so the stdlib (which is built on these — `alloc` in 14 modules) stays usable. Guarded by `tests/security_gate.sh`, which asserts BOTH directions plus no-cascade |

---

## MEASUREMENT TRAPS (each cost real time)

- **`cargo build -p kryos-cli` leaves the staticlibs stale.** Runtime edits are
  invisible to AOT programs until a full `cargo build --release`. This produced
  a wrong "no effect" reading and a wrongly "ruled out" theory. `preflight`
  checks it.
- **Bootstrap fails spuriously (rc=127, rotating modules) under load.** Run it
  alone; only a solo failure is real.
- **A control that changes the workload proves nothing.** A server that accepts
  and sends without READING looked perfectly flat — 3965 of 4000 requests were
  failing with RST.
- **`KRYOS_FREE_DIAG=1` completing while the program normally crashes means the
  crash IS corruption.** Master's `parse_int: invalid numeric input: '}'` was
  memory corruption, not a parse bug.
- **`kryos_string_clone` is not a deep copy.** It is a refcount bump returning
  the same pointer, identical to `kryos_string_retain`.
