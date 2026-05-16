<!--
Thanks for contributing to Kryos. A few quick notes:

* Keep PRs focused. One conceptual change per PR is best.
* For language-behavior changes, please include a `.kry` test program in
  `compiler/tests/native/` or wherever the most relevant suite lives.
* Run `cargo test --release -j 2 --manifest-path compiler/Cargo.toml`
  locally before pushing.
-->

## Summary

<!-- What does this PR do, in one or two sentences? -->

## Motivation

<!-- Why is this change worth making? Link to any related issue or
     discussion. -->

## Implementation notes

<!-- Anything reviewers should know before reading the diff:
     * non-obvious design choices,
     * tradeoffs you considered and rejected,
     * follow-up work intentionally left for a later PR. -->

## Tests

<!-- How did you verify this works? At minimum:
     * which test files cover the new behavior,
     * pass/fail counts before and after, especially the native
       `--release` sweep if you touched codegen/MIR/runtime. -->

## Checklist

- [ ] `cargo build --release` is clean (no warnings).
- [ ] Relevant unit tests pass.
- [ ] Native `--release` sweep still passes (run if you touched
      compiler, MIR, codegen, runtime, or stdlib).
- [ ] CHANGELOG entry added under the next unreleased section, if the
      change is user-visible.
