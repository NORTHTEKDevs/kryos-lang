# kryos-prompt-lineage

Prompt assembly as `Tracked<str>`. The assembled prompt remembers its parts:
which template, which variables, and which source each variable came from.
`explain(output)` then shows the full prompt provenance, not just the final
string.

`kryos-rag` tracks *retrieval* citations. Nothing tracks *prompt assembly* --
which template, which variables, which system message produced a given output.
A LangChain prompt template discards this: the assembled string has no memory
of its parts. Here the lineage IS the value's own history, so "show me exactly
what produced this output" is answerable at the prompt layer.

## What it does

- `template(role, text)` builds a `PromptTemplate`. Holes are written with
  escaped braces `{{name}}` in Kryos source (a bare `{name}` is string
  interpolation in Kryos, so holes must be escaped -- at runtime `{{name}}`
  becomes the literal hole `{name}`).
- `bind(tmpl, name, value)` binds a `Tracked<str>` to a hole. The value's
  `source` and `source_description` are pulled into the binding so they land in
  the assembled lineage. `bind_literal(tmpl, name, text)` binds a plain string
  with a `literal` source.
- `assemble(tmpl)` fills every hole and returns a `Tracked<str>`. Its lineage is
  gap-free: one `source` entry for the template, then one `bind` entry per
  variable, each carrying `var=<name> source=<src>` in its metadata.
- `to_audit_record(t, system_id, user_id)` emits a compact JSON record in the
  `kryos-audit-trail` lineage shape (`{step, operation, description, timestamp,
  metadata}` entries), so a prompt's provenance flows into an Annex IV record
  without re-deriving anything.
- `bound_count(t)` and `has_unfilled_holes(t)` are introspection helpers: assert
  every variable is accounted for and no hole is left unfilled before sending
  the prompt to a model.

## MVP scope

Implemented:

- `template` + `bind` -> `Tracked<str>` assembled prompt with a lineage entry
  per substitution.
- `{name}` interpolation under the hood (written `{{name}}` in source).
- `to_audit_record()` bridging to the kryos-audit-trail JSON shape.
- Demo: assemble a system+user prompt, call a mock model, `explain` the result
  (`demo_assemble.kry`); compose with rag citations via `tracked_merge`
  (`demo_rag_merge.kry`).
- Tests on lineage completeness (`tests/test_lineage.kry`).

Deferred (out of scope for MVP): partial templates / includes, multi-modal
content, per-fragment token counting.

## Run it

```
kryos run demo_assemble.kry
kryos run demo_rag_merge.kry
```

`demo_assemble.kry` output (the provenance chain reaches back through the
prompt into the model call):

```
=== explain(answer) -- full prompt provenance ===
Value: The capital of France is Paris.

Lineage:
  1. [source] template role=chat
     source=template:chat
  2. [bind] bind {policy} <- system message from the policy registry
     var=policy source=system-policy-v3
  3. [bind] bind {style} <- answer-length preset chosen in the UI
     var=style source=ui-preset:concise
  4. [bind] bind {country} <- country field submitted by the end user
     var=country source=user-form
  5. [model_call] mock model: claude-mock-1
```

## Composing with kryos-rag

`demo_rag_merge.kry` shows two retrieved chunks (each a `Tracked<str>` with its
own source) merged with `std::agent_bridge::tracked_merge`, bound into the
prompt, and the citations recovered with `tracked_to_citation`:

```
citations from merge:
  - doc:wikipedia/France
  - doc:worldbank/FR
```

Honest note on merge depth: `bind` records the bound value's *primary* source
(`source` / `source_description`) in the prompt lineage, and the full retrieval
citation list is recoverable from the merged value via `tracked_to_citation`.
The library does not inline every upstream chunk's lineage entries into the
prompt's own lineage array -- it records the binding event plus the fragment's
source. If you need the complete merged citation set inside the prompt record,
read it off the bound `Tracked` value before assembly (as the demo does).

## Testing notes

`kryos test --path ecosystem/kryos-prompt-lineage` is green. The tests build
only on `tracked_source` and this library -- they deliberately do **not** call
`std::tracked::inference`. `kryos test` eagerly JIT-compiles every function in
every imported module, and the polymorphic `inference___str` monomorphization
mis-compiles its `to_string(confidence: f64)` under that eager path (a known
upstream codegen bug, also documented in `kryos-audit-trail`). Avoiding
`inference` keeps the eager path clean. Each `@test` is also driven by a
`main()` in the test file, so `kryos run tests/test_lineage.kry` runs the same
assertions.

## Capabilities

`compute` only. The library does pure string assembly and JSON building -- no
`io`, `net`, or `process`.

## License

Apache-2.0.
