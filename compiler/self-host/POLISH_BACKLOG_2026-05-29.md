# Kryos polish backlog (from 4-investigator swarm, 2026-05-29)

Self-hosting is achieved; crutches [a] KRYOS_SKIP_TYPES and [c] inference both
REMOVED (see CRUTCH_REMOVAL_FINDINGS_2026-05-28.md). Crutch [b] force-spill is
the only one left -- perf-only, non-blocking, needs a cdb runtime trace.

## DONE this session (commits)
- 0b4bd6f re-enable call-return struct inference (crutch [c]); 39769e6 drop canonical annotation
- 421165f CLAUDE.md + cd4aba3 docs/19: corrected FALSE "missing feature" claims
- 7c27dbd resolver visitor completion (silent broken selective imports)
- 30d846a wire E0100 onto the type-mismatch diagnostic
- 99e50f2 iter.sort O(n^2) -> O(n log n) merge sort

## VERIFIED facts (corrected docs)
- String interpolation `"{name}"` (NOT `${}`), nested block comments `/* */`,
  `else if` (alias for elif), and tuple destructuring `let (a,b)=...` ALL WORK
  in the full compiler. Tuple destructuring is MISCOMPILED by Cranelift
  (`kryos run` -> field-access-on-unknown-struct, returns 0) but correct on LLVM
  (`build --release`). The self-host parser is a deliberate subset that avoids these.
- Moved-value code is E0300 (not E0382); E0501 does not exist (caps use E-CAP-*).

## REMAINING -- ranked (highest value first)

### Self-hosting (last crutch)
- [b] force-spill removal: regalloc.kry:1058 forces call-crossing intervals to
  spill. Replacing with reg_pool_alloc_callee_saved(pool) (the dead fn at :753)
  BUILDS but stage-2 then mis-parses (parse_expr_bp `pp` clobber, d2.kry
  "Parse errors:1") -> a REAL live callee-saved clobber static analysis can't see.
  NEXT: cdb write-watchpoint on pp's reg inside stage-2 parse_expr_bp; also the
  spill-slot byte/index inconsistency (regalloc.kry:1079/1083 store bytes vs
  codegen.kry:179 cg_stack_offset_ra *8 again, masked by the 2048-byte prologue fudge).

### Correctness
- [DONE 2026-05-29 s8] Tuple destructuring Cranelift bug FIXED. Root cause: infer_expr_type
  (kryos-mir/lower.rs:3868) returned MirType::I64 for TupleLiteral, so the destructure temp
  got the wrong type and Cranelift's field guard fell into the unknown-struct->0 fallback.
  Now infers MirType::Tuple element-wise. `kryos run` prints 10/30 (was 0/0); LLVM unchanged;
  fixed point holds 989ba174. Minimal fix; no codegen guard needed.
- [DONE 2026-05-29 s8] ffi.dlcall0..8 now work on BOTH backends. (1) Cranelift JIT:
  registered all kryos_ffi_* symbols in jit.rs JitCompiler::new (impls existed in
  kryos-stdlib-native/ffi.rs, were unregistered -> unresolved-symbol). (2) LLVM AOT
  (recon wrongly said it worked): emit_extern_declarations had dlcallv* but NOT the
  i64-returning dlcall0..8 nor read_*/write_* -> "undefined value @kryos_ffi_dlcall1".
  Added the declares. Verified both backends: msvcrt abs(-42)=42. Fixed point 989ba174.
  NOTE: tests/parity/gen_decls.py generates the LLVM declare block; it emitted dlcallv
  but not dlcall -- update the generator so a regen doesn't drop the manual declares.

### Perf
- O(n^2) string building everywhere (`s = s + ch` loops) in string.kry:125-160,
  279-304 / fmt.kry / json.kry serialize. Add a StringBuilder over a grow-buffer.
  Blocked partly by anemic bytes.kry (4 fns -- add slice/extend/to_str/from_str). M.

### Diagnostics (stage-0 engine is strong; self-host regresses to bare strings)
- [DONE 2026-05-29 session 8] Self-host byte-offset -> line:col. Was: tc_error/p_error
  printed "at 1234" (raw byte). Now: byte_to_line_col()+format_diag() in main.kry do a
  PRINT-TIME rewrite of the trailing " at <off>" suffix -> "file:line:col: msg" (1-based,
  byte columns since source is ASCII). p_error auto-attaches the current-token offset so
  all 6 previously-unlocated parser sites get a location. No Parser/TypeChecker struct
  change, no codegen-path change. Wired all 7 print sites. Verified: fixed point
  sha 989ba174, stage-2 smoke renders line:col on parse+type errors. Caveat: EOF errors
  point 1 byte past last char; columns are byte (not codepoint) columns.
- [DONE 2026-05-29 s8] Replace stray E0382/W0383 (kryos-ownership/analysis.rs:586,596).
  Added E0303 (partial move) + W0300 (conditional move) consts in kryos-errors/codes.rs,
  wired explain.rs match+list+articles. `kryos explain E0303|W0300` now work.
- [DONE 2026-05-29 s8] Folded E-CAP-* into E0501..E0507 (codes.rs consts, 9 checker.rs
  sites, 7 explain articles + list/match, ~13 test assertions updated). `kryos explain
  E0501..E0507` work. 75 capability tests green. Fixed point 989ba174.
- Code the ~12 codeless parser errors + ~32 codeless typecheck errors; replace "here". M.
- [DONE 2026-05-29 s8] LSP now runs ownership + capability passes too (kryos-lsp
  check_source extends type_diags with analyze_ownership().errors + check_capabilities()).
  Added the two crate deps. LSP surfaces E0300/E05xx, not just type errors.
- offset_to_line_col returns BYTE columns -> caret misaligns on multibyte UTF-8 lines. Verify+fix.

### Ergonomics gaps (real)
- [if-let + while-let DONE 2026-05-29 s8] parse-time desugar to match in parser.rs
  (parse_if_rest/parse_if_let/parse_if_let_else + parse_while_let). Verified JIT+AOT with
  local enum; fixed point 989ba174. let-else STILL PENDING (B6, needs AST/scope work).
- [DONE 2026-05-29 s8] glob `use a::b::*` works -- parser accepts `*` (parse_import) and
  the resolver already imports-all when items is empty. No AST/resolver change needed.
- [DONE 2026-05-29 s8] file_write now creates parent dirs (create_dir_all in
  kryos-rt/builtins.rs kryos_builtin_file_write). Verified AOT: nested path -> rc=0.
- [DONE-NATIVE 2026-05-29 s8] Array index with any int already works on Cranelift+LLVM
  (typechecker is_integer() accepts it; both backends sign-extend). Verified arr[i32]=20
  on JIT+AOT. CLAUDE.md gotcha #6 corrected. WASM v0.1 backend has type-coercion gaps
  (i32-index I32WrapI64 misuse AND i64-const->i32-local store) -> DEFERRED to a future
  WASM-hardening pass; WASM can't compile real programs yet (no to_string). Not worth
  fixing one symptom of an experimental backend now.

### Cleanup (low value, identical-MIR)
- ~37 redundant `let mut X: Struct = user_fn(...)` annotations now inferable.
  SAFE (mutable -> identical MIR) in mir/lower/main/types/parser.kry; RISKY for
  IMMUTABLE `let X: Struct = ...` (alias-shortcut path). Strip only mutable ones, verify fixed point.

## Method note
All findings from read-only opus swarm investigators (Read/Grep/Glob only, no build).
Every fix must be verified by the orchestrator under scratch/kryos-session-guard.ps1
(per-proc 18GB cap + 8GB free-RAM floor) -- this project's self-host builds have
OOM-crashed the machine; the guard caught 3 leak/explosion events this run.
