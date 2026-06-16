# kryos-registry-badging (ecosystem project 05)

Every package version carries a machine-readable **capability badge** derived
from its source at publish time. `kryos pkg show <name>` tells you whether a
package is compute-only, network-capable, or process/FFI-spawning **before you
install it**; `kryos pkg audit <name>` fails CI when a new version adds a
dangerous capability (`ffi`/`process`) that the previous version lacked.

No mainstream registry (npm, crates.io, PyPI) shows this. Deno is the closest
prior art, but Deno permissions are *requested at runtime*; a Kryos badge is
*derived from the compiler's capability analysis of the source*.

## Implemented (this PR)

Compiler-crate feature, not a standalone `.kry` program:

- **`kryos-package`** — `CapsBadge` data model (`caps.rs`): schema, capability
  union, dangerous subset, annotation-coverage %, audit-escalation logic.
  `RegistryEntry` gains an optional `capabilities` field; `generate_index_entry`
  embeds it, `parse_index_entry` reads it back (absent → `None`, fully
  backward-compatible with pre-badging entries). `pack()` reads
  `target/caps.json`.
- **`kryos-cli`** — new `caps_badge.rs`:
  - `kryos pkg badge [path]` — walk a project's `.kry` sources, union the
    capability sets of `@capabilities`-annotated functions, write
    `target/caps.json`.
  - `kryos pkg show <name>` — render the latest version's badge.
  - `kryos pkg audit <name> [--strict]` — compare the two latest versions; exit
    1 on a dangerous escalation (`ffi`/`process`), or on any new capability
    under `--strict`.

> Naming note: the badge generator is `kryos pkg badge` (not `kryos manifest
> --caps` as the original spec drafted) to avoid editing `manifest_cmd.rs`,
> which was under concurrent development. `kryos manifest` already emits the
> per-function capability manifest this builds on.

## Verified (executed)

```
$ kryos pkg badge ./fixture        # 4 fns: 2 annotated (net, ffi), 2 unannotated
wrote .../target/caps.json
capabilities : ffi, net
coverage     : 50% of functions annotated
dangerous    : ffi
# caps.json: {"schema":"kryos-caps/1","capabilities":["ffi","net"],
#             "dangerous":["ffi"],"annotation_coverage_pct":50,"inferred_uncovered":[]}

$ kryos pkg show http-router       # live registry
Package      : http-router v0.1.0
Capabilities : (no badge — package predates capability badging)

$ kryos pkg audit http-router
kryos pkg audit: http-router has fewer than 2 published versions — nothing to compare
```

- `cargo test -p kryos-package` — 37 unit + 19 integration tests pass (9 new for
  the badge: serialization round-trip, generate/parse with caps, parse without
  caps → `None`, dangerous-escalation detection, strict new-capability
  detection, coverage math).
- `cargo test -p kryos-capabilities` — 26 + 49 pass (unchanged, no regression).

## Honest limitations (stated in the badge JSON and `pkg show` output)

- **Annotated functions only.** Kryos capabilities are opt-in per function (no
  deny-by-default yet). `annotation_coverage_pct` reports how much of the
  package is proven; a zero-annotation package shows an empty badge — that is
  "unknown", not "no capabilities".
- **Coarse-grained.** Top-level capabilities only (no `fs:read` vs `fs:write`).
- **`inferred_uncovered` is empty in this MVP** (best-effort inference of
  unannotated-function capabilities from builtins is deferred).
- **Badge trust:** generated from source on the author's machine at publish
  time, not re-verified by a third party. Registry-side re-generation from the
  uploaded tarball is a v2 item.
- **Not done here:** writing a real badge into the live `kryos-registry`
  `http-router` entry (separate repo + push), and `pkg publish` auto-running
  `pkg badge`.
