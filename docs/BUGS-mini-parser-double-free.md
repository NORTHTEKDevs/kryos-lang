# Tracked: 7 pre-existing double-frees in `stage1_mini_parser.kry`

Not introduced by any recent branch. Measured with `KRYOS_FREE_DIAG=1`, which
never deallocates and instead reports every free of an already-released header:

| Tree | double-frees | normal-mode result |
| --- | --- | --- |
| `ac45392` (master) | **60** | `rc=98` panic: `parse_int: invalid numeric input: '}'` |
| `fix/aggregate-sret-and-box-header` | **10** | `rc=98`, same panic |
| leak/ABI line alone | **7** | `rc=0` — completes |
| both integrated | **7** | `rc=139` SEGFAULT |

Two things this table establishes.

**Master's `parse_int '}'` panic is not a parser bug — it is the corruption.**
Under `KRYOS_FREE_DIAG=1` (no deallocation), master runs the program to
completion with `rc=0`. The panic only appears when frees are real, i.e. a
released token string is reused and its text reads back as the wrong
character. Anyone "fixing the mini-parser's parse logic" is chasing a symptom.

**The recent work reduced the count 60 -> 7, and none of it introduced the
bug.** The segfault in the integrated tree is the *remaining* 7 becoming fatal:
`335551e` moves struct/enum boxes onto `kryos_calloc`/`kryos_free`, so a
double-free is now real pointer arithmetic on a header rather than a forgiving
libc `free`.

## What is being double-freed

Single-character token texts and one small array:

```
str DOUBLE-FREE hdr=0x... rc=0 len=1 content="2"
str DOUBLE-FREE hdr=0x... rc=0 len=1 content="x"
array DOUBLE-FREE rc=0 len=4 cap=1
```

`rc=0` means the header was already fully released before the offending free.

## Where to look

The path is `cur_text(p)` -> `alloc_node(..., t, ...)` -> `push(np.str_args, s)`.
Each step is individually accounted for — a struct field read retains on
Cranelift, and `push` retains its value argument — so the imbalance is in how
those compose across the `Parser` struct being threaded by value through every
`p_*` call. Count retains and releases for one token string end to end; they
must balance at one release per retain.

Useful facts before starting:

- `kryos_string_clone` is a refcount bump returning the SAME pointer, identical
  to `kryos_string_retain`. It is not a deep copy. Reasoning that treats a
  "clone" as an independent allocation is wrong.
- `KRYOS_FREE_DIAG=1` makes the program survive, which is the fastest way to
  confirm a suspected fix actually changes the COUNT rather than just hiding
  the crash.
- `stage1_mini_parser.kry` reaching `rc=0` in normal mode is the acceptance
  test. The `selfhost-stage1` CI job runs exactly that.

## Current CI position

`selfhost-stage1` is red on this account, as it was on master (for a different
reason there — the `no_struct_lit` field, fixed). Every other job is green:
Linux, macOS, Windows, docs-examples, fuzz, quickstart-e2e, registry-smoke,
wasm-smoke. Master's baseline was 4 of 9 green.

Dropping `335551e` is NOT the answer and was tried: it makes this job pass but
breaks Linux and macOS (`test_re_anchors_captures`, `test_tracked_generic`),
because that commit is fixing real heap corruption there.
