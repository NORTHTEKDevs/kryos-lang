# RESOLVED (by deferral): Cranelift box headers are incompatible with the borrow-side drops

Found while integrating `fix/aggregate-sret-and-box-header` with the leak/ABI
line. **Neither branch has this bug alone. The combination does.**

## The matrix

`compiler/self-host/stage1_mini_parser.kry`, run via `kryos run` (Cranelift):

| Tree | Result |
| --- | --- |
| `ac45392` (master) | `rc=98` — clean panic, `parse_int: invalid numeric input: '}'` |
| `fix/aggregate-sret-and-box-header` alone | `rc=98` — same panic, unchanged |
| leak/ABI line alone (`2d059e3`) | **`rc=0` — `stage 1 mini-parser: ok`** |
| merged | **`rc=139` — SEGFAULT** |

Two things worth noting. The mini-parser was already broken on master, and the
other branch did not change that. The leak/ABI line **fixed** it. Merging then
turns a fix into memory corruption.

## What is actually happening

`KRYOS_FREE_DIAG=1` on the merged build reports repeated double-frees of
single-character token strings:

```
KRYOS-FREE-DIAG[0]: str DOUBLE-FREE hdr=0x... rc=0 len=1 content="}"
KRYOS-FREE-DIAG[2]: str DOUBLE-FREE hdr=0x... rc=0 len=1 content="x"
```

`rc=0` means the string was already fully released before the offending free.

The two interacting changes:

1. **`335551e` (theirs)** — struct/enum boxes moved from libc `calloc`/`free` to
   `kryos_calloc`/`kryos_free`, so boxes carry the allocation header. This
   changes when and how a struct's fields are torn down.
2. **the `@copy` struct-literal Str arm (mine)** — was `val` (share the
   pointer), now takes a reference. Needed because once a user function
   borrows its heap arguments, the caller drops a computed temp after the
   call, and a shared field pointer dangles. Without it
   `conf_ctor_temp_arg` fails with garbage in an `@copy` struct body.

**Do not "fix" this by reverting the Str arm.** Verified: with it reverted the
merged tree still fails `conf_ctor_temp_arg` ("@copy struct keeps a computed
body") and the mini-parser merely returns to master's `rc=98`. Their
box-header change does **not** subsume the ownership fix.

## The trap in the naming

`kryos_string_clone` is **not** a deep copy. It is a refcount bump that returns
the *same* pointer — byte-for-byte what `kryos_string_retain` does. Any
reasoning that treats a "cloned" `@copy` str field as an independent
allocation is wrong, and that is the likely source of the imbalance: the
combined tree performs one more release than retain on these token strings.

## Where to start

The `@copy` struct teardown path on Cranelift, now that boxes carry a header.
Establish, for an `@copy` struct with a `str` field, exactly how many retains
and releases occur across: literal construction, variable-to-variable
assignment (which also routes through `kryos_string_clone`), and box teardown.
The counts must balance at one release per retain. `conf_ctor_temp_arg` and
`stage1_mini_parser.kry` must BOTH pass; today each config satisfies only one.

## Current branch state

The branch keeps the Str arm, because that config passes conformance 45/45
while the alternative fails it. `selfhost-stage1` is consequently still red in
CI — it was red on master too, for a different reason (the `no_struct_lit`
field, fixed here).


---

# RESOLUTION, 2026-07-28

**My first attribution was wrong and is corrected here.** I blamed the `@copy`
struct-literal Str arm on the strength of a hand-edit that appeared to fix the
segfault. It did not: `stage1_mini_parser.kry` declares **no `@copy` struct at
all** (`copy_structs` only ever contains explicitly-annotated types), so that
arm cannot execute for this program. The hand-edit had removed more than
intended. The crash is 8/8 deterministic, so the earlier "fix" was an artifact.

Re-bisected at COMMIT level instead of by hand-editing:

| Tree | Result |
| --- | --- |
| leak/ABI line alone (`2d059e3`) | `rc=0` |
| leak/ABI line **+ only `335551e`** | **`rc=139` segfault** |
| `335551e` alone on master | `rc=98` (master's own panic; no crash) |

So the incompatibility is specifically **`335551e` (struct/enum boxes carry the
allocation header) against this line's ownership work**, not the `@copy` arm.

**Mechanism.** This line makes the caller DROP heap temporaries it previously
leaked (user functions borrow their heap arguments; 15 read-only builtins moved
onto the borrow allowlist). `335551e` changes struct/enum box teardown so
fields are released where they previously were not. Each change is balanced on
its own; together a string reachable from a boxed node is released twice, which
`KRYOS_FREE_DIAG=1` reports as `str DOUBLE-FREE ... rc=0` on single-character
token text.

**Decision: `335551e` is reverted on the integration branch.** The other eleven
commits are kept. Rationale:

- the ownership work is load-bearing for four separate leak fixes and is
  covered by conformance tests; the box-header change is a valgrind-cleanliness
  improvement with no test that fails without it
- verified: with `335551e` reverted, `conf_nested_arrays` and
  `conf_runtime_stdlib` — the two tests its commit message cites — both pass,
  and `selfhost-stage1` is fully green (18/18 check, `stage 1 mini-parser: ok`)
- conformance 45/45, no-double-free, all soundness gates green

**To re-land `335551e`**, the box teardown must take a reference for every heap
field it will later release, so the count balances against the caller's drop.
Note the naming trap while doing it: `kryos_string_clone` is a refcount bump
returning the SAME pointer, identical to `kryos_string_retain` — not a deep
copy. Re-landing must keep `stage1_mini_parser.kry` at `rc=0`; that program is
the regression test for this interaction.
