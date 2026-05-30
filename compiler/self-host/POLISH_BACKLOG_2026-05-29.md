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
- [PARTIAL DONE 2026-05-29 s8] O(n^2) `s = s + ch` string building. stdlib/string.kry
  to_upper/to_lower/reverse converted to O(n) via the buffer builtins
  (alloc(slen) + ptr_set_byte + buf_to_str; output size is known = input byte length).
  Verified byte-identical on JIT+AOT (HELLO/hello/fedcba/empty/[Z]); fixed point 989ba174.
  These are global builtins (no import needed). pad_left/pad_right ALSO done (output size =
  width, known): verified [00042]/[42...]/[hello]/[   x] on JIT+AOT. So to_upper/to_lower/
  reverse/pad_left/pad_right (the 5 common ops) are all O(n) now. fmt.kry _replace_all
  (TWO-PASS alloc: count matches -> fill exact size; written bytes provably == out_len) +
  fmt pad_left/pad_right ALSO done; verified JIT+AOT (hell0 w0rld, bbbbbb, ba, a-b-c-d,
  [00042], [42...]). REMAINING string perf: fmt center/_escape_string + json.kry serialize
  (unknown size; the grow-buffer buf_new family is UNUSED by any .kry and may have JIT-
  registration / handle-vs-ptr gaps -> use two-pass alloc, NOT buf_new). NOTE: do NOT touch
  self-host lower.kry/codegen.kry concat (bootstrap critical path).

### Bare variant patterns (found + fixed 2026-05-29 s8 via correctness sweep)
- [DONE] Bare (unqualified) enum-variant PATTERNS now parse + bind: `match s { Circle(r)
  => .., Some(x) => .. }` without the `Enum.` prefix (CLAUDE.md documents `Some(x)` but only
  qualified `Option.Some(x)` parsed before). Fix across 3 sites: (1) parse_pattern emits
  `Pattern::Enum { name:"", variant, fields }` for `Name(fields)`; (2) types/check.rs
  bind_pattern resolves the empty name from the subject's `Type::Enum{name}` to bind field
  vars; (3) mir/lower.rs match lowering resolves it from subj_enum_name for tag+field
  extraction. Verified JIT+AOT (Circle/Rect 12/15, Some/Nothing 99/-1); fixed point 989ba174
  (self-host uses qualified patterns -> additive). 
- [REMAINING] Bare variant CONSTRUCTION (`Rect(3,5)` / `Some(42)` without `Enum.`) still
  errors "undefined variable Rect" -- separate expression/FnCall-resolution path. M.

### map<K,V> annotation vs literal mismatch (found + fixed 2026-05-29 s8 via sweep)
- [DONE] `let m: map<str,i64> = #{...}` failed: "type mismatch expected map<str,i64> found
  Map<str,i64>", then "not indexable". Root cause: the type resolvers only matched CAPITAL
  `Map`/`Set` (check.rs:113/181/194 + mir/lower.rs:5178) but CLAUDE.md documents lowercase
  `map<K,V>`, so the annotation fell through to Struct("map") while the `#{}` literal was
  Type::Map -> mismatch -> map unusable. Fix: accept `Map|map` + `Set|set` in both the
  typechecker and MIR lowering; display Type::Map/Set lowercase (matches the written form).
  Verified read/write/update on JIT (1, 5, 99/20); types tests green; fixed point 989ba174.
- [DONE] generic `Result<T,E>` / `Option<T>` annotations now unify with stdlib `Result.Ok(x)`
  / `Option.Some(x)` construction. Root cause: stdlib defines NON-generic `enum Result {
  Ok(any), Err(any) }` (-> Type::Enum) but the annotation `Result<i64,str>` is the builtin
  Type::Result -> didn't unify -> "found Result". Fix: unifier (infer.rs) bridges
  Type::Result<->Enum("Result") + Type::Option<->Enum("Option") (stdlib enums are any-typed,
  so inner T/E carry no constraint). Verified JIT+AOT (ok 5 / got 10); types tests green;
  fixed point 989ba174.
- [REMAINING] `contains(m, key)` builtin only does substring search (expects str, not map).
- [PARTIAL — JIT only] Maps now typecheck + work on Cranelift JIT (`kryos run`), but FAIL on
  LLVM AOT (`kryos build --release`). The map-type fix exposed this (maps never reached LLVM
  codegen before). PRECISE DIAGNOSIS: MIR lowering DOES emit kryos_map_get[_str] (lower.rs:
  4634) -- not a missing feature. The bug is LLVM call-arg coercion: it emits
  `%t8 = ptrtoint ptr %t7 to i64` to pass the STRING KEY to kryos_map_get_str(i64,i64), but
  %t7 is already i64 (string-rep inconsistency: strings are sometimes ptr, sometimes i64-handle
  in the LLVM backend). Fix = guard that ptrtoint on the key operand's actual LLVM type. Fiddly
  (string-rep), not a one-liner. Maps work on JIT today.

### Pre-existing bugs surfaced 2026-05-29 s8 (NOT regressions; found while testing)
- `fmt.format("...{0}...", args)`: the `{0}` placeholder collides with Kryos STRING
  INTERPOLATION -- a literal `"{0}"` is interpolated at compile time, so format() gets a
  mangled template ("Hello, {0}!" -> "Hello, 0!"). Fix = change format's placeholder syntax
  or escape. M.
- [DONE 2026-05-29 s8] `any`-typed values miscompiled on LLVM AOT (`load %any`). Root cause:
  lower_type_expr fell `any` through to MirType::Struct("any"); Cranelift maps all aggregates
  to i64 handles (worked) but LLVM emitted an undefined `%any` named type (AOT compile fail).
  Fix: lower_type_expr maps `any`/`Any` -> MirType::I64 (matching the typechecker's
  Type::Error->I64 fallback + Cranelift). NOT deep -- 1 match arm. Verified `any` pass-through
  + `[any]` arrays compile+run on JIT+AOT (42, 10/30). Bootstrap 989ba174 (self-host has no
  `any` uses). REMAINING (deeper, separate): to_string(any) on a STRING/BOOL-valued any is
  still int-interpreted -- `any` carries no runtime type tag. int-valued any works.

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
- [DONE 2026-05-29 s8] Every error now carries a code. The base parser `fn error` defaults
  to E0009 (general syntax error) and the typechecker `fn error` to E0110 (general type
  error); both added to codes.rs + explain.rs with articles. Specific sites keep their
  precise error_with_code codes. 13 codeless parser + 32 codeless type sites are now coded
  + explainable via `kryos explain`. (Per-site SPECIFIC upgrades of the generics = future
  nicety.) 110 parser+types tests green; fixed point 989ba174.
- [DONE 2026-05-29 s8] LSP now runs ownership + capability passes too (kryos-lsp
  check_source extends type_diags with analyze_ownership().errors + check_capabilities()).
  Added the two crate deps. LSP surfaces E0300/E05xx, not just type errors.
- offset_to_line_col returns BYTE columns -> caret misaligns on multibyte UTF-8 lines. Verify+fix.

### Ergonomics gaps (real)
- [if-let + while-let DONE 2026-05-29 s8] parse-time desugar to match in parser.rs
  (parse_if_rest/parse_if_let/parse_if_let_else + parse_while_let). Verified JIT+AOT with
  local enum; fixed point 989ba174.
- [let-else DONE 2026-05-29 s8] `let Enum.Variant(x) = e else { D }`. Handled INLINE in
  parse_block_stmts (is_let_else_ahead lookahead + parse_let_else_desugar) -> NO AST field,
  NO MIR/typecheck change: rest-of-block becomes the match success arm, else-arm = D.
  GOTCHA: user enum types lex as Ident (not TypeIdent) -- detection accepts both. Verified
  JIT+AOT (51/-1; multi-field 7/0; bindings flow to later stmts); fixed point 989ba174.
  if-let / while-let / let-else trio COMPLETE.
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
