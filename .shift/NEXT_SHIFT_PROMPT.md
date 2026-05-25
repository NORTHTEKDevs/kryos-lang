# Next-shift handoff (kryos-lang self-host)

## SESSION 3c (2026-05-24): DEBUGGER WORKING + LINCHPIN root-caused

Commits b0c8622 (hang), 3919065 (field-access fix #1), 86eed3e/e54345d (diagnostics),
4bc17f7/76f3f08 (docs). Full detail: self-host/STAGE2_MEMORY_BLOCKER.md SESSION 3a/3b/3c.

WINS THIS SESSION:
- Hang fixed (is_alpha nested short-circuit terminator). stage-2 no longer hangs.
- Tokenize VERIFIED correct (11 tokens on hi.kry). Earlier "32 tokens" was a STALE
  kryos-sh-full.kry -- ALWAYS regenerate the concat from current sources.
- DEBUGGER installed + working: cdbX64.exe (winget Microsoft.WinDbg) at
  ~/AppData/Local/Microsoft/WindowsApps/cdbX64.exe. Build stage-1 with `-g` for a PDB
  (cdb symbolizes it). See STAGE2_MEMORY_BLOCKER.md SESSION 3c for cdb recipes
  (rbp-walk script is essential -- kb fails on no-unwind self-host frames).

THE LINCHPIN (do this first -- unblocks everything):
~300 call-init field-access sites (`let x = call(); x.field` reads field 0 because x
is ANY-typed) need the GENERAL type-inference fix. That fix is ALREADY WIRED
(MirModule/LowerCtx fn_sig_*, pass 1.5 struct-only, ctx_fn_ret_type) -- just re-enable
the lookup in lower_fn_call (lower.kry, ~line 1061, currently a NOTE). BUT it triggers
a STAGE-0 codegen bug: cdb rbp-walk (PDB) proves __kryos_clone_Annotation recurses into
itself ~18x -> stack overflow -> AV in kryos_string_clone. Root: stage-0 generates the
@copy clone of a struct's array field [str] with the WRONG per-element clone fn (the
struct's own clone instead of kryos_string_clone). Only manifests on NON-EMPTY args
(the source's @copy/@test annotations have empty args, so it never showed).

FIX LOCATION: crates/kryos-codegen-llvm/src/codegen.rs (the __kryos_clone_<Name> body /
@copy struct field clone). Ensure array field [T] uses the T-appropriate element clone
(string_clone for [str], not the containing struct's clone). cdb-confirm by breaking
__kryos_clone_Annotation, dumping the element-clone fn ptr it calls. THEN re-enable the
general fix and the whole pipeline's call-init field access resolves at once.

VERIFY a clean stage-2 tokenize (should print "Tokens: 11"):
  cd compiler
  KRYOS_NO_ASLR=1 ./target/release/kryos.exe build self-host/main.kry -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  # REGENERATE the concat (critical):
  cd self-host; cat runtime.kry token.kry lexer.kry ast.kry parser.kry types.kry mir.kry lower.kry optimize.kry regalloc.kry x86.kry codegen.kry elf.kry coff.kry linker.kry main.kry > ../target/bootstrap/kryos-sh-full.kry
  grep -vE '^use (token|lexer|ast|parser|types|mir|lower|optimize|regalloc|x86|codegen|elf|coff|linker|runtime|main)$' ../target/bootstrap/kryos-sh-full.kry > /tmp/f; mv /tmp/f ../target/bootstrap/kryos-sh-full.kry; cd ..
  KRYOS_SKIP_TYPES=1 KRYOS_NO_ASLR=1 ./target/bootstrap/kryos-stage1.exe obj target/bootstrap/kryos-sh-full.kry -o target/bootstrap/kryos-stage2.obj
  # link via self-host/link_stage2.bat, then: stage-2 obj hi.kry  (Tokens: 11, then parser crash)

---

# Next-shift handoff (kryos-lang self-host)

## SESSION 3 (2026-05-24): "BLOCKER #2" (stage-2 hang) ROOT-CAUSED + FIXED

Commits: b0c8622 (hang fix), 3919065 (field-access), 86eed3e (diagnostics),
4bc17f7 (docs). Full detail in `self-host/STAGE2_MEMORY_BLOCKER.md` (SESSION 3).

WHAT CHANGED: stage-2 went from "compile pipeline silently no-ops / hangs in
tokenize" to **tokenizes correctly**, now crashes in the PARSER. Two real bugs:
- **is_alpha infinite loop** (the actual blocker #2; step-86's regalloc theory
  was WRONG). `lower_short_circuit_and/or` checked `block_has_terminator(right_bb)`
  instead of `ctx.current_block`; nested `(A and B) or (C and D) or E` left the
  merge block unterminated -> fall-through self-loop. Fixed in lower.kry.
- **field access on let-bound locals read index 0**. `let x = expr` typed x ANY
  unless RHS was a struct literal -> `x.field` = field 0. Fixed: lower_let_stmt
  uses the operand's type. LowerCtx de-@copy'd (was O(n^2) per-fn clone).

NEW TOOLS (env-gated, in kryos-rt): `KRYOS_WATCHDOG=1 KRYOS_WATCHDOG_S=N` samples
the hung thread's RIP (the ONLY way the non-allocating is_alpha loop was found);
`KRYOS_FAULT_TRACE=1` prints fault RVA; `self-host/linkmap_stage2.bat` -> /MAP for
RVA->symbol. Differential harness: `stage-1 obj tiny.kry` + link_stage2.bat + run.

### DO THIS NEXT (the tail is now a clear, repeatable pattern)
1. stage-2 `obj hi.kry` crashes in the PARSER on the same call-init field-access
   pattern: `let p = parser_new(tokens)` types p ANY -> `p.field` index 0.
   QUICKEST: annotate parser/typechecker/main call-init sites, e.g.
   `let mut p: Parser = parser_new(tokens)`. WATCH the immutable-alias shortcut
   in lower_let_stmt — it ignores annotations; make those `let mut` or fix the
   alias to honor s.let_type.
2. BETTER (general, removes need for annotations): re-enable the user-call
   return-type inference in lower_fn_call (currently DISABLED with a comment).
   It typing call results as their struct type triggers an un-root-caused
   downstream lowering CRASH on the full source (isolated repros pass). Bisect
   which construct crashes (build small .kry that mixes struct-returning calls
   until it segfaults via stage-1). The fn_sig table is already wired
   (MirModule/LowerCtx fn_sig_*, pass 1.5) — keep it struct-returning-only and
   NEVER `let names = ctx.fn_sig_names` (deep-clones -> O(n^2); index directly).
3. Then march the same fix through parse->typecheck->lower->codegen, retesting
   `stage-2 obj hi.kry` after each, until it emits a valid obj. Then bootstrap.sh.

### Verify the hang fix / field fix (fast, ~15s)
  cd compiler
  KRYOS_NO_ASLR=1 ./target/release/kryos.exe build self-host/main.kry -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  # is_alpha-pattern repro must print 1,0,1 (was: hang):
  printf 'fn c(x:i64)->bool{return (x>=65 and x<=90) or (x>=97 and x<=122) or x==95}\nfn main(){println(to_string(c(102)))\nprintln(to_string(c(48)))\nprintln(to_string(c(95)))}\n' > /tmp/b.kry
  KRYOS_SKIP_TYPES=1 ./target/bootstrap/kryos-stage1.exe obj /tmp/b.kry -o /tmp/b.obj
  # link via link_stage2.bat (cygpath -w the paths!), run: expect 1 0 1
  # full obj (bounded ~300MB now, NOT 10GB): KRYOS_SKIP_TYPES=1 stage1 obj kryos-sh-full.kry

---

## STEP 74 UPDATE (2026-05-24): BLOCKER #1 SOLVED — stage-2 LINKS + RUNS

- `self-host/rt_shim_win.c` provides the 24 Windows intrinsics codegen.kry:1027
  inlines only on Linux. `self-host/link_stage2.bat` compiles it + links.
- Build a running stage-2:  `bash self-host/build_stage2_extlink.sh`  then
  `MSYS_NO_PATHCONV=1 cmd.exe /c <bat> <obj> <exe>` (the .sh emits the obj; relink
  via link_stage2.bat). NOTE: cmd.exe from git-bash NEEDS `MSYS_NO_PATHCONV=1`.
- stage-2 VERIFIED: usage banner, CLI dispatch (argv incl argv[0]), string
  compare, len/println/error/exit all correct.
- stage-2 BROKEN: the compile pipeline (`ast/check/compile/obj <file>`) silently
  no-ops — EXIT=0, zero output, not even dump_ast's first println. This is
  BLOCKER #2 (codegen correctness in LARGE fns), now the SOLE gate. stage-2
  miscompiles its own pipeline functions (file_read/tokenize/parse/dump_ast).
- Self-hosting is gated entirely on blocker #2 now. See progress.txt step 74.

---
# (historical) Next-shift handoff — updated 2026-05-23, steps 63-68

## TL;DR of this shift (steps 63-68, commits be352d9..HEAD)

THE BIG WIN: fixed the **42GB memory leak** that crashed the machine on
every `compile self-host` / KryosTwin build. Full self-host compile now
completes at **~1.6GB** instead of OOMing. plus 3 codegen correctness fixes.

Landed (all committed + tagged shift/kryos-self-compile/63..67):
- 64: @copy struct push use-after-free. push(arr, @copy_struct_local) stored
  a pointer then drop() freed the body. Fixed in kryos-mir/lower.rs
  (consume_call_args now consumes @copy struct push args) + kryos-ownership.
  Cranelift fully correct; examples 8/8.
- 65: LLVM struct-aggregate boxing. coerce_value(struct->i64/ptr) did
  `extractvalue ...,0` (field 0) where a POINTER was expected -> segfault on
  arr[i].field. Now boxes multi-field structs. (kryos-codegen-llvm)
- 66: **THE LEAK**. Lexer/Parser were @copy structs embedding the src string
  / token array; threaded by value per-char/per-decl, deep-cloning the whole
  collection each time = O(N^2). Moved those read-only/append-only collections
  to module-globals (g_lsrc, g_ltokens, g_ptokens). self-host/lexer.kry +
  parser.kry. Verified: full obj 42GB-OOM -> 1.6GB; examples 8/8; bootstrap 16/16.
- 67: compile path now honors KRYOS_SKIP_TYPES + skips optimizer (matches obj
  path). stage-2 compile now passes typecheck+codegen and reaches the linker.

## Current state
- stage-0 (Rust): builds clean, kryos 4.43.0-rc.4. `--release -j2` ONLY (15GB
  free start; debug=48GB OOM). Binary: compiler/target/release/kryos.exe.
- stage-1: builds from self-host via stage-0. Per-module bootstrap 16/16.
- stage-2: `stage-1 compile kryos-sh-full.kry` now COMPLETES codegen at ~1GB,
  then FAILS at link: undefined runtime symbols (kryos_builtin_exit,
  kryos_string_new, kryos_println_str, kryos_builtin_len). The self-host's own
  linker.kry cannot PROVIDE the Rust runtime — runtime.kry only declares them
  extern.

## Two remaining blockers (in priority order)

### 1. Runtime provision for stage-2 exe
The self-linked stage-2 has unresolved runtime externals. Options:
  (a) EXTERNAL LINK (proven by prior shift): stage-1 emit per-module .objs
      (test_bootstrap does this, 16/16), then link with link.exe against
      target/release/kryos_rt.lib + kryos_stdlib_native.lib + msvcrt/vcruntime/
      ucrt/legacy_stdio_definitions/kernel32. NEEDS %LIB% set (vcvars64.bat).
      See crates/kryos-linker/src/linker.rs:280-333 for the exact lib list +
      entry. This validates stage-1 codegen but isn't "self-linking".
  (b) Implement the runtime in Kryos inside runtime.kry so it's self-contained
      (true self-hosting), OR have linker.kry link kryos_rt.lib.

### 2. Per-module determinism (needed for stage-2 == stage-3 fixed point)
After step 66: token/ast/elf/coff = distinct=1 (deterministic). lexer/parser/
main still distinct=3. This is residual @copy-struct codegen non-determinism
in the larger modules (NOT the leak, NOT the push bug). Use
`KRYOS_RA_DUMP=1` (added to codegen.kry) + self-host/determinism.sh MODULE N
to chase it. e0/d0 are deterministic so the basic path is clean; the variance
appears with struct-heavy code. Likely another @copy-struct store-by-pointer
sink (map insert of struct? struct field = struct? struct returned-and-stored?).

## SAFETY (critical — this project has crashed the machine)
- KryosTwin service is STOPPED + Manual (it auto-ran self-host builds = the
  leak trigger). Leave it Manual. With the leak now fixed its builds would be
  safe (~1.6GB) but don't re-enable without testing.
- KryosLeakGuard service: RUNNING (keep it). Kills kryos-stage*>2GB every 30s.
- TOOLS (use these when running stage-1 on large input):
    scratch/kryos-watchdog.ps1 -CapMB 4000 -MaxSec 200 &   (1s poll, hard kill)
    scratch/kryos-memwatch.ps1   (one-shot snapshot + kill >2GB)
  Always run stage-1 on the FULL source under a watchdog.
- Build: --release -j2. Never bare `cargo build` (debug=48GB).

## Verify recipe
  cd compiler
  rm -f target/bootstrap/kryos-stage1*
  KRYOS_NO_ASLR=1 ./target/release/kryos.exe build self-host/main.kry \
      -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  bash self-host/test_bootstrap.sh   # 16/16
  bash self-host/test_examples.sh    # 8/8
  bash self-host/determinism.sh main 5   # determinism check

## Hard rules (unchanged)
- elif not else if. Don't modify kryos_string_new ABI. Keep R12 fallback,
  call-clobber helpers, deep-clone-on-push, binop scratch-load, RegPool.used i32.
- Preserve build reproducibility. Don't switch @copy struct array fields back
  to deep-clone in lexer/parser (that reintroduces the 42GB leak).
