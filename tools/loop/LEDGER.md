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

### 1. Cranelift shares one box for a loop-local aggregate captured by `spawn`
`tests/known_failures/spawn_loop_capture.kry` -- JIT prints `30 30 30 30`, AOT
prints the four distinct values. **Silent wrong answer.** Independently
reproduced. Likely the per-iteration box is hoisted out of the loop in the
capture-boxing path.

### 2. Bootstrap WINDOWS-ONLY: stage-1 exits -1 inside `tokenize` on big files
`PRE-EXISTING`  `CI IS GREEN`  `NOT A BLOCKER`

14/16 locally (`parser.kry`, `lower.kry`). **Linux CI passes all 9 jobs**, so
this does not gate a release -- but a self-host compiler that dies ~50% of the
time on its own source is not something to ship on Windows either.

**Localized** to `tokenize(source)`: the last output is `File: <name>`, and the
next statement in the `obj` path is `tokenize` (main.kry:569).

**Dose-response on input size** (prefixes of parser.kry, 6 runs each) -- the
failure probability scales with input, i.e. with allocation count:

| 30KB | 60KB | 90KB | 109KB |
| --- | --- | --- | --- |
| 0/6 | 1/6 | 3/6 | 3/6 |

The two failing modules are the two LARGEST self-host files (lower 128KB,
parser 109KB); the third largest (types 86KB) passes.

**RULED OUT, each by measurement -- do not re-litigate these:**
- *Rotating-module contention* (the flake in MEASUREMENT TRAPS): it repeats on
  the same pair, and fails with nothing else running.
- *The field-read fix / any recent commit*: stashing it and rebuilding
  reproduces 14/16 exactly.
- *Heap corruption / double-free*: 0 double-frees under `KRYOS_FREE_DIAG`, and
  it still fails under diag (which never deallocates).
- *The size-class pool allocator*: `KRYOS_PLAIN_ALLOC=1` routes every box and
  buffer to the system allocator and crashes at the SAME rate (4/8 vs 4/8).
- *Windows Defender*: no detections in its threat log, and the repo and
  `compiler/target` are both on the exclusion list.
- *An unhandled exception*: the exit code is `0xFFFFFFFF` (-1), NOT a structured
  exception -- no `0xC0000005` access violation, no `0xC00000FD` stack overflow
  -- and **Windows logs no Application Error event at all**. The process is not
  faulting; it is exiting deliberately.
- *A known runtime exit path*: the runtime's deliberate exits are 101 / 98 / 78
  (watchdog) / 77 and the user `exit(code)` builtin. `grep` finds no `exit(-1)`
  in the runtime, the native stdlib, or `self-host/*.kry`.

**So: who calls exit(-1)?** That is the open question, and it is the ONLY
question -- everything above is closed. Next probes worth the tokens:
(1) confirm the death point without trusting redirected stdout, which is
block-buffered and may simply have lost the tail -- the 68-vs-244-byte reading
is NOT reliable evidence on its own; (2) trace `ExitProcess`/`exit` (a WinDbg
`bp kernel32!ExitProcess` on a loop until it fires names the caller
immediately); (3) check whether stage-1 was built by a stage-0 carrying the
same defect, since this binary is self-compiled.

### 3. Struct-argument leak — ~86MB per 1M calls
`tests/mem/struct_arg_leak.kry`. Passing a struct with HEAP FIELDS across any
call boundary leaks its body. **Not** method-specific — a free function leaks
identically (that correction is measured; the older "method receiver" framing
was wrong). Flat for contrast: scalar-only struct through a method, and the
same struct's fields read directly. Needs the receiver/box representation work;
7 incremental attempts have failed, so treat it as a design change.



### 4. `comptime { }` runs at RUNTIME while the docs sell compile-time
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
| **struct `str` field read leaked, 614MB in CI** | `r.name = mk_str(i)` + `len(r.name)` in a loop: 157.7MB/2M before, 3.9MB after; the full CI workload 617.6MB -> 4.5MB. A `str` field read RETAINS and nothing balanced it. Array/map/struct field reads stay borrows -- `push` grows the shared buffer in place, so dropping those temps is the `alloc_node` double-free |
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
- **A leak needs a workload that ALLOCATES A FRESH VALUE each iteration.**
  Re-reading the SAME string 2M times looks perfectly flat even with a fully
  unbalanced retain -- the refcount climbs, nothing allocates. That false
  reading is what retired the field-read drop and cost 614MB in CI for two
  days. Vary the value, and measure the read and the overwrite TOGETHER: read
  alone 4.3MB, store alone 4.4MB, store+read 157.7MB. Either half alone
  says "no leak".
- **`kryos_string_clone` is not a deep copy.** It is a refcount bump returning
  the same pointer, identical to `kryos_string_retain`.
