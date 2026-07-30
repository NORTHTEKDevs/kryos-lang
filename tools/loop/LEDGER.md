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

### 1. Tuple-destructured struct aliasing: leak vs double-free  `BLOCKS CI`

**Where it stands:** `selfhost-stage1` is GREEN for the first time and the full
`stage1_mini_parser.kry` prints `stage 1 mini-parser: ok` with 0 double-frees.
`tests/acceptance.sh` PASSES all four conditions. **But Linux CI now fails the
memory-plateau gate** -- "churn workload exceeded 250MB; a memory leak was
reintroduced". 8 of 9 CI jobs green (Windows, macOS, docs, fuzz, quickstart,
registry, wasm, selfhost-stage1).

**The trade currently in the tree (82a4f72):** a struct destructured out of a
tuple (`let (p3, thn) = f()`) is marked BORROWED, so it is not dropped. That
removes the double-free but leaks the box. Corruption traded for a leak --
better by the ranking at the top of this file, but the leak is too large to
ship.

**Why the double-free happens:** `let (p3, thn) = f()` lowers to a Field read of
the tuple into a NAMED local, so `p3` aliases the tuple's storage. Then
`parse_stmt` does `let mut pp = p3` -- a SECOND alias, also named and also live.
Both get scope-end Drops, so one box is freed twice.

**Ruled out** (each implemented, built, run, reverted):
- marking the destructured element borrowed -- fixes corruption, LEAKS past the
  250MB ceiling (this is what is committed; it is a stopgap, not the answer)
- marking the SOURCE moved on `let pp = p3` (mirroring what the reassignment
  path already does) -- mini-parser returns to garbage token kinds, so the
  source is still read afterwards on some path
- eight hand-built models of the suspected shapes: all CLEAN while the real
  program failed. Stop modelling this one; reduce the real file instead, which
  is what actually worked.

**Next step:** exactly ONE owner among {tuple temp, destructured element, and
any later alias}. The tuple temp is unnamed and never dropped today, so the
cleanest shape is probably: element OWNS, and any subsequent
`let mut pp = <struct local>` aliases must not double-drop -- i.e. fix the
alias-of-an-alias case rather than the destructure itself.

**Do not** revert 82a4f72 to make mem-plateau green: that restores a
use-after-free, which ranks worse than a leak. Fix the ownership properly.

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

## CAPABILITY SURFACE AUDIT (2026-07-29)

Enumerated all 157 builtins the LLVM codegen maps against the 82 the capability
model gates, filtered to authority-bearing names, and probed each survivor.

- **Raw memory — REAL ESCAPE, fixed.** See the closed table.
- `file_append` — looked ungated to a first-pass grep; it is gated, just in a
  different match arm than `append_file`. No gap.
- `buf_get_byte` / `buf_set_byte` — probed for out-of-bounds access. **Safe:**
  the runtime bounds-checks and returns `-1`. My first probe appeared to show a
  4096-byte over-read only because it counted the `-1` sentinel as data. Verify
  the VALUES, not just that something came back.
- `buf_write_*` — write into an owned buffer, no external authority.
- `read_line`, `time_now_*` — input and clock reads; ambient by design, matching
  the documented model.

**Minor wart, not security:** `buf_get_byte` returns `-1` out of range while
array/string indexing PANICS. Same undocumented-sentinel class as `pop([])`
returning `0`. Inconsistent, and `-1` is a plausible real byte value in signed
contexts. Worth unifying.

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
