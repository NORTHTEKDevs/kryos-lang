# kryos-mcp-governed

A capability-verified [Model Context Protocol](https://modelcontextprotocol.io) server written in [Kryos](https://github.com/NORTHTEKDevs).

**The thesis in one line:** an MCP tool is a Kryos function whose `@capabilities(...)` annotation is enforced by the compiler, mirrored into the `tools/list` response the host sees, and checked against a server allowlist at startup. The capability claim lives in the source, not in a side manifest you have to trust.

## Why

MCP lets an LLM host (Claude Desktop, Cursor, ...) discover and call tools. What MCP does **not** do is tell the host what system resources a tool touches: a `read_file` tool and an `exec_shell` tool look identical in `tools/list` (name, description, schema). Hosts and users accept servers on trust.

This server makes the capability surface structural:

1. **Declared in source** -- each tool function carries `@capabilities(net)` / `@capabilities(compute)` / etc. The Kryos compiler enforces it: a tool annotated `@capabilities(net)` that calls a file-write builtin **fails to compile**.
2. **Advertised to the host** -- the same capability set is emitted into the tool's `tools/list` entry as a machine-readable `kryos_capabilities` array plus a human-readable `[caps: ...]` suffix on the description.
3. **Checked at startup** -- every tool's declared caps are compared against a server allowlist (`KRYOS_MCP_CAPS`); excess is WARNed (soft) or fatal (`KRYOS_MCP_STRICT=1`).

## Quickstart

```bash
# Build the release binary (LLVM AOT)
kryos build --release src/main.kry -o kryos-mcp-governed.exe   # drop the .exe off Windows

# Ask it for its tools
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | ./kryos-mcp-governed.exe
```

`tools/list` returns (formatted here for readability):

```json
{
  "name": "add",
  "description": "Add two numbers [caps: compute]",
  "inputSchema": { "type": "object", "properties": { "a": {"type":"number"}, "b": {"type":"number"} }, "required": ["a","b"] },
  "kryos_capabilities": ["compute"]
}
```

`fetch` and `summarize` carry `"kryos_capabilities": ["net"]`. A host that understands the `kryos_capabilities` vendor field can show the user "this tool wants: net" before they call it; a host that ignores it still sees `[caps: net]` in the description text. (Vendor extension fields in a tool definition are allowed by the MCP spec, so this never breaks an unaware host.)

## Configuring the allowlist

`KRYOS_MCP_CAPS` (comma-separated) declares the capability surface the **server** is allowed to expose. Default: `net,compute`.

```bash
# A stricter operator only allows compute-class tools:
KRYOS_MCP_CAPS=compute ./kryos-mcp-governed.exe
# stderr:
#   WARN: tool 'fetch' declares cap 'net' not in server allowlist
#   WARN: tool 'summarize' declares cap 'net' not in server allowlist
#   WARN: 2 tool capability violation(s); continuing in soft mode
```

`KRYOS_MCP_STRICT=1` turns those WARNs into a hard, non-zero exit at startup:

```bash
KRYOS_MCP_STRICT=1 KRYOS_MCP_CAPS=compute ./kryos-mcp-governed.exe ; echo "exit=$?"
# stderr: FATAL: 2 tool capability violation(s) exceed the server allowlist (strict mode)
# exit=1
```

All diagnostics go to **stderr**; stdout carries only JSON-RPC, so the protocol stream is never corrupted.

## Claude Desktop

See `examples/claude_desktop_config.example.json`. Point `command` at the built binary and set the env:

```json
{
  "mcpServers": {
    "kryos-governed": {
      "command": "C:\\path\\to\\kryos-mcp-governed.exe",
      "env": { "KRYOS_MCP_CAPS": "net,compute" }
    }
  }
}
```

## Proof the enforcement is real

The `kryos_capabilities` field is only meaningful if the compiler actually enforces the annotation. It does. Add a file-write to the net-only `fetch` tool:

```kryos
@capabilities(net)
fn tool_fetch(args_node: i64) -> i64 {
  ...
  file_write("/tmp/x", "y")   // <-- file_write needs `io`, not declared
  ...
}
```

```bash
kryos build --release src/main.kry
# error[E-CAP-BUILTIN]: builtin `file_write` requires `io` capability
#   note: add `@capabilities(io)` to the enclosing function or actor
```

The tool **cannot** silently acquire file-write access. Symmetrically, the `net` declaration is not cosmetic: a function that calls the network builtin `http2_get` fails to compile under `@capabilities(compute)` with `builtin \`http2_get\` requires \`net\` capability`.

Run the whole demo:

```bash
./demo.sh
```

## Architecture

```
src/
  main.kry        server entry: use's the modules below, startup check + JSON-RPC loop
  tools.kry       EDIT HERE: tool impls, each @capabilities-annotated; call_tool dispatch
  cap_emit.kry    schema/prop helpers + tool_def_governed (emits kryos_capabilities)
  cap_check.kry   ServerPolicy / ToolCapRecord, check_tools, split_csv (startup allowlist)
kryos.toml
examples/claude_desktop_config.example.json
demo.sh
```

### Adding a tool

1. Write the function in `tools.kry` with the correct `@capabilities(...)`. The compiler tells you if you under-declare.
2. Add a `tool_def_governed("name", "desc", schema, ["cap", ...])` line to `list_tools_result()` in `main.kry`.
3. Add a `tool_cap("name", ["cap", ...])` line to `tool_registry()` in `main.kry`.
4. Add a `call_tool` dispatch arm in `tools.kry`.

The capability list appears in three places (the annotation, `tool_def_governed`, `tool_cap`) and you keep them in sync by hand -- see the honest gap below.

## Honest gaps (read this)

This MVP gives you **compile-time proof that an annotated tool function is consistent with its declared capabilities**, surfaced to the host and checked against a policy. It does not yet give full sandboxing. Specifically:

1. **Capabilities are opt-in.** A function with **no** `@capabilities` annotation is unconstrained -- the compiler only enforces the annotated surface. A tool you forget to annotate gets no protection. Mitigation: a CI lint that every tool function is annotated (not shipped here). The deny-by-default `--strict-capabilities` build flag is **planned, not implemented**.
2. **The capability strings are mirrored by hand.** The compiler enforces `@capabilities(net)` on the function, but it does not yet expose annotations as a runtime reflection value, so `tool_def_governed(... ["net"])` and `tool_cap(... ["net"])` are written by the developer to match. If they diverge, the host sees a wrong (but not under-enforced) claim. A future `kryos`-attribute-reflection step removes the duplication.
3. **Enforcement does not follow into unannotated helpers.** If an annotated tool calls an unannotated helper that calls `file_write`, the helper is checked only in its own (unannotated, therefore unenforced) scope. Closed by `--strict-capabilities`.
4. **Sub-capabilities are not enforced.** `net:http` vs `net:raw_socket` can be emitted as strings but the compiler enforces only the coarse `net`.
5. **`fetch` is real; `summarize` is still a stub.** `fetch` makes a genuine outbound HTTP/2 GET via the `http2_get` builtin, so its `net` capability is exercised, not just declared. `summarize` declares `net` + `@budget(tokens=2000, calls=1)` and returns a placeholder; the real implementation reads the API key via `env_get` (which requires the `process` capability) and calls `std::llm`, so its production annotation is `@capabilities(net, process)` -- a correction to the original design sketch, which declared only `net`.
6. **Resolved: `http2_get` now links on the LLVM release backend.** It previously failed AOT with `use of undefined value '@http2_get'` (missing LLVM call-site name mapping) while working under `kryos run` (Cranelift); fixed in the compiler (added `http2_get`/`http2_post`/`http2_request`/`https_get` to the LLVM name map). `fetch` was stubbed until this landed and is now a live call. Requires a toolchain that includes the fix.

## Capability vocabulary

Recognized names (case-insensitive): `net, io, ffi, compute, crypto, process, env, term, db, time, all`. Notable builtin -> capability mappings the compiler enforces: `file_write`/`read_file` -> `io`; `env_get`/`exit` -> `process`; `http2_get`/`tcp_connect`/`tls_*` -> `net`; `sha256` -> `crypto`. An unknown name in `@capabilities(...)` is a `W-CAP-UNKNOWN` warning, not an error.
