# kryos-policy

A manifest-level capability-contract linter for `kryos.toml`.

`kryos.toml` already carries a declared policy:

```toml
[capabilities]
allowed = ["compute", "io"]
```

Nothing in the toolchain checks that the *actual* compiler-computed capability
surface of a package is a subset of what that manifest declares. `kryos-policy`
closes that gap. It is `CapabilitySet::is_subset_of` applied at the package
boundary: **declared-allowed** vs **computed-actual**. A package that declares
`["compute"]` but whose code computes `net` is a contract violation, and this
linter is what catches it.

This is a safety contract no other package manager can verify, because no other
package manager computes a real capability set from the source.

## How it works

The linter does not re-implement capability inference. It consumes the
authoritative output of the compiler's own `kryos manifest --caps`:

1. Run `kryos manifest --caps --format pretty <pkg>/src -o caps.manifest` to get
   the computed per-function capability surface.
2. Read `[capabilities] allowed` from `kryos.toml`.
3. Compute the union of all computed capabilities (the **actual** surface).
4. Compare: if `actual ⊄ declared`, fail and print the excess capabilities.

The verdict is one of three:

- **Pass** — the declared allowlist exactly matches the computed surface.
- **Over-declared** — the subset contract holds, but the allowlist grants
  capabilities the code never uses. Safe, but not minimal. `--fix` tightens it.
- **Under-declared** — the code computes capabilities the allowlist does not
  grant. This is the failure the linter exists to catch; it lists the excess.

A literal `"all"` entry in the allowlist is a wildcard that grants every
capability (mirroring `Capability::All` in the compiler).

## --fix

`--fix` rewrites the `allowed = [...]` array to the computed minimum: the exact
set of capabilities the code uses, sorted and de-duplicated. It replaces only
that array and preserves every other byte of the `kryos.toml` (the `[package]`
block, comments, and the `[capabilities]` header). `demo_fix.kry` shows the full
before/after on disk.

## MVP scope

- Read `[capabilities] allowed` from `kryos.toml`.
- Consume `kryos manifest --caps` for the package's actual union surface.
- Fail if `actual ⊄ declared`; print the excess capabilities.
- `--fix` rewrites `allowed` to the computed minimum.
- Tests over over-declared, under-declared, exact-match, and wildcard fixtures.

### Out of scope (deferred)

- Per-dependency transitive policy.
- Package signing.
- Registry upload.

## Layout

```
kryos-policy/
  kryos.toml              package manifest (declares allowed = ["io"])
  src/
    caps.kry              capability-set primitives (subset, excess, normalize)
    manifest.kry          parse `kryos manifest --caps --format pretty` output
    toml.kry              read + rewrite the [capabilities] allowed list
    policy.kry            the verdict: check_policy / minimal_allowed
  demo_policy.kry         run the linter over all three fixtures
  demo_fix.kry            demonstrate --fix on disk
  tests/
    test_policy.kry       6 @test functions
    fixtures/
      exact/              declared == actual
      over_declared/      declares more than the code uses
      under_declared/     uses more than declared (the failure case)
```

Each fixture has a `kryos.toml` and a `caps.manifest` that is the **real**
compiler output for that fixture's `src/`, generated with:

```bash
kryos manifest --caps --format pretty tests/fixtures/<name>/src -o tests/fixtures/<name>/caps.manifest
```

## Run it

```bash
# From the repository root.
kryos test --path ecosystem/kryos-policy     # unit + file tests
kryos run  ecosystem/kryos-policy/demo_policy.kry   # verdicts over all fixtures
kryos run  ecosystem/kryos-policy/demo_fix.kry      # --fix before/after on disk
```

## Notes and limitations

- The linter consumes a manifest file produced by `kryos manifest --caps`; it
  does not shell out to the compiler itself. Produce the manifest as a build
  step (the pretty format is what the parser reads).
- Capability inference is only as complete as `kryos manifest`. The compiler
  walks top-level functions plus impl/trait methods; capabilities reached only
  through a closure passed as a value may be under-reported. This is a property
  of the manifest, not of the subset check, which is exact for whatever surface
  the manifest reports.
- The `--fix` rewrite assumes the `allowed = [...]` array is on a single line
  (the shape `kryos new` and every shipped ecosystem manifest use). A multi-line
  array is left unchanged.

## License

Apache-2.0. See `LICENSE`.
