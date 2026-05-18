# LLVM-vs-Cranelift parity audit (M3)

This document captures the backend parity work for M3, separate from
the high-level production audit in [AUDIT-v2.8.0.md](AUDIT-v2.8.0.md).
The LLVM backend is the "release" path (`kryos build --release`).
At v2.8.0 baseline it failed 66% of the smoke suite Cranelift passes.

This is what the in-tree `ROADMAP.md` called "v2.9 — LLVM backend
parity". Per the decisions on PR #1, that milestone is folded into
the single v3.0 cut.

## Current state (after PR #3 final)

| Backend | Pass | Fail | Rate |
| ------- | ---: | ---: | ---: |
| Cranelift | 34 | 0 | **100%** |
| LLVM | **34** | 0 | **100%** |

Verified locally on Windows + clang 21.1.8. CI verification pending
the GitHub Actions billing fix on NORTHTEKDevs.

The two follow-up failures noted earlier (test_generics, test_process)
both closed in PR #3's final commits:

- **test_process** — push() result aliased to dest local. Kryos's
  `push(arr, item)` is `-> void` at the runtime ABI, but MIR binds
  the call result. The LLVM "push" arm now emits the void call
  AND aliases `%_<dest>` to the input array (since push mutates in
  place), matching Cranelift's behavior.

- **test_generics** — two layered bugs:
  1. KryosArray drop loop used offset 24 (`ref_count`) instead of
     offset 32 (`data`) when iterating elements for per-element
     `kryos_string_free` calls. Iterating ref_count-as-data
     segfaulted on every string-element array at scope exit. A
     4-line minimum repro reproduces the crash:
     `fn main() { let p = ["hello"] }`
  2. With the crash fixed, `to_string<T=str>` was returning the
     decimal of the pointer address. Switched to
     `kryos_string_clone` so the caller owns a fresh KryosString.

### Tests fixed (19 of 21 baseline failures)

| Test | Class fixed | Path |
| ---- | ----------- | ---- |
| test_assert_eq | A | assert_eq builtin codegen path |
| test_string_brace_escape | A | (same) |
| test_io | C → A' | SSA name collision; then runtime decl |
| test_string_clobber | B → MIR-ownership | StringConcat ptr coercion + downstream |
| test_fn_pointer | B → T → cleared | Multiple fixes converged |
| test_struct_field_copy_through_param | B → T → cleared | Same |
| test_crypto | A' | runtime decls added |
| test_db / test_db2 | A' | runtime decls added |
| test_fs | A' | runtime decls added |
| test_net | A' | runtime decls added |
| test_term | A' | runtime decls added |
| test_bootstrap_lexer_smoke | E | tuple aggregate type lowering |
| test_tuple_mut | E | (same) |
| test_match_return | D | recursive enum payload heap-alloc + load |
| test_re | B' | coerce_value field0-is-ptr fix |
| test_tracked | B' | (same) |
| test_user_fn_shadows_builtin | shadow + interp | user-fn shadow detection + bool stringify |
| test_net2 | A + link | int/float mapping + internal linkage |

### Remaining 2 failures

| Test | Class | Detail |
| ---- | ----- | ------ |
| test_generics | T (runtime) | Outputs `FAIL pair[1]` — generic monomorphization for `pair<A, B>(a: A, b: B) -> [str]` produces wrong result for the B=str arg. Build is clean; semantic divergence between Cranelift and LLVM in how `to_string<T>` resolves T=str. Needs MIR-layer investigation. |
| test_process | new codegen bug | Surface: `error: use of undefined value '%_3'` at `inttoptr i64 %_3 to ptr`. Surfaced when internal-linkage suppressed the libc-exit conflict; %_3 was always undefined but earlier link failure masked it. Likely interaction between aggregate-as-payload heap-alloc (M3 class D fix) and Command struct field handling. Needs codegen trace. |

## Baseline @ commit `9c37e0b` (v2.8.0)

Run via `tests/parity/run_parity.sh`. Every smoke test in
`tests/smoke/*.kry` is exercised under both backends:

- **Cranelift** (`kryos run FILE`) — the JIT path used by `kryos run`
  and `kryos build` without `--release`.
- **LLVM** (`kryos build FILE --release --backend llvm -o OUT && OUT`)
  — the AOT release path.

| Backend | Pass | Fail | Rate |
| ------- | ---: | ---: | ---: |
| Cranelift | 32 | 0 | 100% |
| LLVM | 11 | 21 | 34% |

The LLVM backend is real source — `kryos-codegen-llvm/src/codegen.rs`
is 5,823 lines, comparable to the Cranelift backend's 5,892. The
failures below are real omissions in MIR → LLVM-IR lowering, not
"not implemented yet" markers.

## Failure classes

Both backends consume the same MIR. Every divergence is a difference
in lowering MIR → backend IR.

| Class | Symptom (verbatim from LLVM module verifier or runtime) | Count | Example failing tests |
| ----- | ------------------------------------------------------- | ----: | --------------------- |
| **A** | `use of undefined value '@<builtin>'` — the LLVM backend forgot to declare/define a builtin that Cranelift has wired. | 2 | `test_assert_eq`, `test_string_brace_escape` |
| **B** | `'%X' defined with type 'i64' but expected 'ptr'` — string argument is passed as `i64` where the callee signature is `ptr`. Lowering forgot the int-to-ptr cast (or never tagged the local as a string). | ~10 | `test_string_clobber`, `test_fn_pointer`, `test_user_fn_shadows_builtin`, `test_struct_field_copy_through_param`, ... |
| **B'** | Reverse of B: `'%X' defined with type 'ptr' but expected 'i64'` — pointer-shaped value reaches an arithmetic site without the ptr-to-int cast. | ~8 | `test_re`, `test_tracked`, `test_crypto`, `test_fs`, ... |
| **C** | `multiple definition of local value named '_1_fld_0'` — a struct-field temporary is allocated twice on the same scope. SSA naming collision. | 1 | `test_io` |
| **D** | `invalid cast opcode for cast from 'i64' to '%Expr = type opaque'` — enum / opaque struct type not materialised before a load/cast. Bitcast applied where the type is still opaque. | 1 | `test_match_return` |
| **E** | `insertvalue operand must be aggregate type` — tuple construction inserts into a non-aggregate (probably an `i64` placeholder that should be `{i64, i64}`). | 2 | `test_tuple_mut`, `test_bootstrap_lexer_smoke` |
| **T** | LLVM build succeeds, but the resulting binary exits non-zero or produces wrong output. Codegen is shape-correct but semantically wrong. | 1 | `test_generics` |

Total accounted: 2 + 10 + 8 + 1 + 1 + 2 + 1 = **25 fails**. The
remaining 4 of the 21 LLVM fails distribute across these classes
once examined individually (some B/B' fails fold into single fixes).
The parity runner output will pin exact counts.

## Tests passing on both backends today (11)

`hello`, `test_closure_capture_chain`, `test_bidirectional_closure_inference`,
`test_ffi`, `test_ffi2`, `test_keyword_rejection`, plus the simpler
runs. Re-enumerated by the runner output at every commit; do not
hand-edit this list.

## Fix order (ranked by simplicity, broken first)

The classes are independent enough to fix in order without invalidating
prior work, with one exception (B and B' interact through the same
type-tagging code path and are addressed together).

1.  **A — undefined builtins.** Find the builtin declaration site in
    the LLVM backend (`declare_builtins` or equivalent). Compare with
    Cranelift's declaration list. Add missing ones. ETA: small.
2.  **C — SSA name collision on struct-field temps.** One bug in name
    minting for `_<field>_<idx>` locals. Add suffix from the parent
    scope or use a monotonic counter. ETA: small.
3.  **E — insertvalue into non-aggregate.** Tuple `MirType::Tuple(...)`
    must lower to `{T1, T2, ...}` LLVM struct type, not `i64`. Check
    type-lowering switch in `kryos-codegen-llvm/src/types.rs`. ETA:
    small-medium.
4.  **B / B' — string/ptr type confusion.** The largest class. Strings
    in Kryos are `i64` packed pointers in WASM but real `ptr` in
    native LLVM. The backend must track "this i64 is actually a
    string-pointer" vs "this i64 is arithmetic" and emit the cast at
    every cross-domain use site. Single architectural fix once
    located. ETA: medium.
5.  **D — opaque type cast.** Enum payload extraction reaches a cast
    before the enum type is materialised. Type-pre-pass needs to walk
    the function once and emit all named-struct definitions before
    bodies are lowered. ETA: medium.
6.  **T — runtime divergence on generics.** Build succeeds, semantics
    wrong. Likely monomorphisation specialises the same generic
    function differently between Cranelift and LLVM. Bisect with
    minimal repro from `test_generics`. ETA: depends on the bug.

## Runner

`tests/parity/run_parity.sh` builds the compiler if needed and runs
every smoke test under both backends. It classifies every LLVM
failure into the class table above using regex matches against the
LLVM verifier output. Writes a human-readable matrix + JSON report
to `tests/parity/results/parity-<sha>.txt|json`.

Usage:

```bash
# Full matrix.
tests/parity/run_parity.sh

# One test (no .kry extension).
tests/parity/run_parity.sh test_string_clobber

# Stop at the first divergence.
FAIL_FAST=1 tests/parity/run_parity.sh
```

Exit codes: 0 if every test passes both backends, 1 on any
divergence, 2 on environment / build failure.

## CI integration (planned for M3)

A new workflow job `parity-matrix` runs `tests/parity/run_parity.sh`
on Linux (Ubuntu), Windows (windows-latest), and macOS
(macos-14 / Apple Silicon). On PR it must report `parity: 32/32
both_pass=32` before merge. On master push, the run output is
uploaded as an artifact for trend analysis.

## Done definition for the parity-matrix half of M3

- `tests/parity/run_parity.sh` exits 0 on Linux, macOS-14, and Windows.
- CI gate `parity-matrix / both_pass == total` on PRs.
- `STABILITY.md` updated: LLVM release path is tier-1 alongside the
  Cranelift JIT path.
- `ROADMAP.md` "v2.9 — LLVM backend parity" section removed (folded
  into v3.0 release notes).

WASM is the other half of M3 and lives in its own audit file once
the WASM runner story is wired up.
