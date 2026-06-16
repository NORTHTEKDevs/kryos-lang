# kryos-xcompile-caps

Capability-annotated cross-compilation matrix for Kryos projects.

Builds your project for each requested target triple, runs `kryos manifest --caps` on
the source, and asserts the capability surface is identical across all targets.
The output is `release-matrix.json` -- a per-target build record that includes the
binary path and the function-level capability set.

## Why

Zig and Go win cross-compilation on ergonomics -- `kryos-xcompile-caps` competes on a
different axis: a cross-compiled artifact should carry, per target, a statement of what
authority the produced binary holds. The capability set is a property of the source,
so it must be identical across targets. When platform-conditional `@capabilities`
annotations cause it to drift, this tool flags the discrepancy before release.

## Usage

```sh
# Single target (host)
kryos run src/main.kry -- myproject

# Two targets (host + explicit Windows triple)
kryos run src/main.kry -- --targets host,x86_64-pc-windows-msvc myproject

# Custom output directory
kryos run src/main.kry -- --targets host,x86_64-pc-windows-msvc myproject --out dist
```

Or after installing the binary:

```sh
kryos-xcompile-caps --targets host,x86_64-pc-windows-msvc myproject
```

Exit codes:
- `0` -- cap surface is stable (identical across all targets)
- `1` -- drift detected (caps differ across targets)
- `2` -- usage error

## release-matrix.json

```json
{
  "schema": "kryos-xcompile-caps-v1",
  "project": "myproject",
  "cap_stable": true,
  "cap_union": ["io"],
  "targets": {
    "host": {
      "build_success": true,
      "binary": "myproject/binary-host",
      "caps": ["io"]
    },
    "x86_64-pc-windows-msvc": {
      "build_success": true,
      "binary": "myproject/binary-x86_64-pc-windows-msvc",
      "caps": ["io"]
    }
  }
}
```

When a target fails to link (missing cross-toolchain), `build_success` is `false` and
the entry has `build_error` instead of `binary`. Caps are still captured from the source
manifest and included in the stability check.

## GitHub Actions release matrix

```yaml
jobs:
  release-matrix:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Kryos
        run: cargo install kryos-lang

      - name: Build cross-compile capability matrix
        run: |
          kryos run ecosystem/kryos-xcompile-caps/src/main.kry -- \
            --targets host,x86_64-pc-windows-msvc \
            --out dist \
            myproject
        env:
          KRYOS: kryos

      - name: Upload release matrix
        uses: actions/upload-artifact@v4
        with:
          name: release-matrix
          path: dist/release-matrix.json

      - name: Gate on cap stability
        run: |
          $stable = (Get-Content dist/release-matrix.json | ConvertFrom-Json).cap_stable
          if (-not $stable) {
            Write-Error "Capability surface drifted across targets -- check release-matrix.json"
            exit 1
          }
```

## Cross-toolchain note

Cross-compilation requires the target `cc`/linker and sysroot to be installed.
On a stock Windows host, `host` and `x86_64-pc-windows-msvc` link successfully;
Linux and macOS triples show `build_success: false` in the matrix (no linker found).
Caps are always captured from the source manifest regardless of link outcome.

For a real multi-platform matrix, run each target on its native runner (Linux, macOS,
Windows) and aggregate the `release-matrix.json` artifacts -- the `cap_union` and
`cap_stable` fields are deterministic across hosts for the same source.

## Tests

```sh
# Unit tests (caps logic, render, drift detection)
kryos run tests/test_caps.kry
kryos run tests/test_matrix.kry

# End-to-end on stable fixture
kryos run src/main.kry -- --targets host,x86_64-pc-windows-msvc tests/fixtures/stable
```

The drift fixture (`tests/fixtures/drift/`) contains both `@capabilities(io)` and
`@capabilities(net)` functions. The `test_drift_detection_on_platform_conditional_fixture`
test in `test_matrix.kry` demonstrates the tool's drift-detection logic with synthetic
per-target cap sets (simulating a hypothetical platform-conditional build where one
target strips `net`).

## Capabilities

This tool needs `io` (to write `release-matrix.json`) and `process` (to invoke
`kryos build` and `kryos manifest` as subprocesses).
