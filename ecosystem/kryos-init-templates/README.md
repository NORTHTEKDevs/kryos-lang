# kryos-init-templates

Capability-scoped project scaffolds for Kryos. Every template ships a
`kryos.toml` with a correct, minimal `[capabilities]` allowlist and a CI
gate (using `kryos-policy`) so a new project is least-privilege from commit
zero.

## Templates

| Template | Declared caps | Use when |
|----------|--------------|---------|
| `sandbox-tool` | `["compute"]` | Pure in-memory computation, no external access |
| `governed-agent` | `["compute", "net"]` | HTTP-fetching agent with `@budget` call ceiling |
| `mcp-server` | `["compute", "io"]` | MCP JSON-RPC 2.0 server over stdio |

## Scaffolding a new project

```bash
# List available templates
kryos run ecosystem/kryos-init-templates/src/scaffolder.kry

# Scaffold into a new directory
kryos run ecosystem/kryos-init-templates/src/scaffolder.kry sandbox-tool my-tool
kryos run ecosystem/kryos-init-templates/src/scaffolder.kry governed-agent my-agent
kryos run ecosystem/kryos-init-templates/src/scaffolder.kry mcp-server my-server
```

## What each template includes

- `kryos.toml` -- `[capabilities]` allowlist set to the correct minimal surface
- `src/main.kry` -- working entry point demonstrating the declared pattern
- `tests/test_main.kry` -- `@test` functions with a standalone `kryos run` runner
- `ci.yml` -- GitHub Actions snippet: `kryos check` + `kryos-policy` gate
- `README.md` -- explains the declared surface and how to extend it

## Testing

```bash
# Test this package's metadata and policy correctness
kryos test --path ecosystem/kryos-init-templates

# Test each template independently
kryos test --path ecosystem/kryos-init-templates/templates/sandbox-tool
kryos test --path ecosystem/kryos-init-templates/templates/governed-agent
kryos test --path ecosystem/kryos-init-templates/templates/mcp-server
```

## Policy gate

Each template's CI snippet runs:

```bash
kryos manifest --caps --format pretty src -o caps.manifest
kryos run ../kryos-policy/src/policy_check.kry caps.manifest kryos.toml
```

This compares the compiler-computed capability surface against the declared
allowlist. A surface that exceeds the allowlist exits non-zero, blocking the
merge. The guard catches the only real trap in file templating: a code edit
that silently escalates capability usage without updating the allowlist.

## License

Apache-2.0. See [LICENSE](LICENSE).
