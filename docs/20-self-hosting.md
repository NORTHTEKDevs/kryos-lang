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
  `compiler/self-host/*.kry` (21,652 lines of Kryos source across 16 modules).
- **Stage 2**: A second-generation Kryos-compiled compiler. Built by
  stage 1 from the same self-host sources. Each module is
  individually verified to compile through stage 1.

The full chain demonstrates that Kryos source can produce a working
Kryos compiler. The canonical proof is the byte-identical **fixed point**
(stage-3 == stage-4, SHA-256; stage-2 differs because stage-1 uses a
different backend than stage-2 and later). The standalone per-module compile
check (`test_bootstrap.sh`) passes **16 of 16** modules, stable across
consecutive runs.

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
# Expected: PASS: 16 / 16

# Verify the byte-identical fixed point (the canonical criterion):
bash self-host/bootstrap-win.sh
# Expected: stage-3 == stage-4 (SHA-256); stage-2 differs (different backend)
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
| `token` | 353 | 13K | Token kinds + Token struct |
| `lexer` | 624 | 21K | Source → tokens |
| `ast` | 687 | 19K | AST node types + constructors |
| `parser` | 2,911 | 107K | Tokens → AST |
| `types` | 2,340 | 84K | Type checker + inference |
| `mir` | 1,305 | 40K | Mid-level IR |
| `lower` | 3,406 | 125K | AST → MIR lowering |
| `optimize` | 1,585 | 47K | MIR-level optimizations |
| `regalloc` | 1,504 | 49K | Register allocation |
| `x86` | 764 | 25K | x86_64 instruction selection |
| `codegen` | 2,010 | 77K | MIR → machine code |
| `elf` | 694 | 23K | ELF object writer (Linux) |
| `coff` | 574 | 21K | COFF object writer (Windows) |
| `linker` | 1,525 | 46K | Driver for `link.exe` / `cc` |
| `runtime` | 613 | 17K | Runtime ABI bindings |
| `main` | 757 | 24K | CLI entry point |
| **total** | **21,652** | **736K** | — |

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

> **Current status (v1.0.0):** the standalone per-module check
> (`test_bootstrap.sh`) passes **16/16**, stable across consecutive runs.
> The byte-identical bootstrap **fixed point** (stage-3 == stage-4,
> `bootstrap-win.sh`) holds; stage-2 differs because stage-1 uses a
> different backend. The figures below are from release 4.43.0-rc.4 and
> are kept for history.

Empirical stability over 30-run characterization with release 4.43.0-rc.4:

- **Mean PASS / 16**: 15.93
- **Best**: 16 / 16
- **Worst**: 15 / 16
- **Perfect-run rate**: 28 / 30 (93%)
- **Stable modules**: 14 / 16 (always pass)
- **Flaky modules**: 2 / 16 (`parser` and `lower`, ~97% pass rate each)

The remaining flakes at the time affected the two largest self-host modules
(`parser` and `lower`). They were small heap-state issues; both modules now
pass stably at 16/16 after the MIR aggregate-lowering fixes.

## What is verified

The full bootstrap chain is now automated by `bootstrap-win.sh`:

1. Stage 1 produces `.obj` files for all 16 modules (verified ✓)
2. The 16 `.obj` files link into a stage-2 `.exe` (verified ✓)
3. Stage 2 compiles the concatenated self-host source to produce a
   stage-3 `.obj`, which links into stage-3 `.exe` (verified ✓)
4. Stage 3 produces stage-4 `.obj`; SHA-256 of stage-3 `.obj` ==
   SHA-256 of stage-4 `.obj` — the fixed point (verified ✓)

Stage-2 `.obj` differs from stage-3 `.obj` because stage-1 (built by
the Rust/Cranelift stage-0) uses a different backend than stage-2 and
later. The fixed point is stage-3 == stage-4.

## Next steps

In rough priority order:

1. **Codegen retain-emission audit.** The current `*_free` no-ops are
   diagnostic, not production. Auditing `RValue::Field`, `Operand::Local`
   evaluation, function argument passing, and pattern destructuring to
   ensure every heap-pointer copy is matched by a retain — then flipping
   `kryos_array_free`/`kryos_string_free`/`kryos_map_free` to do real
   refcount-decrement-and-dealloc. Eliminates the 80 MB per-invocation
   leak.
2. **Stage-2 fixed point.** Currently stage-2 != stage-3 because stage-1
   uses the Cranelift backend. Investigate whether stage-2 can reproduce
   stage-1's output; this would extend the fixed-point chain one step earlier.
3. **Linux / macOS bootstrap.** `bootstrap-win.sh` is Windows-only;
   port the fixed-point check to the ELF path.

## See also

- [STAGE2_BLOCKER.md](../compiler/self-host/STAGE2_BLOCKER.md) — the
  original blocker analysis (resolved 2026-05-20) and the final fix
  description.
- [REPORT_2026-05-20.md](../.shift/REPORT_2026-05-20.md) — end-of-day
  report covering the methodology and 42 hypotheses tested to reach
  this state.
- [CHANGELOG.md](../CHANGELOG.md) — 4.43.0-rc.2 → rc.4 entries with
  per-step commit references.
