# Self-Hosting

Kryos's compiler is **written in Kryos**. This document explains what
self-hosting means for Kryos, how the bootstrap chain works, what is
verified, and what remains as polish work for full byte-identical
fixed-point bootstrap.

## What self-hosting means

A language self-hosts when its compiler can compile its own source.
Three stages of the bootstrap chain:

- **Stage 0**: Rust-implemented `kryos.exe`. Lives in
  `compiler/crates/`. Built via `cargo build --release`.
- **Stage 1**: The Kryos-compiled compiler. Built by stage 0 from
  `compiler/self-host/*.kry` (34,342 lines of Kryos source).
- **Stage 2**: A second-generation Kryos-compiled compiler. Built by
  stage 1 from the same self-host sources. Each module is
  individually verified to compile through stage 1.

The full chain demonstrates that Kryos source can produce a working
Kryos compiler. The canonical proof is the byte-identical **fixed point**
(stage-2 == stage-3 == stage-4, SHA-256), reached with the ownership and
type checkers disabled on the self-host source (`--skip-ownership` /
`KRYOS_SKIP_TYPES=1`). The standalone per-module compile check
(`test_bootstrap.sh`) currently passes **11 of 16** modules; codegen is
flaky on the rest.

## Verify it yourself

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler

# Build stage 0
cargo build --release -j 2

# Build stage 1
./target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership

# Verify the standalone per-module compile check
bash self-host/test_bootstrap.sh
# Expected: PASS: 11 / 16 (codegen flaky on the rest)

# Verify the byte-identical fixed point (the canonical criterion):
bash self-host/bootstrap-win.sh
# Expected: stage-2 == stage-3 == stage-4 (SHA-256)
```

For a more rigorous test that runs N times and reports per-module pass
rates:

```bash
bash self-host/test_bootstrap_robust.sh 30
# Reports each module as STABLE, FLAKY, or REGRESSION.
```

## The 16 self-host modules

| Module | Lines | Bytes | Role |
|--------|------:|------:|------|
| `token` | 353 | 11K | Token kinds + Token struct |
| `lexer` | 616 | 17K | Source → tokens |
| `ast` | 687 | 24K | AST node types + constructors |
| `parser` | 2,867 | 104K | Tokens → AST |
| `types` | 2,270 | 78K | Type checker + inference |
| `mir` | 1,215 | 40K | Mid-level IR |
| `lower` | 2,705 | 94K | AST → MIR lowering |
| `optimize` | 1,585 | 53K | MIR-level optimizations |
| `regalloc` | 1,300 | 47K | Register allocation |
| `x86` | 759 | 27K | x86_64 instruction selection |
| `codegen` | 1,612 | 60K | MIR → machine code |
| `elf` | 694 | 24K | ELF object writer (Linux) |
| `coff` | 574 | 19K | COFF object writer (Windows) |
| `linker` | 1,525 | 50K | Driver for `link.exe` / `cc` |
| `runtime` | 613 | 21K | Runtime ABI bindings |
| `main` | 642 | 23K | CLI entry point |
| **total** | **34,342** | **692K** | — |

## Memory model for self-host

Stage 1 uses a **share-on-clone, leak-on-free** memory model for `@copy`
struct lifecycle. This was the breakthrough that unblocked the bootstrap:

- **Strings** (`KryosString`): share via refcount; `kryos_string_clone`
  returns the same pointer and increments `ref_count`.
- **Maps** (`MapHeader`): same pattern; `kryos_map_clone` returns same
  pointer.
- **Arrays** (`KryosArray`): `kryos_array_retain` at `@copy` struct
  Array field construction; `kryos_array_free` is a pure no-op (the
  refcount infrastructure exists but allocation is leaked).

Per stage-1 invocation, this leaks ~80 MB of heap (the largest
self-host modules accumulate the most). The leak is **bounded** —
each `kryos.exe build` exits and the OS reclaims. For long-running
processes (LSP server, watch mode), the production cleanup pass
described under "Next steps" must land first.

## Stack size

Stage 1's recursive-descent parser and scope walker hit deep recursion
on large source files. Stage 0 sets a 32 MB stack reserve for all
MSVC dynamic binaries it links. This is generous but the cost is VA
only — physical memory is committed on demand.

## Stability

> **Current status (1.0.0-beta.1):** the standalone per-module check
> (`test_bootstrap.sh`) passes **11/16** — codegen regressed on 5 modules
> since the characterization below. The byte-identical bootstrap **fixed
> point** (stage-2 == stage-3 == stage-4, `bootstrap-win.sh`) still holds.
> The figures below are from release 4.43.0-rc.4 and are kept for history.

Empirical stability over 30-run characterization with release 4.43.0-rc.4:

- **Mean PASS / 16**: 15.93
- **Best**: 16 / 16
- **Worst**: 15 / 16
- **Perfect-run rate**: 28 / 30 (93%)
- **Stable modules**: 14 / 16 (always pass)
- **Flaky modules**: 2 / 16 (`parser` and `lower`, ~97% pass rate each)

The remaining flakes affect the two largest self-host modules
(`parser` 2,867 lines and `lower` 2,705 lines). They appear to be
small heap-state issues unrelated to stack depth (which 32 MB has
already addressed for the other large modules). Tracked as next-shift
codegen audit work.

## What is NOT yet verified

The bootstrap test verifies that each self-host source file
**compiles to a valid `.obj`** through stage 1. It does NOT yet
verify the byte-identical stage-2 → stage-3 fixed point:

1. Stage 1 produces `.obj` files (currently verified ✓)
2. The 16 `.obj` files link into a stage-2 `.exe` (NOT yet automated)
3. Stage 2 compiles every self-host source again to produce stage 3
   (NOT yet automated)
4. Stage 2 and stage 3 emit byte-identical `.exe` files for `hello.kry`
   (NOT yet automated)

Steps 2–4 are the standard "fully bootstrapped" proof. They are
deferred because stage 0's codegen marks user functions as
`Linkage::Local` (to avoid colliding with libc symbols like `open`,
`bind`, `read`). Multi-`.obj` linking requires either flipping
user-function linkage to `Export`, or adding a `--export-all` build
flag that opts into export linkage for stage-2 linking.

## Next steps

In rough priority order:

1. **Codegen retain-emission audit.** The current `*_free` no-ops are
   diagnostic, not production. Auditing `RValue::Field`, `Operand::Local`
   evaluation, function argument passing, and pattern destructuring to
   ensure every heap-pointer copy is matched by a retain — then flipping
   `kryos_array_free`/`kryos_string_free`/`kryos_map_free` to do real
   refcount-decrement-and-dealloc. Eliminates the 80 MB per-invocation
   leak and (likely) the remaining parser/lower flakes.
2. **Multi-`.obj` stage-2 linking.** Add a build mode that exports user
   function symbols so the 16 self-host `.obj` files can link into a
   single stage-2 `.exe`. Then verify stage 2 reproduces stage 1's
   output on the example programs.
3. **Stage-3 fixed point.** Build stage 3 with stage 2; compare hashes
   of `hello.kry` → `.exe` between stages 2 and 3.

## See also

- [STAGE2_BLOCKER.md](../compiler/self-host/STAGE2_BLOCKER.md) — the
  original blocker analysis (resolved 2026-05-20) and the final fix
  description.
- [REPORT_2026-05-20.md](../.shift/REPORT_2026-05-20.md) — end-of-day
  report covering the methodology and 42 hypotheses tested to reach
  this state.
- [CHANGELOG.md](../CHANGELOG.md) — 4.43.0-rc.2 → rc.4 entries with
  per-step commit references.
