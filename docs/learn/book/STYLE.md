# Book style guide

This is a working document for whoever writes a chapter of `docs/learn/book/`.
It is not a chapter itself and is not part of the reading path in `README.md`.
Read this before writing or editing a chapter.

## Why this book exists

`docs/learn/tour.md` is a 30-minute tour. `docs/learn/cookbook/` is 27 recipes.
`docs/0*.md`/`docs/1*.md` are 19 dense reference chapters. None of these is a
book: a tour skips depth on purpose, a cookbook assumes you already know the
language, and the numbered chapters were written as standalone references, not
as a single throughline with a deliberate teaching order. The book at
`docs/learn/book/` is that throughline -- it takes a working programmer who
knows some other language from zero to fluent, in a fixed order, each chapter
building on exactly the concepts the previous ones introduced. Where a chapter
here would just re-explain what an existing `docs/NN-*.md` file already
covers well, the book chapter SUMMARIZES the mental model and links out to
the reference rather than duplicating it -- see `README.md` for which
chapters do this.

## The chapter template

Every chapter follows this shape, in this order:

1. **One-paragraph intro.** States what the reader will be able to DO after
   reading the chapter -- not a list of topics, an outcome. ("By the end of
   this chapter you will be able to write a function that reads a file and
   have the compiler catch it if you forget to declare that.")
2. **Concepts in dependency order.** Introduce nothing that depends on a
   concept from a later chapter. If chapter 4 needs closures, either closures
   moved earlier or the chapter orders itself to avoid the dependency.
3. **At least one complete, runnable worked example, with its actual output
   shown.** Not a fragment -- a full `fn main()` program you could paste into
   `hello.kry` and run. The output block underneath it is the REAL output of
   running that exact program through the reference compiler, pasted, not
   invented (see "Example conventions" below).
4. **Common mistakes**, tied to real compiler diagnostics. Show the broken
   code, the actual error the compiler prints (error code and message,
   copy-pasted from a real `kryos check` run), and the fix. Pull from
   `docs/claude/FULL-REFERENCE.md` and this repo's `CLAUDE.md` gotcha list --
   if a mistake is common enough that Claude needed to be warned about it, a
   human reader will hit it too.
5. **Exercises.** 2-5 short prompts the reader can attempt with what the
   chapter just taught. No solutions needed; the point is retrieval, not a
   graded assignment.
6. **Summary.** 3-6 bullets, the load-bearing facts from the chapter, nothing
   new introduced here.
7. **Next: [chapter title](NN-file.md).** One line, points to the next
   chapter in reading order. The first chapter also has no "previous" link;
   the last chapter of a Part links to the first chapter of the next Part.

## Terminology (one canonical term each, defined once)

Define each of these in the chapter where it is FIRST used, then use the same
word every time after -- do not vary the term for style:

- **ARC** -- automatic reference counting. The mechanism behind `str`,
  `[T]`, `map<K, V>`, and struct/enum value sharing (Chapter 10). Never call
  it "garbage collection" (it isn't -- no cycle collector, no GC pause) or
  "ownership" alone (Rust's move-based ownership is a different model; Kryos
  values are shared by refcount, not moved -- see gotcha table in Chapter 10).
- **Capability** -- a declared, compile-time-checked grant of authority to
  use a class of system resource (`fs:read`, `net:http`, `process`, ...).
  Never "permission" (implies a runtime/OS concept; capabilities in Kryos are
  static) or "scope" (a different, unrelated term for variable visibility).
- **Actor** -- a `Name()`-constructed concurrent unit with private,
  zero-initialized state and message-handler methods, one of the two
  concurrency primitives alongside `spawn`+channels (Chapter 13). Never
  "goroutine" or "thread" (an actor is a higher-level construct; it may or
  may not map 1:1 to an OS thread depending on backend).
- **Backend** -- one of the three code generators: Cranelift (`kryos run`,
  debug JIT-via-subprocess), LLVM (`kryos build --release`, optimizing AOT),
  or wasm (`--backend wasm`, experimental). Never "target" alone for this --
  reserve "target" for the OS/arch triple (`--target x86_64-unknown-linux-gnu`),
  which is a separate axis from backend choice.
- **Box** -- an ARC-managed heap allocation (what a `str`/`[T]`/`map`/struct
  handle actually points at). Only use this word for the runtime allocation;
  do not use it as a verb for "wrap in Option" (that's "wrap" or "construct
  `Some(..)`").
- **Row** -- internal capability-checker terminology (`kryos-types`'s row
  system for `deny!` block enforcement). This is compiler-implementation
  vocabulary, not reader-facing -- do not introduce it in the book. If you
  need to describe the mechanism, say "the capability checker" and describe
  the observable behavior, not the internal data structure.

If you need a term not on this list, add it here with its one-sentence
definition before using it in a chapter.

## Example conventions

- **Complete programs get a real `fn main()`.** If the reader could plausibly
  paste the block into a file and run it, it must be exactly that: a full
  program, not a fragment with implied context.
- **Fragments (illustrating one expression, one signature, a diff-style
  before/after) use `<!-- docs-example: skip -->` on the line directly above
  the fence.** This opts the block out of the CI type-checker
  (`tools/docs-examples/check.py`). Use this freely for pseudo-code and
  partial snippets -- never to avoid fixing a block that should compile but
  doesn't.
- **Every runnable example must have actually been run against the reference
  compiler** (`compiler/target/release/kryos.exe`, with
  `KRYOS_STDLIB_DIR=$PWD/compiler/stdlib` set) and its real stdout pasted
  into the "Output" block underneath. Never invent output. If you cannot run
  the compiler in your current environment, mark the block
  `<!-- docs-example: skip -->` and say so in the surrounding prose rather
  than guessing at output -- a wrong "Output" block is worse than an honest
  gap.
- **A deliberate compiler-error demo** (showing what a mistake looks like)
  uses a `// ERROR` comment on the offending line -- the checker auto-skips
  any block containing `// ERROR` or `error[`. Pair it with the actual error
  text in prose or a following non-`kryos`-fenced block, copy-pasted from a
  real `kryos check` run, not paraphrased.
- **A placeholder/`...`-elided block is auto-skipped** by the checker (any
  block containing `...` or `…`) -- use this for "the rest of the function is
  unchanged" style edits, not as a way to dodge writing a real example.
- Before committing a chapter, run
  `python3 tools/docs-examples/check.py` and confirm every block from your
  file passes (it scans the whole repo, so check that your file's blocks show
  0 failures in the output, not just that the run overall exits 0 if other
  files were already broken independently).

## The honesty rule

Kryos ships with known, documented limitations: the LEDGER items in
`tools/loop/LEDGER.md`, and the compressed gotcha list in this repo's
`CLAUDE.md`. Where a chapter's topic touches one of these, state the
limitation plainly, in the same voice as the rest of the chapter -- not
hedged, not buried in a footnote, not omitted because it's embarrassing.
Concretely:

- If teaching closures, the `||`-continuation parse trap (CLAUDE.md hard
  rule 1) gets its own "Common mistakes" entry, with the actual silent
  behavior shown (it compiles, it just does the wrong thing).
- If teaching capabilities, the precision cost is real: LEDGER item 41 shows
  41 of 75 enumerated legitimate pure-closure shapes require
  `@capabilities(all)` under the fail-closed design, and item 41's own text
  is explicit that this is deliberate, not a bug to be fixed quietly. Chapter
  11 states this as the honest cost of the security property, with a
  pointer to `docs/10-capabilities.md` and `docs/capability-roadmap.md` for
  the full soundness history.
- If teaching structs passed across function calls, the struct-argument
  leak (LEDGER item 3, ~86MB per 1M calls, open design note) is a real,
  measured cost for a specific pattern -- mention it where relevant with the
  workaround (read fields directly in a hot loop, keep heap data out of
  structs crossing calls) rather than presenting struct-passing as free.
- A book that hides a sharp edge produces a reader who finds it in
  production instead of on the page. State the limitation, state the
  workaround, move on -- do not editorialize about whether the limitation
  should exist.

## Cross-reference style

- Link forward and back using relative paths within `docs/learn/book/`
  (`01-hello.md`, not an absolute repo path).
- When summarizing and linking to an existing `docs/NN-*.md` chapter (per
  the redundancy rule in `README.md`), link with the relative path from
  `docs/learn/book/` (`../../06-ownership.md`) and say in one sentence why
  the reader might still want the deep-dive version (more exhaustive
  grammar coverage, more edge cases, etc.).
- Every chapter's "Next:" line is a real relative link, not just chapter
  prose naming the next topic.
- Do not forward-reference a concept by name without a link if it has its
  own later chapter (e.g. don't say "capabilities will stop this" in
  Chapter 2 without linking `11-capabilities.md`).

## Voice

Written for a working developer who already knows at least one other
language. Respect their time:

- Teach the mental model, not just the syntax -- explain WHY Kryos made a
  choice when it differs from what the reader likely expects (e.g. why
  passing a `str` to a function doesn't consume it, unlike Rust's move
  semantics).
- No marketing language. No "powerful", "elegant", "simply", "just",
  "obviously" -- if something is not obvious, that's exactly what the chapter
  should explain instead of asserting it away.
- No hedging filler ("you might want to consider maybe using..."). State the
  rule, then the exception if one exists.
- Comments in code examples are sparing and explain the ONE non-obvious
  thing, not every line.
