# kryos-tool-broker

A capability-attenuated tool dispatcher for agents.

`kryos-agent-loop` runs the agent loop. `kryos-mcp-governed` verifies a server's
tools at startup. The missing piece is a *runtime* broker that, given an agent's
granted capability set, refuses to dispatch a tool whose declared capabilities
exceed that grant — enforcing least privilege per agent, per turn.

LangChain-style tool routers have no notion of a per-agent capability budget:
the grant is implicit, and any registered tool can run regardless of what the
calling agent was trusted with. Here the grant is a value (`[str]`), and the
refusal is a value (`Result::Err(DispatchError::Refused)`) — the runtime analogue
of the compiler's `CapabilitySet::is_subset_of` returning `false`.

## How it works

A `ToolBroker` holds a registry of tools, each a triple `(name, declared_caps, handler)`:

- `name` — the tool's identifier
- `declared_caps` — the capability set the handler needs (a `[str]`)
- `handler` — a `fn(str) -> str` (single string arg in, string out: the MVP tool ABI)

`dispatch(broker, agent_grant, name, args)` is the enforcement point:

| Condition | Outcome |
| --- | --- |
| name not registered | `Err(UnknownTool(name))` |
| `declared_caps` ⊄ `agent_grant` | `Err(Refused(detail))` — detail lists the excess caps |
| `declared_caps` ⊆ `agent_grant` | `Ok(handler(args))` |

Every dispatch — allowed or refused — appends one line to the broker's in-memory
audit log, so a caller can reconstruct what was attempted and why.

### Capability taxonomy

Capabilities are a compile-time concept in Kryos (the `@capabilities(...)`
annotation). To make a per-turn dispatch decision at runtime, the broker
re-encodes the taxonomy as plain data. The names mirror the `Capability` enum in
`compiler/crates/kryos-capabilities/src/model.rs` exactly:

```
net  io  ffi  compute  crypto  process  env  term  db  time   (10 concrete)
all                                                            (wildcard)
```

`all` in a grant is a wildcard that subsumes every concrete capability, matching
`Capability::All` in that crate.

### Attenuation

A tool that itself dispatches to sub-tools (a meta-tool) must not hand a child
*more* capability than it holds. `attenuate(parent_grant, requested)` enforces
the monotone-narrowing rule:

- `requested` ⊆ `parent_grant` → `Ok(requested)` (a valid narrowing)
- otherwise → `Err(excess)` (escalation, refused)

This mirrors the compiler's attenuation/escalation diagnostics (E0503 / E0504):
a child scope's capability set may only shrink.

## Layout

```
kryos.toml             package manifest, [capabilities] allowed = ["compute", "io"]
src/caps.kry           capability-set primitives (subset, excess, taxonomy, render)
src/broker.kry         ToolBroker, register, dispatch, audit
src/attenuate.kry      attenuate / attenuate_clamp for sub-delegation
demo_broker.kry        end-to-end demo (two agents, one turn, audit trail)
tests/test_broker.kry  7 @test functions
```

## Run it

```
kryos test --path ecosystem/kryos-tool-broker
kryos run  ecosystem/kryos-tool-broker/demo_broker.kry
```

The demo registers `summarize[compute]`, `fetch_url[net]`, `write_report[io]`,
then runs the same turn under a `{compute}`-only agent (net and io tools refused)
and a `{compute, net, io}` agent (all run), and prints the full audit trail.

## MVP scope

- `ToolBroker` registry of `(name, declared_caps, handler)`.
- `dispatch` refuses when a tool's declared caps are not a subset of the grant.
- Attenuation: a meta-tool sub-delegates only within its own grant.
- One audit entry per dispatch (allowed or refused), composable with the
  agent-loop audit trail.
- Tests: an over-privileged tool is refused; an in-grant tool runs; the audit
  records both.

## Out of scope (deferred)

- Dynamic capability negotiation (the grant is fixed per call here).
- Network tool transport — handlers are in-process `fn(str) -> str`.
- Per-call budgets (tokens/calls/wall) — compose with `@budget` separately;
  see the build notes on `@budget` placement sensitivity.
- Richer tool ABIs (typed args, multiple parameters). The MVP handler is
  `fn(str) -> str`.

## Notes on the data model

The registry uses parallel arrays (`names`, `decls`, `handlers`) rather than a
`[Tool]` of structs, because a struct field holding a `[str]` inside an array of
structs is a nested aggregate that is fragile across the two backends. Parallel
arrays keep the model portable. The audit log is a `[str]` threaded back through
`DispatchResult.broker`; persisting it to disk or JSON is left to the caller
(compose with `kryos-audit-trail`).

## License

Apache-2.0. See `LICENSE`.
