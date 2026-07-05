# How Kryos was built

Kryos was designed and implemented by one person, Kristian Baer, working with
AI coding assistants as the primary implementation tool. This page is here
because that fact is easy to discover (the commit history and development notes
make it obvious) and worth being direct about rather than coy.

## The honest version

AI wrote a large share of the code. A human made every design decision — the
capability model, the ownership/ARC memory strategy, the dual-backend
architecture, the wedge (compile-time governance for AI agents) — and, more
importantly, refused to accept "it works" without proof. That second part is
what makes the difference between an impressive-looking prototype and a
compiler you can actually build software with.

## Why "AI-built" is not the same as "unverified"

The failure mode people rightly worry about with AI-generated systems code is
plausible-but-wrong: it compiles, the demo runs, and it falls apart on the
first input the author did not think of. Kryos is built against that failure
mode on purpose. Concrete examples from the actual development record:

- **Cross-backend differential testing.** Every language feature is compiled by
  *both* backends (the Cranelift JIT and the LLVM AOT path) and their output is
  compared byte-for-byte. A 48-probe adversarial corpus plus the full smoke
  suite must agree on both. This has caught real miscompiles that a
  single-backend test would have shipped — aggregates through collections,
  closure ABI mismatches, a latent use-after-free in every `try`/`catch`.

- **Memory measured, not asserted.** The memory model was not called "fixed"
  because the code looked right. It was profiled: a churn workload that grew to
  ~10.7 GB of resident memory was driven down to a flat ~4 MB, verified on two
  different machines and two operating systems, and is now enforced by a
  standing CI gate that fails the build if a workload exceeds a hard ceiling.
  A double-free that only manifested under Linux glibc was found with valgrind,
  root-caused to a specific compiler pass, and fixed — not papered over.

- **Claims audited against reality.** The README says "one primary author, not
  yet externally stress-tested" because that is true. Where documentation drifted
  ahead of the implementation (benchmarks measured at a process-launch floor;
  async described as more finished than it was), the claims were corrected to
  match measured behavior, not the other way around.

- **CI is the arbiter, not self-report.** Nothing is considered done on the
  strength of "the model said it passed." A change is green only when the build,
  the full test suite, cross-backend parity across Linux/macOS/Windows, the
  ecosystem typecheck sweep, and the memory gate all pass on GitHub's runners.

## What this means if you are evaluating Kryos

Judge it on the executed evidence, which is public: the CI runs, the parity
reports, the benchmark methodology, [BENCHMARKS.md](../BENCHMARKS.md), and
[SHOWCASE.md](../SHOWCASE.md). Kryos is a feature-complete **beta** with one
author and no external stress-testing yet — that is the real limitation, and no
amount of verification discipline substitutes for real users finding the bugs a
test corpus cannot anticipate. Bug reports are genuinely welcome.

The pitch is not "an AI wrote a language." The pitch is: a capability-safe
systems language with an original wedge, built to a verification standard most
solo projects — AI-assisted or not — do not hold themselves to.
