# kryos-doctor-caps

Project-wide capability audit report for Kryos projects.

## What it does

Consumes `kryos manifest --caps --strict --format json` output and renders a structured audit report:

- **Summary** - total functions, annotated vs unannotated, source files
- **By Capability** - which functions use each cap; dangerous caps (`ffi`, `process`, `net`) flagged
- **By Function** - declared caps per function; unannotated and dangerous callers highlighted
- **Could Narrow** - functions where declared caps are a strict superset of actual direct usage
- **Strict-Mode Would Fail** - unannotated functions that call gated builtins

## Usage

```bash
# Generate the manifest (--strict includes unannotated functions)
kryos manifest --caps --strict --format json <project-dir> -o caps.json

# Run the audit
kryos run src/main.kry caps.json
```

## Could Narrow detection

Because `kryos manifest` reports declared caps (from `@capabilities(...)` annotations), the tool
scans source file bodies directly for gated builtin calls to compute actual direct usage.
Transitive caps through callees are not tracked - the report focuses on direct builtin calls only.

## Capability taxonomy

10 concrete caps: `compute`, `crypto`, `db`, `env`, `ffi`, `io`, `net`, `process`, `term`, `time`.
Dangerous caps (`ffi`, `process`, `net`) are highlighted in the report.

## Tests

```bash
cd ecosystem/kryos-doctor-caps
kryos test
```

Runs 7 file tests + 20 `@test` functions including a full end-to-end pipeline test that verifies
the fixture's over-declared function (`save_report`) appears in "Could Narrow" and the unannotated
gated caller (`fetch_data`) appears in "Strict-Mode Would Fail".
