# kryos-mcp-governed: Capability-Verified MCP Server Template

**One-line pitch:** MCP tools are Kryos functions with `@capabilities` annotations -- the server refuses to register tools that exceed the declared capability surface, and every tool description advertises what system resources it may touch.

---

## Why This Exists

### The gap in the MCP ecosystem

The Model Context Protocol lets an LLM host (Claude Desktop, Cursor, etc.) discover and call tools on a local or remote server. What the protocol does NOT do:

- Tell the host what system resources a tool can access. A "read_file" tool and a "exec_shell" tool look identical in the tools/list response -- both return a name, description, and input schema.
- Enforce any correspondence between what a tool advertises and what it actually does. A server can lie or simply not know.
- Give the host a machine-readable capability surface it can render to the user before they consent to installing a server.

The result: users and LLM hosts accept MCP servers on trust. There is no structural difference between a safe, read-only tool and one that exfiltrates data.

`NORTHTEKDevs/kryos-mcp-template` already exists and implements the JSON-RPC 2.0 stdio wire protocol in ~180 lines of Kryos. This project extends it by making the Kryos `@capabilities` annotation the single source of truth for:

1. What a tool function is allowed to do (enforced by the Kryos compiler today for annotated functions).
2. What the MCP host is told the tool can do (emitted into the tool description JSON at registration time).
3. What the server verifies at startup (allowlist check against a declared server capability surface).

### Who else does this / novelty rating: PARTIAL

Kryos is not the first system to think about sandboxing tool servers. Three prior approaches exist:

- **Wasm-based tool sandboxing:** Extism and similar frameworks run plugins in a WASM sandbox where the host controls what imports the plugin can call. This is runtime enforcement, not a language-level annotation. You cannot look at the plugin source and read what it claims to need; you inspect the import section of the compiled binary.
- **OpenTelemetry / audit logging:** Any server can emit structured logs of what it touches. This is after-the-fact and voluntary.
- **MCP server capability field (current spec):** The MCP `initialize` response has a `capabilities` field, but it is about protocol features (tool listing, resource listing, prompts) -- NOT about what system resources the tools access. It says nothing about filesystem, network, or process spawning.

What Kryos adds that these do not:

1. The capability claim is written in the source file, not a config file or a separate manifest. The annotation `@capabilities(net)` on `fn fetch_url(...)` and the MCP tool description `"capabilities": ["net"]` come from the same attribute -- you cannot have one without the other.
2. The compiler already checks (for annotated functions) that the declared set covers the builtins used. A function annotated `@capabilities(compute)` that calls `http_get` will fail to compile today. This is compile-time, not runtime.
3. The server startup allowlist check gives the operator a defined policy surface: "this server is allowed to use net and io; refuse to start if any tool declares more."

Honest status of each piece:
- `@capabilities` annotation parsed and checked by `kryos-capabilities` crate: IMPLEMENTED (opt-in per annotated function).
- Attenuation (a called function cannot exceed its caller's declared set): IMPLEMENTED.
- Deny-by-default / unannotated functions restricted: NOT IMPLEMENTED (`--strict-capabilities` is planned, not shipped).
- Sub-capabilities (`net:http` vs `net:raw_socket`): DEFINED IN DOCS, NOT ENFORCED by the compiler today.
- Runtime `CapabilityEnforcer` / sandboxing: NOT IMPLEMENTED.

The honest claim: today you get compile-time proof that an annotated tool function is consistent with its declared capabilities. You do not get proof that an unannotated helper it calls is also constrained. The value is still real -- a server that ships with `@capabilities` on every tool function and runs `kryos build --release` is provably consistent at the annotated surface. The governance story tightens further when `--strict-capabilities` ships.

---

## Which Kryos Primitives This Uses

All of the following are implemented and working today:

| Primitive | Where it lives | How this project uses it |
|---|---|---|
| `@capabilities(net, io, compute, ...)` | `kryos-capabilities` crate; compiler attribute | Annotate each tool `fn`; emit the set into MCP tool description JSON |
| `json_object`, `json_array`, `json_string`, `json_get`, `json_to_str`, `json_parse`, `json_stringify` | Built-in JSON primitives (no import needed) | Build/parse all MCP JSON-RPC messages; emit capability arrays into tool definitions |
| `std::tracked::Tracked`, `tracked_source`, `transform`, `to_json` | `compiler/stdlib/tracked.kry` | Optional: wrap tool output in a `Tracked<str>` whose lineage the MCP host can inspect |
| `@budget(tokens=N, calls=M)` | `kryos-rt/src/budget.rs`; compiler attribute | Budget-bound any tool that calls `std::llm` internally |
| `std::llm::chat`, `complete`, `LlmConfig` | `compiler/stdlib/llm.kry` | LLM-powered tools inside the MCP server |
| `@capabilities(net)` on entry points | same crate | Server main loop declares `net` (for any tool needing outbound HTTP) |
| `read_line`, `println`, `json_parse`, `json_stringify` | built-in builtins | JSON-RPC stdio transport (already in the template) |

No new language features are required. This is purely additive on top of the existing template.

### Language work needed first

None for the MVP. The MVP (annotated tools + capability emission + startup allowlist) works with the current compiler. The following would improve the story but are not blockers:

- `--strict-capabilities`: would close the unannotated-helper gap. Ship MVP without it; document the limitation.
- Sub-capability enforcement (`net:http` vs `net:raw_socket`): currently in docs only. The spec can emit sub-capability strings in JSON without the compiler enforcing them; add a note that enforcement is planned.
- Runtime `CapabilityEnforcer`: would allow the server to sandbox a tool function at call time. Not needed for the declaration/emit/allowlist MVP.

---

## Architecture

### Components

```
kryos-mcp-governed/
  src/
    main.kry          -- server entry point: startup check + JSON-RPC loop
    tools.kry         -- tool implementations (EDIT HERE; each fn annotated)
    cap_check.kry     -- startup capability allowlist check
    cap_emit.kry      -- helpers to emit @capabilities into MCP tool JSON
    lineage.kry       -- optional Tracked<str> wrapper for tool responses
  kryos.toml
  README.md
  examples/
    claude_desktop_config.example.json
```

### Data model

The key insight: the existing `tool_def` helper in the template takes `(name, description, schema_node)`. We extend it to `tool_def_governed` which takes `(name, description, schema_node, caps)` where `caps` is a `[str]` -- the capability strings extracted from the function's `@capabilities` annotation.

In the current Kryos compiler, `@capabilities(...)` is an attribute checked at compile time but not available as a runtime value. Therefore the capability strings are passed explicitly as a parallel `[str]` argument by the developer -- they mirror the annotation. This is the honest, buildable-today approach. When the compiler gains reflection of attributes, this parameter can be auto-generated.

Key structs (in `cap_check.kry`):

```kryos
// The declared capability surface of the whole server.
// Read from an env var or a config field at startup.
struct ServerPolicy {
    allowed_caps: [str]
}

// Per-tool capability record assembled at startup.
struct ToolCapRecord {
    tool_name: str,
    declared_caps: [str],
    allowed: bool
}
```

### Kryos code sketches (real syntax)

#### cap_emit.kry -- build an MCP tool definition with a capability field

```kryos
fn prop_string() -> i64 { return json_object(["type"], [json_string("string")]) }
fn prop_number() -> i64 { return json_object(["type"], [json_string("number")]) }

// Build the "kryos_capabilities" JSON array node from a [str] of cap names.
fn caps_node(caps: [str]) -> i64 {
    let mut nodes: [i64] = []
    let mut i: i64 = 0
    while i < len(caps) {
        nodes = push(nodes, json_string(caps[i]))
        i = i + 1
    }
    return json_array(nodes)
}

// Extended tool_def that embeds capability claims in the description object.
// The MCP spec allows arbitrary fields in the tool description;
// we add "kryos_capabilities" as a vendor extension key.
fn tool_def_governed(name: str, description: str, schema_node: i64, caps: [str]) -> i64 {
    let cap_str: str = caps_to_str(caps)
    let full_desc: str = description + " [caps: " + cap_str + "]"
    return json_object(
        ["name", "description", "inputSchema", "kryos_capabilities"],
        [json_string(name), json_string(full_desc), schema_node, caps_node(caps)]
    )
}

fn caps_to_str(caps: [str]) -> str {
    if len(caps) == 0 { return "compute" }
    let mut s: str = caps[0]
    let mut i: i64 = 1
    while i < len(caps) {
        s = s + ", " + caps[i]
        i = i + 1
    }
    return s
}
```

#### cap_check.kry -- startup allowlist enforcement

```kryos
fn cap_allowed(cap: str, allowed: [str]) -> bool {
    let mut i: i64 = 0
    while i < len(allowed) {
        if allowed[i] == cap or allowed[i] == "all" { return true }
        i = i + 1
    }
    return false
}

// Returns true if ALL declared tool caps are within the server allowlist.
// Logs a warning (or throws) for any tool that exceeds the allowlist.
fn check_tools(records: [ToolCapRecord], policy: ServerPolicy) -> bool {
    let mut all_ok: bool = true
    let mut i: i64 = 0
    while i < len(records) {
        let rec = records[i]
        let mut j: i64 = 0
        while j < len(rec.declared_caps) {
            let cap = rec.declared_caps[j]
            if not cap_allowed(cap, policy.allowed_caps) {
                println("WARN: tool '" + rec.tool_name + "' declares cap '" + cap + "' not in server allowlist")
                all_ok = false
            }
            j = j + 1
        }
        i = i + 1
    }
    return all_ok
}

// Build a ToolCapRecord for a single tool.
fn tool_cap(name: str, caps: [str]) -> ToolCapRecord {
    return ToolCapRecord { tool_name: name, declared_caps: caps, allowed: true }
}
```

#### tools.kry -- annotated tool implementations (the EDIT HERE section)

```kryos
use std::tracked::{Tracked, tracked_source, transform, to_json}

// Pure computation tool -- no external resources.
@capabilities(compute)
fn tool_add(args_node: i64) -> i64 {
    let a_n: i64 = json_get(args_node, "a")
    let b_n: i64 = json_get(args_node, "b")
    if a_n <= 0 or b_n <= 0 { return err_content("missing 'a' or 'b'") }
    let a: f64 = json_to_float(a_n)
    let b: f64 = json_to_float(b_n)
    return text_content(to_string(a + b))
}

// Network-accessing tool -- declares net.
@capabilities(net)
fn tool_fetch(args_node: i64) -> i64 {
    let url_n: i64 = json_get(args_node, "url")
    if url_n <= 0 { return err_content("missing 'url'") }
    let url: str = json_to_str(url_n)
    let body: str = http_get(url)
    // Wrap in Tracked so the MCP host can inspect provenance.
    let tracked_body: Tracked<str> = tracked_source(body, url, "http GET")
    return text_content(to_json(tracked_body))
}

// LLM-powered tool -- declares net (outbound HTTP to API) + budget.
@capabilities(net)
@budget(tokens=2000, calls=1)
fn tool_summarize(args_node: i64) -> i64 {
    use std::llm::{anthropic_config, complete}
    let text_n: i64 = json_get(args_node, "text")
    if text_n <= 0 { return err_content("missing 'text'") }
    let text: str = json_to_str(text_n)
    let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-haiku-4-5")
    let summary: str = complete(cfg, "Summarize in one sentence: " + text)
    return text_content(summary)
}
```

#### main.kry -- startup check + dispatch

```kryos
// Server capability allowlist -- what this server as a whole is allowed to use.
// Override with KRYOS_MCP_CAPS env var (comma-separated).
fn server_policy() -> ServerPolicy {
    let env_caps: str = env_get("KRYOS_MCP_CAPS")
    if len(env_caps) > 0 {
        return ServerPolicy { allowed_caps: split_csv(env_caps) }
    }
    // Default: net and compute only.
    return ServerPolicy { allowed_caps: ["net", "compute"] }
}

fn split_csv(s: str) -> [str] {
    use std::string::{split}
    return split(s, ",")
}

// Build the tool registry at startup and run the allowlist check.
fn startup_check() -> bool {
    let policy: ServerPolicy = server_policy()
    let records: [ToolCapRecord] = [
        tool_cap("add",       ["compute"]),
        tool_cap("fetch",     ["net"]),
        tool_cap("summarize", ["net"])
    ]
    let ok: bool = check_tools(records, policy)
    if not ok {
        println("WARN: some tools exceed server capability surface")
        // Log and continue (soft mode). Change to `throw` for hard mode.
    }
    return ok
}

fn list_tools_result() -> i64 {
    let add_schema: i64 = schema(["a", "b"], [prop_number(), prop_number()], ["a", "b"])
    let fetch_schema: i64 = schema(["url"], [prop_string()], ["url"])
    let sum_schema: i64 = schema(["text"], [prop_string()], ["text"])

    let tools: [i64] = [
        tool_def_governed("add",       "Add two numbers",                   add_schema,   ["compute"]),
        tool_def_governed("fetch",     "Fetch a URL and return the body",   fetch_schema, ["net"]),
        tool_def_governed("summarize", "Summarize text using Claude Haiku", sum_schema,   ["net"])
    ]
    return json_object(["tools"], [json_array(tools)])
}

fn main() {
    startup_check()
    while SHUTDOWN == 0 {
        let line: str = read_line()
        if len(line) == 0 { break }
        let resp: str = handle_message(line)
        if len(resp) > 0 { println(resp) }
    }
}
```

### Lineage in tool responses (optional but recommended)

For any tool that produces output that flows into an LLM context, wrapping in `Tracked<str>` gives the MCP host structured provenance. The `to_json` output is a valid string the host can log or display:

```kryos
// In tool_fetch:
let tracked: Tracked<str> = tracked_source(body, url, "http GET from tool_fetch")
let traced: Tracked<str> = transform(tracked, body, "mcp_response", "returned to MCP host")
return text_content(to_json(traced))
```

The `to_json` output is a JSON object with `value`, `source`, and `lineage` keys. An MCP host that understands this format can display the full audit trail; one that does not simply receives the JSON string as text -- no breakage.

---

## MVP Scope

The smallest shippable slice that proves the thesis:

1. Fork `kryos-mcp-template/main.kry` into `src/main.kry`.
2. Extract three helper files: `cap_emit.kry`, `cap_check.kry`, `tools.kry`.
3. Replace `tool_def(...)` calls with `tool_def_governed(...)` -- the only user-visible API change.
4. Add `startup_check()` call at the top of `main()`.
5. Three demo tools: `add` (compute), `fetch` (net), `summarize` (net + budget).
6. `KRYOS_MCP_CAPS` env var to configure the server allowlist.
7. README showing Claude Desktop config and the capability field in the tool description.

Expected size: ~250 lines of Kryos across four files, plus the existing ~183-line template plumbing (kept unchanged).

### What MVP does NOT include (deferred to full vision)

- Auto-extraction of caps from `@capabilities` annotation at compile time (needs compiler reflection API -- not implemented). For now: developer mirrors the annotation as a `[str]` parameter to `tool_def_governed`. Checked by a build-time linter in full vision.
- Sub-capability enforcement (`net:http` vs `net:raw_socket`). The JSON can include sub-cap strings; enforcement is on the roadmap.
- `--strict-capabilities` build flag. When that lands, un-annotated helper functions called by tools also become constrained. MVP documents this gap.
- Runtime enforcement / sandboxing per tool call. Planned as `CapabilityEnforcer` in docs; not implemented.
- MCP resource listing with capability metadata (only tool listing in MVP).
- A linter / `kryos check --mcp-caps` command that verifies annotation-to-parameter parity. Full vision only.

### Full vision

- Compiler emits capability metadata into the binary at build time; `tool_def_governed` reads it via a `@reflect` attribute (or a codegen-time step) rather than a manual `[str]` parameter.
- `--strict-capabilities` closes the unannotated-helper gap: every helper called by an annotated tool is transitively constrained.
- MCP registry integration: capability surface is committed to `NORTHTEKDevs/kryos-registry` as part of the package manifest, visible before install.
- Hard-mode startup: server refuses to start (not just warns) if any tool exceeds the allowlist.
- `Tracked<str>` lineage in all tool responses becomes the default, not opt-in.

---

## Build Plan

A fresh session can follow these steps in order. Each step ends with a verification command.

### Step 1 -- scaffold the project

```bash
mkdir -p kryos-mcp-governed/src kryos-mcp-governed/examples
cd kryos-mcp-governed
```

Create `kryos.toml`:

```toml
[package]
name = "kryos-mcp-governed"
version = "0.1.0"
```

Copy `NORTHTEKDevs/kryos-mcp-template/main.kry` into `src/main.kry` verbatim. Verify it compiles:

```bash
kryos build --release src/main.kry
```

Expected: binary produced, no errors.

### Step 2 -- write cap_emit.kry

Create `src/cap_emit.kry` with `caps_node`, `caps_to_str`, and `tool_def_governed` as shown in the architecture section.

There are no imports needed -- all JSON builtins are in scope globally.

Verify with a minimal one-file test:

```bash
kryos run src/cap_emit.kry
```

Add a `main()` that calls `tool_def_governed("test", "desc", json_object([], []), ["net", "compute"])`, stringifies the result, and prints it. Expect JSON with `kryos_capabilities: ["net","compute"]` in the output.

### Step 3 -- write cap_check.kry

Create `src/cap_check.kry` with `ServerPolicy`, `ToolCapRecord`, `cap_allowed`, `check_tools`, `tool_cap`.

Write a test main that builds two records -- one within allowlist, one exceeding it -- and calls `check_tools`. Expect the exceeding tool to print a WARN line and return `false`.

```bash
kryos run src/cap_check.kry
```

### Step 4 -- write tools.kry

Create `src/tools.kry` with `tool_add`, `tool_fetch`, `tool_summarize` as shown.

For `tool_fetch`, use the `http_request` builtin (same as `std::http`; `http_get` is a thin wrapper). For local testing replace the network call with a hardcoded string so the test does not require outbound access.

```bash
kryos run src/tools.kry
```

Verify each tool function compiles without capability errors. If `tool_fetch` (annotated `@capabilities(net)`) tried to call `file_write`, the compiler should emit an error like `E0502`. Confirm by temporarily adding a `file_write("/tmp/x", "y")` call and checking the error, then remove it.

### Step 5 -- integrate into main.kry

Edit `src/main.kry`:

1. Replace the three `// EDIT HERE` sections with calls to the functions in `tools.kry` and `cap_emit.kry`.
2. Replace `tool_def(...)` calls with `tool_def_governed(...)`.
3. Add `startup_check()` at the top of `main()`.
4. Add `split_csv` helper and `server_policy()` function.

Build the final binary:

```bash
kryos build --release src/main.kry
```

### Step 6 -- write lineage.kry (optional)

Create `src/lineage.kry` with a `wrap_tracked(value: str, source: str) -> str` helper that calls `tracked_source`, optionally `annotate`, then `to_json` and returns the JSON string. Update `tool_fetch` to use it.

```bash
kryos run src/tools.kry  # with lineage enabled
```

Verify the output includes `"lineage"` key with at least one entry.

### Step 7 -- write the README and example config

`examples/claude_desktop_config.example.json`:

```json
{
  "mcpServers": {
    "kryos-governed": {
      "command": "/path/to/kryos-mcp-governed",
      "env": {
        "ANTHROPIC_API_KEY": "sk-...",
        "KRYOS_MCP_CAPS": "net,compute"
      }
    }
  }
}
```

README must include: what `kryos_capabilities` looks like in the tools/list response, how to set `KRYOS_MCP_CAPS`, and the honest disclaimer that `--strict-capabilities` (unannotated helper constraint) is not yet implemented.

### Step 8 -- integration test

Install the binary in Claude Desktop using the example config. Open a new conversation and ask Claude to list the available tools. Verify:

- The tool description for "fetch" contains `[caps: net]`.
- The tool description for "add" contains `[caps: compute]`.
- Calling "add" returns the correct sum.
- Calling "fetch" with a real URL returns the body (or the tracked JSON if lineage is enabled).
- Setting `KRYOS_MCP_CAPS=compute` and restarting logs a WARN for "fetch" and "summarize" at startup.

---

## Success Criteria / Demo Script

The demo should be runnable in under 5 minutes:

1. Start server: `KRYOS_MCP_CAPS=net,compute ./kryos-mcp-governed`
2. Send a `tools/list` JSON-RPC request (pipe it to stdin or use a test script):
   ```bash
   echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | ./kryos-mcp-governed
   ```
   Expected response includes `"kryos_capabilities":["net"]` on the fetch tool and `"kryos_capabilities":["compute"]` on add.
3. Call the add tool and get the correct result.
4. Call the fetch tool on `https://example.com` and get the body (or tracked JSON).
5. Restart with `KRYOS_MCP_CAPS=compute` -- startup prints:
   ```
   WARN: tool 'fetch' declares cap 'net' not in server allowlist
   WARN: tool 'summarize' declares cap 'net' not in server allowlist
   ```
6. In Claude Desktop: show that the tool description for "fetch" says `[caps: net]` so the user knows before they call it what the tool accesses.

The Kryos-specific claim to prove: open `tools.kry` and temporarily add `file_write("/tmp/x", "y")` to `tool_fetch` (which is only annotated `@capabilities(net)`). Run `kryos build --release src/main.kry`. The compiler rejects with `E0502: builtin 'file_write' requires capability 'io' not in declared set`. This is the demo moment: the tool cannot silently acquire file-write access.

---

## Risks and Honest Unknowns

### Risk 1: capability annotation is opt-in today

A tool function with NO `@capabilities` annotation is unconstrained. A developer who forgets to annotate gets no protection. This is the main honest gap between what the demo shows and full sandboxing. Mitigation: the README states this clearly; a build-time linter step (check that every exported tool function has `@capabilities`) can be added as a `Makefile` check without waiting for `--strict-capabilities`. Document the planned path to deny-by-default.

### Risk 2: the capability strings in tool_def_governed are manual

Until the compiler gains attribute reflection, the developer writes `@capabilities(net)` on the function AND passes `["net"]` to `tool_def_governed`. If they diverge, the MCP host sees a lie. Mitigation: the README and template include a comment reminding developers to keep them in sync. A post-compile lint step using `kryos check` output could catch divergence in the full vision.

### Risk 3: MCP spec does not define "kryos_capabilities" as a standard field

We add it as a vendor extension. MCP hosts that do not understand it ignore it (the spec allows additional fields in tool definitions). Hosts that do understand it (a Kryos-aware MCP client, or Claude Desktop with a future capability-display feature) can render it. No breakage risk, but no guarantee of uptake.

### Risk 4: http_get inside a tool function may fail on Windows without TLS cert setup

`http_get` calls `http_request` which uses the system TLS stack. On some Windows configurations this works out of the box; on others it requires a CA bundle path. The template README should note this. The `tool_summarize` function calls the Anthropic API over HTTPS and will surface this issue first. Mitigation: test on Windows before publishing; document the `SSL_CERT_FILE` env var workaround if needed.

### Risk 5: startup check is soft (warn, not throw) by default

The default behavior logs a WARN but continues. An operator who does not read the logs will not notice that tools exceed the declared surface. Mitigation: add a `KRYOS_MCP_STRICT=1` env var that switches from warn to `throw` (exits the process with a clear error message). Implement this in Step 4 alongside `server_policy()`.

### Unknown: does the Kryos compiler currently parse `@capabilities` and `@budget` on the same function correctly?

Both `@capabilities(net)` and `@budget(tokens=2000, calls=1)` on `tool_summarize` should stack. Verify this compiles without error in Step 4. If the attribute parser rejects stacked attributes, write them on separate lines (both backends support multi-attribute functions; verify in the test suite under `tests/`).
