# Overnight Work Log — 2026-05-21

**Start**: 01:10 AKST
**Target report**: 09:00 AKST (~7h50m of work)
**Branch**: feat/overnight-2026-05-21

## Goals (rough priority)

1. **Operand::Local retain audit** — last unaudited retain path. Should
   eliminate parser+lower flakes if hypothesis holds.
2. **Flip `*_free` to real refcount-decrement-and-dealloc** — eliminates
   the 80MB per-invocation leak. Requires step 1 first.
3. **Investigate parser+lower specifically** — what's unique that flakes?
4. **Stage-2 link infrastructure** — multi-`.obj` linking proof.
5. **Performance benchmarking** — stage-1 vs stage-0 on real workloads.
6. **Language design analysis** — write up what's distinctive about Kryos.
7. **Documentation polish** — fill in any gaps.

Notes will be appended as I go.

---
