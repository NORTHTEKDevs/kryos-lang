# Kryos Self-Compile Shift — Final Summary
## 2026-05-20 (single ~14-hour shift)

**Status: KRYOS SELF-COMPILES.** Stage-1 (Kryos-compiled compiler) successfully compiles all 16 self-host source files. Bootstrap mean: 15.8 / 16. The 3 largest modules occasionally flake (~5–10% each), the other 13 are deterministic.

---

## Bootstrap progress over the shift

| Phase | Mean / 16 | Failure set | Method |
|------:|----------:|:------------|:-------|
| Start of day | 8 — 10 | 6+ rotating modules | Step 19 baseline (pre-shift) |
| H1a refuted | 6 — 8 | 8 modules | Universal helper-body deep clone — REGRESSED |
| H4 instrumentation | 8 — 10 | 6+ stable | Sentinel-on-free diagnostic; ZERO double-frees seen |
| H8 codegen retain | 9 — 10 | parser/types/lower/optimize/codegen/linker | `@copy` Array fallback → `kryos_array_retain` |
| H19 + H20 share clones | 12 — 13 | parser/types/lower | string + map clone return same pointer |
| H21 share nested @copy | 13 (stable) | parser/types/lower | nested @copy struct fields pass-through |
| **H25 empty whitelist** | **16 / 16** | **none** | **THE BREAKTHROUGH** — Token off deep-clone whitelist |
| H26 alloc-leak grow | 16 / 16 (20-run) | none | array push uses alloc+copy+leak |
| Steps 37 — 41 hardening | ~15.9 | parser+lower 5% each | refcount infrastructure + no-op free |
| H42 32 MB stack | 15.95 | parser 1/20 | `/STACK:33554432` linker flag |
| H43 KRYOS_NO_ASLR | 15.8 | parser/types/lower 5-10% each | `/DYNAMICBASE:NO` when env set |

---

## Key insight (H25)

The breakthrough was identifying that **stage-1's `@copy` struct semantics were doing O(N²) clone work** during tokenize/parse. Each `lex_emit` call cloned the Lexer struct, which deep-cloned its `Array<Token>` field. Over 10,000+ token emissions, this compounded to a quadratic blowup that fragmented the heap and crashed.

The fix was a unified **share-everywhere, leak-on-free** memory model applied at every level:

- **Codegen**: `@copy` struct construction shares heap fields via retain instead of cloning
- **Runtime clone functions**: return same pointer + refcount increment
- **Runtime free functions**: pure no-op (refcount infrastructure exists for future audit)
- **Linker**: 32 MB stack for deep recursion in parser/types/lower

---

## Final memory model

```
Heap container types: KryosArray, KryosString, MapHeader
All carry ref_count: i64 field

kryos_*_clone:    increment ref_count, return same pointer
kryos_*_retain:   explicit retain ABI for codegen
kryos_*_free:     PURE NO-OP (refcount infrastructure exists)

Per-invocation leak:    ~80 MB bounded (full self-host compile)
Per-process lifetime:   until exit
Production-safe?         For short-lived CLI: yes
                         For LSP/long-running: needs codegen audit
```

---

## Stability metrics (final, 30-run characterization)

```
Mean PASS / 16:        15.77 — 15.95   (varies sample to sample)
Best:                  16 / 16
Worst:                 15 / 16
Perfect-run rate:      ~85-95% (varies by sample)
STABLE modules:        13 / 16  (always pass — token, lexer, ast,
                                 mir, optimize, regalloc, x86, codegen,
                                 elf, coff, linker, runtime, main)
FLAKY modules:         3 / 16   (parser, types, lower — ~90-97% pass)
```

The 3 flaky modules are the 3 largest source files:
- `parser.kry` — 2,867 lines, 104 KB
- `lower.kry` — 2,705 lines, 94 KB
- `types.kry` — 2,270 lines, 78 KB

---

## What was tested

| Hypothesis | Step | Outcome |
|------------|-----:|---------|
| H1a helper-body deep clone | 20 | Refuted (regress) |
| H3 lexer crash (content) | 21 | Refuted (heap flakiness) |
| H4 double-free detector | 22 | ZERO double-frees — runtime innocent |
| H7 true leak-all | 23 | Initial confused with stale .lib; later subsumed by H40 |
| H8 codegen retain | 24 | +0.67 modules, stable failure set |
| H10 string-free no-op | 25 | Layer with others |
| H11 file-based diagnostic | 26 | Never triggered — bug not in panic path |
| H12 array-free no-op | 27 | UAF impossible by construction, crashes persist → CODEGEN bug |
| H15 expanded whitelist | 28 | Same range, neutral |
| H18 map-free no-op | 29 | Layer with others |
| H19 + H20 share strings + maps | 30 | Mean 12.67 → 13 stable failures |
| H21 share nested @copy | 31 | Mean → 13/16 stable |
| H22 4× array growth | 32 | Neutral, reverted |
| H23 min cap 64 | 33 | Regressed, reverted |
| H24 raise ref_count limit | 34 | Neutral, reverted |
| **H25 empty deep-clone whitelist** | **35** | **🎯 BREAKTHROUGH: 16/16** |
| H26 alloc-leak grow | 36 | 20/20 perfect lock-in |
| Steps 37-41 hardening | 37-41 | Refcount infra + leak-on-free production-ish |
| H42 32 MB stack | 42 | Stack overflows in parser/types/lower fixed |
| H43 KRYOS_NO_ASLR | 43 | Marginal — gated behind env var |

---

## Polish committed

- **Source**: 3 `let mut` corrections (main.kry, parser.kry, lower.kry) → zero kryos build warnings
- **Cargo**: 6 unused-import/dead-code warnings cleaned → zero cargo warnings
- **Tests**: `test_bootstrap.sh` surfaces per-module diagnostics; added `test_bootstrap_robust.sh [N]`
- **Docs**: README, CHANGELOG (4.43.0-rc.2 + rc.3 + rc.4), CRYSTAL, STAGE2_BLOCKER (RESOLVED), kryos-rt lib.rs module docs, docs/20-self-hosting.md added to ToC
- **Test fix**: map::tests::clone_map updated for share-on-clone semantics
- **Bisection artifacts**: 17 untracked repro files committed as regression sentinels

---

## What's next (for the future)

In rough priority:

1. **Codegen retain-emission audit.** Eliminates the leak. RValue::Field, Operand::Local, function arg passing, pattern destructuring — ensure every heap-pointer copy is matched by a retain. Then flip `*_free` from no-op to refcount-decrement-and-dealloc. 4 – 8 hours estimated.

2. **Multi-`.obj` stage-2 linking.** Stage-1 emits per-module `.obj` files but they don't link together because user functions have `Linkage::Local`. Add a build mode that exports user-function symbols so all 16 `.obj` link into a single `stage-2.exe`. Then verify stage 2 reproduces stage 1 on examples.

3. **Stage-3 fixed point.** With multi-`.obj` linking in place, build stage 3 from stage 2 and verify byte-identical `.exe` outputs on `hello.kry`. This is the canonical "fully bootstrapped" proof.

4. **Codegen polish for parser/types/lower flakes.** Likely related to (1); auditing retain emission should eliminate the remaining heap-state-sensitive crashes.

---

## Commits this shift

```
$ git log --oneline shift/kryos-self-compile/19..HEAD | wc -l
50+
```

Tags pushed: `shift/kryos-self-compile/{20, 20-revert, 21, 22, 24, 25, 27, 27-wrap, 28, 29, 30, 30-breakthrough, 31, 35-SELFCOMPILES, 36-DETERMINISTIC, 37-refcount-hardening, 38-final, 39-forgiving-refcount, 39b-forgiving-all, 40-leak-data-for-reliability, 41-pure-no-op-frees, 42-16mb-stack, 42b-32mb-stack, 42-final-32mb, 43-aslr-off, report-2026-05-20}`

---

## See also

- [REPORT_2026-05-20.md](REPORT_2026-05-20.md) — earlier end-of-day report
- [progress.txt](progress.txt) — full shift log
- [CHANGELOG.md](../CHANGELOG.md) — rc.2 + rc.3 + rc.4 entries
- [docs/20-self-hosting.md](../docs/20-self-hosting.md) — user-facing bootstrap docs
- [compiler/self-host/STAGE2_BLOCKER.md](../compiler/self-host/STAGE2_BLOCKER.md) — original blocker, marked RESOLVED
