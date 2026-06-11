# Versioning

## The recalibration (June 2026)

This repository's early version numbers tracked **development sprints**, not
conventional semver maturity: during the AI-assisted bring-up, each working
session typically shipped a "minor" version, reaching an internal v4.46.0
within months of the first commit. Read conventionally, "v4" implies years
of production stability — an expectation this project could not honestly
meet with one user and zero external validation.

Before the first public release, the scheme was recalibrated:

| Old internal tag | Meaning today |
|---|---|
| v0.x – v2.x | Bring-up: parser → backends → stdlib → self-host bring-up |
| v3.x – v4.4x | Hardening sprints: bootstrap fixed point, dual-backend parity, exception semantics, conformance suite |
| **v1.0.0-beta.1** | First externally-facing release. Feature-complete; self-hosting; **one user**; not yet stress-tested by anyone else |

Historical tags are preserved in git for forensics; the CHANGELOG retains
every entry under its original number.

## The contract going forward

- **1.0.0-beta.N** — feature-complete betas. Breaking changes are possible
  and will be called out loudly in the CHANGELOG.
- **1.0.0** — cut only after external users have run real workloads against
  it. Not before.
- **Post-1.0** — conventional semver: patch = fixes, minor = additive,
  major = breaking. The stability surface (CLI, LSP methods, stdlib symbols,
  ABI) is specified in [STABILITY-v4.0.md](STABILITY-v4.0.md), which carries
  over as the contract document (its guarantees were written against the
  internal v4 line and apply to 1.x unchanged).

## What "feature-complete beta" means concretely

- The language, both backends, stdlib, and toolchain are done and gated:
  47-test dual-backend parity suite, 6-chapter spec conformance suite,
  byte-identical 3-stage bootstrap, 900+ workspace tests, docs whose every
  code block is CI-checked.
- "Beta" is about **exposure**, not completeness: one user's usage patterns,
  however thoroughly gated, are not a substitute for strangers' workloads.
