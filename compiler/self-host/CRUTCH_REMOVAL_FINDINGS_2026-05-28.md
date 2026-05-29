# Self-host crutch-removal findings (session 6, 2026-05-28)

Goal: remove all 3 self-hosting crutches (KRYOS_SKIP_TYPES, force-spill regalloc
workaround, annotate_calls.py / disabled call-return inference) without ever
crashing the machine (35-42GB stage-1 leak history).

Method: 3 parallel read-only opus investigators (claude-swarm) root-caused each
crutch; only the orchestrator built, serialized, under a memory guard
(scratch/kryos-session-guard.ps1: kills any build proc >18GB or when system free
RAM <8GB). The guard fired 3 times this session (10GB, 10GB, 18GB leaks) and
prevented every crash.

## RESULT SUMMARY

| Crutch | Status | Why |
|---|---|---|
| KRYOS_SKIP_TYPES | **REMOVED** (commit 083dbe9) | typecheck OOM was @copy TCExprResult; dropping @copy fixed it |
| force-spill regalloc | DEFERRED | proposed fix re-broke parse_expr_bp; real call-clobber (cdb needed) |
| annotate_calls.py / inference | DEFERRED | fix is correct but explodes memory to 18GB; architectural |

Plus a CRITICAL leak-regression fix (commit c1e4782) that was blocking the
bootstrap entirely (see below).

---

## [PRE] Leak regression in committed HEAD (FIXED, commit c1e4782)

Committed HEAD's bootstrap-win.sh OOMed to ~10GB at stage-1->stage-2.obj.
Root cause: commit 12e50d4 added typechecker Pass 1.5 (top-level let globals)
UNGATED -- it ran even under KRYOS_SKIP_TYPES. Its `tc2 = tc_define(tc2,...)`
per-let threading over the 16-module concat is O(N^2). Fix: gate Pass 1.5 +
Pass 2 together under SKIP_TYPES in tc_check_module (types.kry). The 05-25
milestone (sha fee6a79) predates 12e50d4; the current converged fixed point is
sha 812a1746...(types.kry source changed -> obj legitimately changed).

## [a] KRYOS_SKIP_TYPES -- REMOVED (commit 083dbe9)

- types.kry has NO ownership checker; type-rule false positives were already 0
  (commit 12e50d4). The ONLY blocker was a Pass-2 heap OOM at bootstrap size.
- Root cause: `@copy struct TCExprResult { tc: TypeChecker, ty }` (types.kry:772)
  embeds the growing (non-@copy) TypeChecker; tc_check_expr builds one per
  expression node, recursively -> deep-clone-on-construct = O(N^2). Same class
  as the step-84 Lexer fix.
- Fix: drop @copy from TCExprResult (used linearly: construct, destructure, discard).
- VERIFIED: `check` on full concat WITHOUT skip-types runs Pass 1.5+Pass 2 in
  ~3.7s, bounded memory, 0 errors. bootstrap-win.sh no longer sets
  KRYOS_SKIP_TYPES; fixed point holds (812a1746). The self-host now type-checks
  its own source as part of every build.

## [b] force-spill regalloc workaround -- DEFERRED (needs cdb runtime trace)

- Swarm hypothesis: just call the existing-but-dead reg_pool_alloc_callee_saved
  (regalloc.kry:753) in ra_linear_scan_with_calls (1058) instead of force-spill;
  the Heisenbug was a separate (now-fixed) R12-fallback collision.
- ATTEMPTED + REJECTED: with that change, stage-2 mis-parsed `(a + b) / a`
  ("Parse errors: 1" on /tmp/d2.kry) and failed to emit stage-3.obj. So callee-
  saved allocation alone is INSUFFICIENT -- the swarm's flagged "Alt 1" (a
  genuine live callee-saved clobber not visible to static analysis) is REAL.
- NEXT: cdb hardware write-watchpoint on the callee-saved reg holding `pp` inside
  stage-2's parse_expr_bp loop, single-step into each callee, find the one that
  writes a callee-saved reg without a matching prologue push. ALSO investigate
  the flagged spill-slot byte/index inconsistency (regalloc.kry:1079/1083 stores
  bytes; codegen.kry:179 cg_stack_offset_ra multiplies by 8 again).
- force-spill is CORRECT (compiler self-hosts); it is only a PERF cost. Non-blocking.

## [c] annotate_calls.py / disabled call-return inference -- DEFERRED (architectural)

- Two linked defects per swarm:
  (1) Cranelift `__kryos_clone_<Name>` / @copy-construction array-field clone
      (crates/kryos-codegen-cranelift/src/codegen.rs ~2009, ~3046) selected the
      WRONG per-element clone fn (shallow kryos_array_clone instead of element-
      type clone). Latent today (self-host @copy annotations carry empty arrays).
  (2) lower.kry call-return struct-type inference disabled (relies on baked-in
      annotations like `let mut lex: Lexer = lexer_new(...)`).
- defect (1) fix (route [str]/[Struct] element arrays through kryos_array_clone_deep
  with the element clone fn, mirroring the drop selector at codegen.rs:6150) is
  SEMANTICALLY CORRECT and VERIFIED in isolation (a non-empty [str] @copy struct
  repro printed "xy" cleanly; previously stack-overflowed).
- BUT it EXPLODES MEMORY on the self-host: bootstrap stage-1 ballooned to 18.4GB
  (guard-killed). Reason: ~35 CORE @copy structs have array fields (Expr, Stmt,
  Decl, Module, MirFunction, RValue, Instruction, TypeInfo, Parser, ...). The
  self-host threads these by value pervasively; deep-cloning them all = the same
  O(N^2)/heap-blowup class as the original 42GB leak. Shallow clone is
  LOAD-BEARING for memory, and the double-free it theoretically risks does NOT
  manifest (the compiler self-hosts correctly with shallow clones).
- annotate_calls.py is NOT wired into any build script; the annotations are baked
  into the source. So the compiler IS truly self-hosting standalone today.
- The REAL fix = de-@copy the hot @copy structs (continue the Lexer / LowerCtx /
  TypeChecker / TCExprResult pattern) so cloning is rare, THEN defect-1's deep
  clone is safe and defect-2 inference can be re-enabled. This is a full
  value->move semantics re-architecture of the self-host AST/MIR (multi-session).
  Non-blocking: the compiler self-hosts with annotations.

## SAFETY (kept the machine alive)

- KryosTwin service STOPPED+Manual (the auto-leak trigger). KryosLeakGuard RUNNING.
- scratch/kryos-session-guard.ps1: per-proc cap + system-free-RAM floor, 1s poll,
  whitelist (kryos-stage*/kryos/cargo/rustc/link/cl/ld). Caught all 3 leaks.
- Build discipline: --release -j2 only; never two builds at once; swarm members
  were Read/Grep/Glob only (provably cannot build).

---

## SESSION 7 UPDATE (2026-05-29): crutch [c] inference RE-ENABLED (no deep clone needed)

Reversed the session-6 [c] deferral. The swarm had SPECULATED that re-enabling
inference (defect 2) requires the cranelift deep-clone (defect 1), which explodes
to 18GB. That was wrong: defect 2 ALONE, using the existing SHALLOW clone, works.

- Re-enabled user-call struct return-type inference in lower_fn_call (commit
  0b4bd6f): ctx_fn_ret_type override when result is ANY, struct-only.
- VERIFIED: bootstrap fixed point stage-2==3==4 (sha 139e1cc1...), examples 9/9,
  memory bounded (no explosion -- shallow clone is cheap). The theoretical
  double-free the deep clone guards does NOT manifest on the self-host.
- The self-host typechecker already inferred user-call returns (tc_check_let
  infers from value; tc_check_fn_call returns sig.ret_type), so manual call-init
  annotations are redundant for BOTH passes. PROVEN by dropping the canonical
  `let mut lex: Lexer = lexer_new(src)` -> `let mut lex` (commit 39769e6):
  still self-hosts + examples 9/9. annotate_calls.py is obsolete.

CRUTCH STATUS: [a] KRYOS_SKIP_TYPES REMOVED; [c] inference/annotate_calls REMOVED
(inference live; ~24 redundant annotations remain as optional cleanup);
[b] force-spill = LAST crutch, perf-only, deferred (real call-clobber needs cdb).
