# mcp-server

A minimal MCP (Model Context Protocol) server template for Kryos.
Speaks JSON-RPC 2.0 over stdio; the host process sends newline-delimited
requests on stdin and reads responses from stdout.

## Declared capability surface

```toml
[capabilities]
allowed = ["compute", "io"]
```

The server reads stdin and writes stdout (`io`) and parses JSON (`compute`).
It does NOT declare `net` -- all communication goes through the host's
stdio bridge, not direct socket calls. If your tools need to make outbound
HTTP requests, add `net` to the allowlist and update the CI gate.

## What this template ships

| File | Purpose |
|------|---------|
| `src/main.kry` | JSON-RPC 2.0 dispatcher: initialize, ping, tools/list |
| `tests/test_main.kry` | Unit tests for JSON parsing and dispatch (no stdin) |
| `ci.yml` | GitHub Actions snippet running `kryos-policy` on every push |
| `kryos.toml` | Package manifest with `allowed = ["compute", "io"]` |

## Protocol support (MVP)

| Method | Response |
|--------|---------|
| `initialize` | `InitializeResult` with `serverInfo` |
| `ping` | Empty result `{}` |
| `tools/list` | Empty `{"tools":[]}` (add your tools here) |
| _unknown_ | JSON-RPC error `-32601` (MethodNotFound) |

## Running

```bash
kryos run src/main.kry           # starts server, reads from stdin
kryos test                       # run @test functions (no stdin needed)
kryos check src/main.kry         # type-check only
kryos manifest --caps --format pretty src -o caps.manifest
```

## Adding tools

1. Add a handler function annotated `@capabilities()` (or `@capabilities(io)` if it reads files).
2. Register it in `dispatch()` with the MCP method name.
3. Update `tools/list` to include the tool descriptor.
4. Run `kryos manifest --caps` and verify the surface stays within `["compute", "io"]`.
5. The CI step enforces this on every PR.
