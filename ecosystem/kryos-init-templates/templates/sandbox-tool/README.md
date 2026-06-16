# sandbox-tool

A pure-compute Kryos project template. No IO, no net, no process.

## Declared capability surface

```toml
[capabilities]
allowed = ["compute"]
```

This is the tightest allowlist: the tool may only perform in-memory
computation. The compiler will reject any code that touches files, the
network, environment variables, or spawns processes without you first
updating the allowlist.

## What this template ships

| File | Purpose |
|------|---------|
| `src/main.kry` | Pure-compute entry point (prime sieve demo) |
| `tests/test_main.kry` | Unit tests for the computation logic |
| `ci.yml` | GitHub Actions snippet running `kryos-policy` on every push |
| `kryos.toml` | Package manifest with `allowed = ["compute"]` |

## Running

```bash
kryos run src/main.kry           # output primes up to 50
kryos test                       # run @test functions
kryos check src/main.kry         # type-check only
kryos manifest --caps --format pretty src -o caps.manifest
```

## Replacing the demo logic

1. Delete or rewrite the prime-sieve functions in `src/main.kry`.
2. Annotate every function with `@capabilities()` (empty parens = no caps needed).
3. Run `kryos manifest --caps` and verify the output surface is `[]` or `["compute"]`.
4. The `ci.yml` CI step will enforce this on every PR.

If your tool needs to read files or call an API, switch to the
`mcp-server` or `governed-agent` template instead.
