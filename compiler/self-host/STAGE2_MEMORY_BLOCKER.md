# Stage-2 Bootstrap — progress log

## SESSION 3c (2026-05-24 cont.): DEBUGGER + precise root-causes

Installed WinDbg (`winget install Microsoft.WinDbg`); cdb at
`~/AppData/Local/Microsoft/WindowsApps/cdbX64.exe`. Build stage-1 with `-g` for a
PDB (`kryos build ... -g` -> kryos-stage1.pdb); cdb then symbolizes stage-1.
stage-2 (obj+link, no PDB) still needs the /MAP + manual rbp-walk.

### cdb recipes (proven this session)
- Catch crash + registers:  `cdbX64.exe -g -G -c "g; r; kb 30; q" <exe> <args>`
- rbp-chain walk (kb fails on no-unwind-info self-host frames):
  `-c "g; r $t0=@rbp; .for(r $t1=0;@$t1<25;r $t1=@$t1+1){ln poi(@$t0+8); r $t0=poi(@$t0)}; q"`
- Break mid-recursion: `-c "r $t0=@rsp-0x800000; ba w8 @$t0; g; ..."`
- Run WITHOUT KRYOS_FAULT_TRACE so the VEH doesn't pre-empt cdb.

### CORRECTED understanding (supersedes 3b's "stale file / scale-dependent")
The "32 tokens" WAS partly a stale `kryos-sh-full.kry` (regenerated before the
lex annotation landed). On a FRESH full source, tokenize is CORRECT (11 tokens on
hi.kry) -- VERIFIED. So the lexer self-compiles. ALWAYS regenerate the concat.

Two REMAINING crashes, both = the call-init field-access class (a `let x = call()`
local is ANY-typed, so `x.field` for non-index-0 fields reads field 0):
1. **stage-2 parser crash on hi.kry** (the live blocker): parse chain
   parse_module->parse_declaration->parse_statement->parse_expr_or_assign->
   parse_expr->parse_expr_bp->parse_prefix->parse_primary, then a spurious parse
   error -> token_kind_name -> kryos_string_concat AV. parse_primary mis-reads a
   token field (p_peek(pp).text reads .kind etc.) -> errors on valid input.
2. **the general fix (type call results) crashes STAGE-1**: cdb rbp-walk (PDB)
   shows `_kryos_clone_Annotation` recursing into itself ~18x -> stack overflow ->
   AV in kryos_string_clone. Typing call results triggers @copy DEEP-CLONE of a
   result whose Annotation.args:[str] ends up holding Annotations (type confusion)
   -> infinite clone recursion. This is a STAGE-0 (Rust kryos-codegen) @copy-clone
   generation interaction, NOT a self-host edit.

### WHY the clean fix is blocked
Typing call-init LOCALS via explicit annotation is SAFE (verified: `let mut lex:
Lexer`, `let mut pp: Parser` both compile). But:
- IMMUTABLE `let x = call()` ALIASES the ANY call-temp (alias ignores annotation),
  and skipping the alias to allocate a typed local CRASHES stage-1 the same way.
- The GENERAL fix (type the call RESULT temp) triggers the _kryos_clone_Annotation
  recursion above.
- ~100+ parser call-init sites make per-site `let mut X: T =` annotation impractical.
So the real fix is in STAGE-0: either fix @copy-clone-gen so a typed call result
doesn't deep-clone-recurse, OR make AST structs (Annotation, Parse*Result) not
@copy / clone-by-reference. THEN re-enable the general fix (lower_fn_call +
ctx_fn_ret_type, already wired) and all call-init field access resolves at once.

NEXT: debug WHY a typed ParseAnnotationResult's clone puts Annotations into
Annotation.args (cdb: break _kryos_clone_Annotation, dump the args array r15).
Fix in stage-0 kryos-codegen's @copy clone generation. Then the parser + the rest
of the pipeline should fall into place.

## SESSION 3b (2026-05-24 cont.): CORRECTION + scale-dependent wall identified

CORRECTION to 3a below: "stage-2 now tokenizes correctly" was WRONG. stage-2 no
longer HANGS (real win), but its lexer still mis-tokenizes: it emits ~1 token per
SOURCE BYTE (hi.kry: stage-1=11 tokens, stage-2=32 = byte count). Root: tokenize's
`return lex.tokens` / `lex.pos` read field OFFSET 0 (= src) instead of 8/16, so
`len(tokens)` returns len(src) and p_peek then indexes a string as an array ->
segfault (the "parser crash" at p_peek RVA 0x1d3e2).

BISECTION RESULT (the precise wall): the symptom is `tokenize(source)` returning
~1 token per SOURCE BYTE (whitespace not skipped) ONLY in certain call contexts.
Module-by-module + call-context bisection (all via stage-1 obj + link, run):
- runtime+token+lexer (1594 ln)          -> tokenize correct.
- runtime..lower (8 mod, 11754 ln)       -> correct.
- runtime..x86 (11 mod, 15566 ln)        -> correct.
- ALL 15 modules + a simple test main    -> correct (count=11 on hi.kry).
- ALL 15 modules + main.kry PRESENT but a simple test main runs -> correct (11).
- full source, real main -> emit_object -> tokenize -> WRONG (32 = byte count).
So it is NOT the module count, NOT main.kry's mere presence -- it is the
emit_object CALL CONTEXT. Same `file_read`+`tokenize`+`len` code gives 11 from a
low-local-count function and 32 from emit_object (many live locals). Verified the
not-aliasing-of-call-results change does NOT fix it, and emit_object's disasm
shows `tokens` correctly stored/read -- so tokenize itself RETURNS 32 elements in
that context. This is a register-pressure / scale-dependent codegen miscompile
(the file's own alias comment warns regalloc mishandles this class). Every
individual function (is_alpha, is_alnum, scan_identifier loop, field offsets) is
correct in disasm and in small/medium repros; only the high-pressure aggregate
fails.

PROVEN ROOT (machine-code diff of `tokenize` between a count=11 binary and the
count=32 stage-2, same lexer.kry source):
```
  count=11 (correct)            count=32 (broken)
  mov rax,[r12+8]   ; lex.pos   mov rax,[r12]     ; offset 0 (= lex.src)
  mov rax,[r12+10h] ; lex.tokens mov rax,[r12]    ; offset 0
```
tokenize's `return lex.tokens` / EOF-emit `lex.pos` resolve to field index 0 in
the full binary but to the correct 8/16 in a near-identical binary (= full source
+ one extra fn). So `lex`'s LOCAL TYPE comes out ANY (-> field index 0) for the
SAME annotated line `let mut lex: Lexer = lexer_new(src)` depending on surrounding
code. fix-#1 (operand type) keeps PARAM-sourced locals correct (scan_identifier's
`l.pos` IS [rbx+8] in the same broken binary); only tokenize's annotation/call-
sourced `lex` degrades to index 0.

DETERMINISM REFINED (3 re-emits of the full obj): the .obj HASH differs each time
(regalloc/temp-order non-determinism -- the known "distinct" gap) BUT tokenize's
`lex.pos` offset is CONSISTENTLY 0. So the field-offset bug is DETERMINISTIC for a
given source, and FLIPS to correct (offset 8) only when source content changes
(adding `kry_bisect_main` -- a fn that calls tokenize -- to the full source makes
tokenize's OWN field resolution correct). I.e. it is layout/function-set-dependent
deterministic codegen, NOT heap-random. Adding code that references tokenize/Lexer
changes tokenize's compiled field offsets. Smells like a global table (struct_defs
ordering, a type/temp index, or fn-count-sized state) whose state when tokenize is
lowered depends on the whole program. Two SEPARATE issues remain: (a) this
deterministic field-offset degradation, (b) the obj-hash non-determinism (blocks
stage-2==stage-3 even once (a) is fixed).

NEEDS A DEBUGGER or a memory-safety audit. No cdb/windbg/x64dbg on this host
(checked). Next session options, in order:
1. Make stage-1's codegen DETERMINISTIC first (self-host/determinism.sh +
   KRYOS_RA_DUMP). Non-determinism is the umbrella bug; fix it and the
   field-offset degradation + stage-2!=stage-3 likely both resolve.
2. Audit @copy-struct handling: Stmt.let_type / Rvalue.field_idx may be corrupted
   by share-on-clone aliasing (cf. the step-87 operand-aliasing fix) -- the
   field_idx integer or the let_type array being clobbered fits the symptom.
3. Install a debugger and single-step lower_let_stmt on tokenize in the full
   source to watch s.let_type / local_ty for `lex`.

BLOCKED ON TOOLING: no working debugger on this host (cdb/gdb absent; lldb fails;
the fault tracer + watchdog only catch the SYMPTOM site, not the corruption
source). Cracking this needs either a real debugger (windbg/cdb install) or
painstaking binary-bisection of the full source (remove modules until tokenize
works) to find which code's presence triggers the mis-resolution. This is a
multi-session effort, not a single bug.

The GENERAL user-call return-type fix (lower_fn_call typing call results as their
struct type) ALSO hits a scale-only crash and is left DISABLED. Annotations don't
help either because the field-resolution itself fails at scale.

## SESSION 3a (2026-05-24 cont.): HANG ROOT-CAUSED + FIXED; field-access bug fixed

NOTE: the "tokenizes correctly" claim here is corrected in 3b above.
The stage-2 "hang in tokenize" (Session 2's remaining gate) is SOLVED. Two real
self-host bugs found and fixed. Commits 3919065
(self-host) + 86eed3e (diagnostics) + b0c8622 (the hang fix).

### Tooling built (commit 86eed3e, all env-gated, no-op by default)
- `KRYOS_FAULT_TRACE=1` — vectored exception handler prints faulting RVA.
- `KRYOS_WATCHDOG=1 [KRYOS_WATCHDOG_S=N]` — suspends the main thread after N s and
  reads RIP via GetThreadContext. THIS is what cracked the hang: it catches a
  CPU-bound infinite loop that allocates nothing (alloc/len/etc. counters never
  fire). Map RVA->symbol with `self-host/linkmap_stage2.bat` (/MAP).
- Differential harness: compile a tiny repro with stage-1 (`obj` + link_stage2.bat)
  and run it; compare to `kryos.exe run`. Reliable, unlike in-source markers.

### Bug A — is_alpha infinite loop = the real "blocker #2" (FIXED, b0c8622)
Watchdog pinned the hang to `is_alpha` (RVA mapped via /MAP). Disasm showed a
self-jump loop. Root cause in `lower.kry`: `lower_short_circuit_and/or` checked
`block_has_terminator(right_bb)` instead of the CURRENT block. When the right
operand is itself an and/or (nested `(A and B) or (C and D) or E`, exactly
is_alpha), lowering it switches blocks; right_bb is terminated by the nested
expr while the right expr's own merge block is left open -> codegen falls through
into the false-path block -> infinite loop. Fix: check `ctx.current_block`
(the idiom used everywhere else). Step-86's "regalloc/xor r12" theory was WRONG.

### Bug B — struct field access on let-bound LOCALS read field index 0 (FIXED, 3919065)
`let x = <expr>` typed x ANY unless RHS was a struct literal (resolve_expr_type is
syntactic). So `x.field` resolved to index 0 (read field-0 bytes = a pointer where
an int belonged -> substr out-of-bounds in scan_identifier). Fix: lower_let_stmt
takes the lowered operand's type (`ctx_operand_ty`). Handles identifier-init
(`let mut l = lex`). LowerCtx made non-@copy (per-function deep-clone was O(n^2)).

### REMAINING TAIL (next session)
1. **General user-call return-type inference is DISABLED** (lower_fn_call). The
   fn-signature scaffolding exists (MirModule/LowerCtx fn_sig_*, pass 1.5,
   ctx_fn_ret_type) but typing call results as their struct type triggers an
   un-root-caused downstream lowering CRASH on the full source (works for
   isolated `let p = mk()` and reassign-in-loop repros; crashes on some
   construct at scale). MUST bisect which construct. Memory note: the table
   MUST stay struct-returning-only and MUST NOT be bound to a local
   (`let names = ctx.fn_sig_names` deep-clones; index directly) or it's O(n^2).
2. Meanwhile, **annotate call-init sites**: `let mut lex: Lexer = lexer_new(src)`
   done in tokenize. Parser/typechecker/lower have many `let p = parser_new(...)`
   etc. that will read field index 0 until annotated or fix #1's general form lands.
   NOTE the immutable-alias shortcut in lower_let_stmt ignores annotations — make
   `let x = call()` sites `let mut` or fix the alias path to honor `let_type`.
3. Current stage-2 status: `obj hi.kry` prints `Tokens: 32` then segfaults in the
   PARSER (next call-init field-access). `ast` crashes in dump_ast (debug-only).
4. After the lexer+parser path is clean, drive bootstrap.sh to stage-2==stage-3.

## SESSION 2 (2026-05-24 cont., steps 84-88): memory solved + 5 codegen bugs fixed

The full-source obj now builds (~10 GB) and stage-2 links + runs (banner, file
read, size). Fixes landed this session, each a real self-host codegen/runtime bug
that stage-0 handled but the self-host codegen did not:

1. step 82 — field-access offset (field_idx*8 from struct_defs).
2. step 84 — O(n^2) compile memory: Lexer made non-@copy so its growing tokens
   array is shared (refcount) not deep-copied per token. (kryos_array_clone is a
   deep copy, not the O(1) rc-bump its doc claims.)
3. step 86 — cg_emit_struct_lit lost the struct pointer (RAX) across field loads
   when a field load called the runtime; now saved/reloaded like array_lit.
4. step 87 — operand share-on-clone aliasing: rvalue constructors (rv_use,
   rv_field, rv_index, rv_unop, rv_cast, rv_enum_*) stored operands that a later
   operand could overwrite; now cloned. Fixed `x = call_result` losing its value.
5. step 88 — string indexing s[i] was miscompiled as ARRAY indexing (KryosArray
   data@32, *8 stride) instead of string byte access; now routes strings to
   kryos_string_char_at. The lexer crashed on its first lex.src[lex.pos].

After all 5: examples 9/9; minimal loop-carried struct-capture repro returns 35;
stage-2 no longer crashes in tokenize.

### REMAINING: stage-2 still does not finish tokenize (not self-compiling yet)

`stage-2 ast tiny.kry` prints the header + "Size:" then hangs (does not reach
"Tokens:"). Narrowing was inconclusive because the compiler is buggy enough that
debug instrumentation in the self-host source SHIFTS or re-triggers codegen bugs
(adding markers changed a crash to a hang; an eprintln in dump_ast made stage-1
itself crash during codegen of the full source). Findings:
- NOT register allocation: forcing all-spill (KRYOS_RA_SPILL_ALL experiment) did
  not fix the hang.
- lex_advance and lex_at_end disassemble correctly (pos+1, field offsets right).
- The hang is in the tokenize/lex_scan_token path or dump_ast's token-count loop;
  guards placed in tokenize's loop and lex_scan_identifier's loop did NOT fire
  (so either those loops terminate and the hang is elsewhere, or the guards
  themselves miscompile).

This is a TAIL of self-host codegen bugs that surface sequentially; 5 are fixed,
more remain in the lexer/parser path. Each needs disassembly-level work because
in-source instrumentation is unreliable on the buggy compiler. The memory wall
that made the bootstrap impossible is gone; this is now incremental codegen
hardening.

---

# (historical) Stage-2 Bootstrap Blocker — Memory O(n^2) [RESOLVED 2026-05-24, step 84]

## STATUS: MEMORY BLOCKER RESOLVED. Gate is now blocker #2 (codegen miscompile).

### RESOLUTION (step 84)

Root cause of the O(n^2): `kryos_array_clone` (array.rs) is a **deep O(len) buffer
copy**, NOT the O(1) refcount-bump its own doc claims. Codegen calls it on `@copy`
struct construction AND field-read of array fields. The self-host stores the growing
`tokens` array in the `@copy` `Lexer` struct and rebuilds `Lexer` per token
(functional update), so each token deep-copied the whole `tokens` array twice
(field-read + construction) -> sizes 1,2,...,30560 -> O(n^2). (The `cap=1025,1026,...`
pairs signature; token array alone ~3.7 GB.) The runtime CANNOT use mutable globals
(no support), so the runtime's "big collections live in module-globals, not struct
fields" design assumption was violated.

**Fix: drop `@copy` from `Lexer`** (lexer.kry) so its array field is shared (refcount)
and grown in place via `push` -> O(n). Verified:
- examples 9/9; field_offsets exact (11/22/33/44).
- sub6 (5151 lines): **13169 MB -> 756 MB live (17x)**.
- full source (20787 lines): **>18 GB OOM -> ~9.7 GB, COMPLETES**, valid 1.2 MB obj.
- stage-2.exe links via `link_stage2.bat` (rt_shim_win.c + kryos_rt.lib) -> 1.22 MB.

Measurement tooling: `crates/kryos-rt/src/memstats.rs`, `KRYOS_MEMSTATS=1` (gated, zero
cost off). Shows arr/str/struct new/free counts, per-category bytes, big-array caps.

### REMAINING residual memory (not blocking; next optimization)

`Parser` is still `@copy` and holds growing arrays -> ~9 GB residual `arr_new`
(max_cap 104295). Same fix applies (drop `@copy` from Parser and other growing-array
structs) IF the parser doesn't rely on `@copy` value snapshots for backtracking
(verify first). Would bring full-source compile to ~1-2 GB.

### blocker #2 — DISASM EVIDENCE (step 86, 2026-05-24)

Disassembled stage-2's `tokenize` (dumpbin /disasm of kryos-stage2.obj). The
miscompile is concrete:

```
  call  lexer_new
  mov   rbx, rax      ; the lexer_new RESULT is captured in RBX
  xor   r12, r12      ; but `lex` lives in R12 and is ZEROED, not set from RBX
  jmp   <loop cond>
  ...: mov rcx, r12 ; call lex_at_end   ; loop reads lex from R12 (=0/garbage)
  ...  mov r12, r15                      ; loop body DOES update lex in R12
  ...  return path: mov rax, [r12]       ; lex.tokens read at WRONG offset (0, not 16)
```

`lex` is a **mutable, loop-carried local**: its definition (`let mut lex =
lexer_new(src)`) is assigned register **RBX**, but every USE inside the while
loop reads **R12**, and the connecting copy (`mov r12, rbx`) was emitted as
`xor r12, r12` (zero) — so the call result is lost and `lex` is 0/garbage in
the loop. Reading `lex.<field>` then dereferences garbage / wrong offsets
(observed: `lex.pos` read 61 = the src string's cap; `lex.tokens` read at
offset 0).

The lowering (`lower_let_stmt` -> `inst_assign(lex, rv_use(t))`), `cg_emit_call`
(moves RAX->dest), `cg_emit_instruction`, `cg_get_local_reg`/`cg_store_local`,
and `x86_emit_memop` (disp8/disp32) all read **correct on paper** — yet the
emitted code splits `lex` across two registers with a zeroing in between. So the
root cause is in **regalloc** (`regalloc.kry`): a loop-carried mutable local
gets inconsistent register assignments at its def vs its loop-body uses (the
linear-scan "back-edge blindness" the file's own comments mention a partial
workaround for). NEXT: dump the regalloc intervals for `tokenize` (instrument
ra to print local_id -> reg/spill_slot), find where `lex`'s def-interval and
loop-interval diverge, and unify them (or force-spill loop-carried locals so def
and uses share a stack slot).

NOT yet root-caused to a single line. Examples 9/9 don't exercise this because
they don't have struct-returning functions captured into loop-carried mutable
locals.

### (earlier framing) stage-1 miscompiles the large self-host source

`stage-1 ast tiny.kry` works perfectly (Tokens: 19, AST dumped, exit 0). But the
stage-2 it produces crashes (exit 139): `stage-2 ast/obj/check tiny.kry` prints the
command header then segfaults at the first real operation (file_read / tokenize);
`stage-2` with no args prints usage cleanly (exit 0). So stage-2's `main` dispatch
works, but the command handlers crash -> **stage-1's codegen miscompiles the large
self-host source** (scale-dependent: examples 9/9 compile fine via stage-1, but the
20k-line self-host does not). This is the long-standing "blocker #2" (large-`main`
spill/call-sequence-at-scale codegen bug), now cleanly reproducible and no longer
masked by the OOM. Repro: build stage-1, build stage-2 (extlink obj + link_stage2.bat),
then `kryos-stage2.exe ast tiny.kry` (crashes) vs `kryos-stage1.exe ast tiny.kry` (works).

---

## (historical) original O(n^2) analysis below

Two things happened on 2026-05-24 (shift steps 82-83):

1. **FIXED: field-access offset bug** (committed, step 82). `cg_emit_field_access`
   always emitted offset 0, so every multi-field struct field read returned the
   first field (`lx.pos` -> garbage pointer). Now the field index is resolved at
   lowering from `MirModule.struct_defs` via the receiver operand type and carried
   in `rv.field_idx`; codegen emits `field_idx * 8`. Verified: examples 9/9,
   `examples/field_offsets.kry` prints distinct `lit.a..d` and `param.a..d`
   (11/22/33/44) for both struct-literal-local and parameter receivers.

2. **DISCOVERED: the real stage-2 gate is a pre-existing memory blowup**, not the
   field bug. Prior sessions' "stage-2" binaries were built from a **stale/truncated
   `.obj`** — the watchdog had been killing stage-1 mid-emit at the old 3 GB cap, and
   a leftover obj from an earlier run was being linked. A clean rebuild reveals
   stage-1 cannot produce the full-source obj at all.

## Evidence

`stage-1 obj <source>` peak working set (self-measured, polled 1 Hz):

| input                          | lines | `ast` peak | `obj` peak | completes? |
|--------------------------------|-------|-----------|-----------|------------|
| examples/*.kry                 | ~30   | low       | low       | yes (9/9)  |
| runtime+token+lexer (sub3)     | 1581  | fast      | ~OK       | yes        |
| 5000 trivial `let v=N`         | 5003  | 5113 MB   | —         | yes        |
| 5000 trivial, `KRYOS_USE_REALLOC=1` | 5003 | 2839 MB | —     | yes        |
| +ast+parser (sub6)             | 5151  | >13 GB    | >13 GB    | **OOM**    |
| +ast+parser (sub6), realloc    | 5151  | —         | **14.8 GB** | **yes, no crash** |
| half (runtime..lower)          | 11625 | >16 GB    | >16 GB    | **OOM**    |
| full (16 files)                | 20782 | —         | >18 GB    | **OOM**    |
| full (16 files), realloc       | 20782 | —         | ~8.6 GB then **CRASH** | exit 127 |

- ~1 MB/statement on TRIVIAL code; but **REAL code is ~3 MB/line** (sub6 = 5151
  lines completed at 14.8 GB *with realloc*). At O(n^~2) the full 20 782-line source
  would need **>100 GB regardless of the crash** — memory efficiency, not the crash,
  is the primary wall. No env var or runtime tweak fixes this; the retain/release
  leak (arrays never reaching rc=0) must be fixed so memory is reclaimed during the
  compile.
- realloc is **safe through sub6** (5 k lines, frontend files: runtime/token/lexer/
  ast/parser) — completes cleanly, "Type check: OK, MIR functions 398, Object file
  written". It only **crashes on the full source**, so the remaining heap corruption
  is introduced by a later file (types/mir/lower/optimize/regalloc/x86/codegen/elf/
  coff/linker/main). That narrows the OOB-write hunt to those files.
- **Confirmed pre-existing**: a stage-1 built from the step-81 source (before the
  field fix) OOMs identically (16385 MB on half). The field fix is NOT the cause.

## Root cause

The runtime (`crates/kryos-rt`) **deliberately leaks** to stay alive under heap
corruption, and the full self-host source still corrupts the heap:

- `array.rs` `kryos_array_push` grow path defaults to **alloc-copy-leak** (the old
  data buffer is never freed; line ~122 "H26: default to alloc-copy-leak grow path").
  `KRYOS_USE_REALLOC=1` switches to a realloc (leak-free) grow path.
- `kryos_array_free` DOES free by default (only ~40 B header leaked);
  `KRYOS_LEAK_ON_ZERO` is opt-in/off. So frees are not the primary leak.
- The deeper issue (`array.rs:360-363`): codegen retain/release is **not balanced**
  ("the full fix requires auditing codegen to emit retain at every array pointer
  copy ... tracked as separate next-shift work"). Over-retained arrays never reach
  rc=0, so even the working free path can't reclaim them.

Why we can't just flip the leak off:
- `KRYOS_USE_REALLOC=1` cuts trivial-5k from 5113->2839 MB and does NOT crash on
  trivial code, **but CRASHES on the full self-host source** (exit 127, only the
  header prints, ~8.6 GB). HeapReAlloc probes adjacent heap-block headers; a
  remaining out-of-bounds write somewhere in the self-host's generated code corrupts
  heap metadata, and realloc/free are the canaries that surface it. The leak path
  masks it by never touching freed/old blocks.

So the true blocker is **remaining heap corruption** in stage-1's compilation of the
full source (an OOB store in some codegen path), plus the unbalanced retain/release
that forces the leak. The field-access fix removed one source of garbage pointers
but not all corruption.

## Candidate fixes (next shift, in order of safety/value)

1. **Find the remaining OOB write.** Build `kryos_rt` with debug assertions or a
   guard allocator; run stage-1 on sub6 with `KRYOS_USE_REALLOC=1` and catch the
   first corrupt-header panic (`kryos_array_push` already panics on corrupt headers —
   widen that check to fire earlier, log the allocation that overflows). Likely an
   uncoerced i64->i32 store or an array/struct write past its length, similar to the
   step-79/80 fixes.
2. **Balance codegen retain/release** (stage-0, `crates/kryos-codegen-llvm` +
   `kryos-mir` drop insertion) so arrays reach rc=0 and free. This is the
   "separate next-shift work" the runtime comments defer. Delicate; gate every step
   on examples 9/9 + bootstrap 16/16 to catch use-after-free.
3. **`free-on-grow when ref_count<=1`** in `kryos_array_push` (alloc+copy+dealloc-old,
   not realloc) — only safe once (1)/(2) land; risks use-after-free if rc is
   under-counted.

## SAFETY — this project has OOM'd the machine before

- Always run `scratch/kryos-watchdog.ps1 -CapMB <N>` before any full-source stage-1
  run. With ~33 GB free, `-CapMB 16384` is safe headroom. The leak path needs
  >18 GB for the full obj and was never observed to complete — do NOT raise the cap
  chasing a completion; fix the leak instead.
- `KryosLeakGuard` service (auto-kills kryos-stage*.exe >2 GB) should stay Running.
  `KryosTwin` must stay Stopped (it auto-runs the 35 GB self-host build).

## How to reproduce

```bash
cd compiler
# build stage-1 (safe, stage-0 is the Rust compiler)
KRYOS_NO_ASLR=1 ./target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership
cp -f target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
# reproduce the OOM (under watchdog!) — sub6 is the smallest reliable repro
cat self-host/{runtime,token,lexer,ast,parser}.kry \
  | grep -vE '^use (token|lexer|ast|parser|types|mir|lower|optimize|regalloc|x86|codegen|elf|coff|linker|runtime|main)$' > /tmp/sub6.kry
KRYOS_SKIP_TYPES=1 KRYOS_NO_ASLR=1 ./target/bootstrap/kryos-stage1 obj /tmp/sub6.kry -o /tmp/sub6.obj
```
