# governed-agent

A network-enabled Kryos agent template with a `@budget` call ceiling.

## Declared capability surface

```toml
[capabilities]
allowed = ["compute", "net"]
```

The agent may make outbound HTTP requests (`net`) and perform
in-memory computation (`compute`). File system access, process
spawning, environment variables, and database access are all
outside the declared surface -- the compiler will reject any
code that touches them without you updating the allowlist first.

The `@budget(calls = 3)` annotation on `run_agent` limits the
number of HTTP calls that function may make per invocation,
enforced at runtime by the Kryos budget frame.

## What this template ships

| File | Purpose |
|------|---------|
| `src/main.kry` | Agent with `fetch_data` (net) and `summarize` (compute) |
| `tests/test_main.kry` | Unit tests for the pure-compute helpers (offline) |
| `ci.yml` | GitHub Actions snippet running `kryos-policy` on every push |
| `kryos.toml` | Package manifest with `allowed = ["compute", "net"]` |

## Running

```bash
kryos run src/main.kry           # offline demo (no network call from main)
kryos test                       # run @test functions (pure compute, no net)
kryos check src/main.kry         # type-check only
kryos manifest --caps --format pretty src -o caps.manifest
```

## Customizing

1. Replace the `fetch_data` URL with your endpoint.
2. Update the `summarize` function to parse the response.
3. Adjust `@budget(calls = N)` to your per-invocation budget.
4. Run `kryos manifest --caps` and verify the output surface contains only `net` (and optionally `compute`).
5. The CI step enforces this on every PR.

If your agent needs to write local state, add `io` to `allowed` and switch to the `mcp-server` template as a reference.
