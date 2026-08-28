# The Kryos Book

A cover-to-cover book for a working programmer learning Kryos, in a fixed
reading order. Every chapter builds only on concepts earlier chapters
introduced.

This is not a replacement for the other docs in this repo -- it's the spine
that ties them together:

- [`docs/learn/tour.md`](../tour.md) is a 30-minute tour if you just want a
  taste before committing to the book.
- [`docs/learn/cookbook/`](../cookbook/) has 27 standalone recipes for things
  people actually build -- read these after the book, as reference.
- [`docs/19-language-reference.md`](../../19-language-reference.md) is the
  spec -- the grammar and semantics are the ground truth this book teaches
  from and never contradicts.
- The numbered `docs/NN-*.md` chapters are deep references on single topics.
  Where a book chapter's subject already has a good `docs/NN-*.md` reference,
  the book chapter teaches the mental model with a worked example and links
  out for the exhaustive version, instead of duplicating it.

Chapters not yet written are listed so the table of contents is complete and
the mapping to existing reference docs is decided up front; they link out to
the existing reference material until their book chapter lands. Follow
[`STYLE.md`](STYLE.md) when writing one.

## Part I -- Foundations

| # | Chapter | Status |
|---|---|---|
| 01 | [Hello Kryos & the toolchain](01-hello.md) | written |
| 02 | [Values & types](02-values-and-types.md) | written -- summarizes the type half of [`docs/02-variables-and-types.md`](../../02-variables-and-types.md) (that reference chapter predates the current dialect in places -- e.g. it claims `i32` is the default integer and shows `none`/no-`Option` -- this book chapter is the corrected, current version; do not treat the two as interchangeable) |
| 03 | [Bindings](03-bindings.md) | written -- summarizes the `let`/`let mut` half of [`docs/02-variables-and-types.md`](../../02-variables-and-types.md), split out for teaching order: types before binding rules. Also covers the `-`/`(`/`[` line-continuation trap (CLAUDE.md hard rule 1) |
| 04 | [Functions](04-functions.md) | written -- summarizes [`docs/03-functions.md`](../../03-functions.md) (that reference chapter's closure section is pre-dialect-shift and not reflected here); this book chapter's core content is the borrow/ownership-transfer split between `str`/`[T]`/`map` params, struct params, and scalar params, including the LEDGER item 3 struct-argument leak |
| 05 | [Control flow](05-control-flow.md) | written -- summarizes [`docs/04-control-flow.md`](../../04-control-flow.md) (that reference chapter's "Coming from Rust" section overclaims `match` as value-only patterns; current `match` supports tuple/or/guard patterns, covered here); also covers `comptime {}` as an expression-position block under "Blocks are expressions" (full depth: [`docs/11-comptime.md`](../../11-comptime.md)) since no standalone book chapter is allocated to it in this outline |

## Part II -- Data

| # | Chapter | Status |
|---|---|---|
| 06 | [Structs & enums](06-structs-and-enums.md) | written -- summarizes [`docs/05-structs-and-enums.md`](../../05-structs-and-enums.md); also covers the bare-variant name-collision gotcha, the tuple-variant-only rule, and why struct-style variants (`A { x: i64 }`) are rejected |
| 07 | [Collections](07-collections.md) | written -- new material: `[T]`, `map<K, V>`, tuple, `contains()`'s str/map-only scope, and the `push`-returns-reassign rule (including the aliasing hazard; the former enum-array-push leak, LEDGER item 45, is FIXED); `std::collections` (`List`/`Stack`/`Queue`/`Deque`/`Dict`) is not yet a standalone section |
| 08 | [Strings](08-strings.md) | written -- new material: UTF-8 byte-vs-codepoint model, interpolation and literal-brace escaping, the `base64_encode`/`chr`/`byte_at` latin-1 byte-buffer caveat, `string_builder` for O(n) concat, and the substr-mid-codepoint panic; `std::bytes` is not yet a standalone section |
| 09 | [Generics & traits](09-generics-and-traits.md) | written -- summarizes [`docs/08-traits-and-generics.md`](../../08-traits-and-generics.md); also covers the `dyn Trait` container limitation (clean `E0110`) and its enum-and-`match` workaround |

## Part III -- The Kryos difference

| # | Chapter | Status |
|---|---|---|
| 10 | [Ownership & ARC](10-ownership-and-arc.md) | written -- summarizes [`docs/06-ownership.md`](../../06-ownership.md); deepens it with worked examples for arrays, structs, and multi-binding chains showing share-on-call vs copy-on-assignment side by side, plus the design rationale for why the two boundaries differ and the LEDGER item 3 struct-argument leak |
| 11 | [Capabilities](11-capabilities.md) | written -- the headline feature, deepest chapter. Full soundness history and precision-cost data lives in [`docs/10-capabilities.md`](../../10-capabilities.md) and [`docs/capability-roadmap.md`](../../capability-roadmap.md); this chapter teaches the model and links out to those for the exhaustive version |
| 12 | [Error handling](12-error-handling.md) | written -- summarizes [`docs/07-error-handling.md`](../../07-error-handling.md); covers `Result`/`throw` side by side, the always-str catch value, and the hard line between a catchable `throw` and an uncatchable panic (exit 98 implicit / 101 for `throw`/explicit `panic()`) |

## Part IV -- Concurrency

| # | Chapter | Status |
|---|---|---|
| 13 | [Concurrency: spawn/channels/actors](13-concurrency.md) | written -- summarizes the sync half of [`docs/09-concurrency.md`](../../09-concurrency.md); also covers `std::sync`'s `Mutex`/`AtomicInt`, and states two confirmed hang/deadlock caveats (LEDGER item 46: a two-caller `WaitGroup.wg_wait` only releases the first waiter, and a shared mutating closure's lock held across a coop-yield point deadlocks) |
| 14 | [Async](14-async.md) | written -- summarizes the async/await half of [`docs/09-concurrency.md`](../../09-concurrency.md), split out for teaching order: sync concurrency before async; states the "`await` is a yield point, not a future combinator" limit and Chapter 13's coop-yield/closure-lock deadlock as it applies to `coop_spawn` |

## Part V -- Ecosystem

| # | Chapter | Status |
|---|---|---|
| 15 | [Modules & packages](15-modules-and-packages.md) | written -- summarizes the CURRENT module/package system (the old [`docs/12-modules-and-packages.md`](../../12-modules-and-packages.md) predates the current dialect in several places -- e.g. it shows `use m as alias` syntax that is now a parse error, and a stale `kryos.toml`/CLI shape -- this book chapter is the corrected, current version, verified against a live `kryos pkg init`/`add`/`install` run); covers the flat-namespace no-aliasing rule and the local-path-dependency entry-file-naming gotcha (must match the package name, not a fixed `lib.kry`) |
| 16 | [The standard library tour](16-stdlib-tour.md) | written -- new material: a guided walk through the 66-module stdlib by category (see the table in this repo's `CLAUDE.md`); full per-symbol reference is [`docs/STDLIB.md`](../../STDLIB.md) |
| 17 | [Building & testing real programs](17-building-and-testing.md) | written -- new material: project layout, `kryos.toml`, `kryos test`, `kryos fmt`, `kryos pkg`, capabilities in a real project (`kryos audit`), ties together [`docs/01-getting-started.md`](../../01-getting-started.md)'s CLI reference with the cookbook |

## Part VI -- Depth

| # | Chapter | Status |
|---|---|---|
| 18 | The backends: Cranelift/LLVM/wasm (`18-backends.md`) | planned -- summarizes [`docs/15-codegen.md`](../../15-codegen.md); cross-compilation ([`docs/18-cross-compilation.md`](../../18-cross-compilation.md)) folds in here as the same "one source, multiple targets" topic |
| 19 | FFI & unsafe (`19-ffi-and-unsafe.md`) | planned -- summarizes [`docs/13-ffi.md`](../../13-ffi.md) and [`docs/17-unsafe-audit.md`](../../17-unsafe-audit.md) |
| 20 | Idioms & pitfalls (`20-idioms.md`) | planned -- new material: the named traps, drawn from this repo's `CLAUDE.md` gotcha list and [`tools/loop/LEDGER.md`](../../../tools/loop/LEDGER.md)'s open items -- the book's honest "here's what will bite you" chapter |

## Not part of the core path

Two existing reference chapters are compiler-internals material rather than
language-teaching material, and are not mapped to a book chapter:
[`docs/14-ai-runtime.md`](../../14-ai-runtime.md) (experimental AI-runtime
surface) and [`docs/20-self-hosting.md`](../../20-self-hosting.md) (the
self-hosting compiler effort). Both are linked from Part VI chapters where
relevant instead of getting their own book chapter. [`docs/16-integer-overflow.md`](../../16-integer-overflow.md)'s
content is folded into Chapter 20's idioms/pitfalls material (it's a named
trap, not a standalone teaching topic) rather than getting its own chapter.

## Known, documented limitations this book will not hide

Kryos has real, open limitations, tracked in
[`tools/loop/LEDGER.md`](../../../tools/loop/LEDGER.md). The book states these
plainly where relevant, per `STYLE.md`'s honesty rule, instead of omitting
them. The ones with the widest teaching surface:

- **Capability precision cost** (LEDGER item 41): 41 of 75 enumerated
  legitimate pure-closure shapes require `@capabilities(all)` under the
  fail-closed design -- Chapter 11 covers this.
- **Struct-argument leak** (LEDGER item 3): passing a struct with heap
  fields across a call boundary leaks ~86MB per 1M calls -- open design
  note, no fix shipped. Relevant to Chapters 6 and 10.
- **`any` type erasure** (LEDGER item 6 / CLAUDE.md gotcha #22): `any` has
  no runtime type tag; `to_string`/`format` on a `str`/`f64` value routed
  through a direct `any` slot mis-renders it. Relevant to Chapters 7 and 9.
- **`std::result::to_array<T>` needs an explicit annotation** (LEDGER item
  40c) to stay type-safe -- unannotated it renders a raw pointer. Relevant
  to Chapter 12.
- **Two confirmed concurrency-primitive hangs** (LEDGER item 46, found and
  logged 2026-08-28, not yet fixed): a `std::chan::ChanWaitGroup` with two
  callers blocked on `wg_wait` only releases the first one, and a shared
  mutating closure's per-closure lock held across a coop-yield point
  deadlocks the whole process. Relevant to Chapters 13 and 14.
- ~~AOT-only proportional leak in the enum-array-push pattern~~ (LEDGER
  item 45): FIXED 2026-08-27 on both backends (was never actually
  AOT-only -- a measurement artifact). No longer a caveat.

## Contributing a chapter

1. Read `STYLE.md`.
2. Pick the next `planned` chapter above.
3. Write it at `docs/learn/book/NN-name.md`.
4. Run `python3 tools/docs-examples/check.py` and confirm every block in your
   file passes.
5. Update this table's status to `written` and link the chapter title.
