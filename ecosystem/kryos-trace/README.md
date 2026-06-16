# kryos-trace

Capability-scoped, cost-aware distributed tracing for Kryos.

An OpenTelemetry span is a side-channel you can forget to open, and it tracks
latency and attributes -- not *authority* or *enforced spend*. Kryos already
makes both first-class: a function's capability set is inferred by the compiler,
and `std::cost::ComputeCost` accrues real spend. `kryos-trace` makes a span
carry **the capability surface in effect for that span** and **the ComputeCost
accrued within it**, then exports the trace as OTLP-shaped JSON lines. A trace
can then answer, structurally, a question OTLP cannot express: *did this span
exceed its authorized capability surface?*

```
chat_tools_governed        cap=net  own[tok=8   ...]  subtree[tok=608 calls=3 ms=728 $=3000e-6]
  step-1                   cap=net  own[tok=0   ...]  subtree[tok=420 calls=2 ms=475 $=2100e-6]
    llm-call               cap=net  own[tok=420 ...]  subtree[tok=420 calls=1 ms=380 $=2100e-6]
    tool:get_weather       cap=net  own[tok=0   ...]  subtree[tok=0   calls=1 ms=95  $=0e-6]
  step-2                   cap=net  ...
    llm-call               cap=net  own[tok=180 ...]  subtree[tok=180 calls=1 ms=240 $=900e-6]
  checkpoint               cap=io   own[tok=0   ...]  subtree[tok=0   calls=0 ms=12  $=0e-6]
```

## MVP scope

This is the MVP. It implements exactly:

- **Spans with nesting and parent ids.** `span_start(spans, parent_id, name, cap_tag)`
  appends a span; its id is its index; `parent_id = -1` marks a root. `span_end`
  closes it. Scoping is the **synchronous call stack**, made explicit by passing
  the parent's id -- see *Honest limitations* below.
- **ComputeCost per span.** `span_add_cost(spans, id, cost)` accumulates a
  `std::cost::ComputeCost` as the span's *own* cost. `span_subtree_cost` (and the
  scalar `span_subtree_tokens` / `_api_calls` / `_wall_ms` / `_money_micro`) roll
  up own + all descendants. **Invariant: a span's subtree cost equals its own
  cost plus the sum of its children's subtree costs.**
- **OTLP-shaped JSON-line export.** `tracer_to_otlp` (own cost) and
  `tracer_to_otlp_subtree` (rolled-up) emit one OTLP/JSON `Span` object per line
  (`traceId` / `spanId` / `parentSpanId` / `name` / `attributes[]`).
  `tracer_export_file` writes the lines to a file sink -- no collector.
- **Capability tag from a manifest sidecar.** `caps_for_function_from_file` and
  `caps_union_from_file` read a `kryos manifest --caps` sidecar and produce the
  compact `cap_tag` a span carries. `caps_subset_of(child, parent)` detects
  **capability escalation** -- a span reaching for authority its authorized
  surface does not declare.

## Honest limitations

- **Synchronous scoping only.** Kryos has no async executor, so there is no
  implicit/async context propagation and no cross-process W3C `traceparent`. A
  child span is attached by passing the parent's id explicitly. The abstraction
  is a call-stack tracer; it does not pretend to span tasks or processes.
- **OTLP-*shaped*, not OTLP.** One JSON object per span with the core OTLP Span
  fields, suitable for a file/stdout sink and offline inspection. No live
  gRPC/HTTP export, no resource/scope envelope, no sampling.
- **Cost is integer in the wire/rollup.** `ComputeCost` carries f64 fields;
  `to_string(f64)` is unsupported in the Cranelift JIT (`kryos test`), so the
  exporter truncates `wall_time_ms` to whole ms and scales `money_usd` to
  micro-USD (`money_micro_usd`). Token and api-call counts are exact.

## Composition

- **`std::cost`** -- `ComputeCost` is the cost type at every API boundary.
- **kryos-agent-loop** (merged) -- the demo traces the structure of its
  `chat_tools_governed` loop and tags spans from kryos-agent-loop's own
  `kryos manifest --caps` output.
- Shares the JSON-line sink + manifest-sidecar `cap_tag` discipline with
  **kryos-log** (merged) rather than reinventing it.

## Layout

```
src/span.kry          Span, tracer ops, own + subtree cost, parent ids
src/manifest_caps.kry kryos manifest --caps sidecar -> cap_tag; escalation check
src/otlp.kry          OTLP-shaped JSON-line exporter (+ file sink)
tests/test_trace.kry  parent-id, cost-rollup, cap-tag, escalation, OTLP-shape tests
tests/fixtures/agent_loop.caps.json   real `kryos manifest --caps` of kryos-agent-loop
demo_trace.kry        end-to-end: trace a governed loop, show cost-per-span, export OTLP
```

## Public API

### `src/span.kry`

| Function | Returns | Purpose |
| --- | --- | --- |
| `tracer_new()` | `[Span]` | an empty tracer |
| `span_start(spans, parent_id, name, cap_tag)` | `[Span]` | append a span (id == index; `-1` parent = root) |
| `last_id(spans)` | `i64` | id of the most-recently started span |
| `span_add_cost(spans, id, cost)` | `[Span]` | accumulate a `ComputeCost` into a span's own cost |
| `span_end(spans, id)` | `[Span]` | mark a span closed |
| `span_children(spans, id)` | `[i64]` | direct child ids |
| `span_own_cost` / `span_subtree_cost` | `ComputeCost` | own / rolled-up cost (call from an entry module) |
| `span_own_tokens` / `span_subtree_tokens` (and `_api_calls`, `_wall_ms`, `_money_micro`) | `i64` | scalar cost accessors (module-boundary safe) |

### `src/manifest_caps.kry`

| Function | Returns | Purpose |
| --- | --- | --- |
| `caps_for_function(json, name)` / `caps_for_function_from_file(path, name)` | `str` | one function's cap tag from a manifest |
| `caps_union(json)` / `caps_union_from_file(path)` | `str` | the module's whole capability surface |
| `cap_tag_parts(tag)` | `[str]` | split a compact tag back into parts |
| `caps_subset_of(child_tag, parent_tag)` | `bool` | escalation check: is child within parent? |

### `src/otlp.kry`

| Function | Returns | Purpose |
| --- | --- | --- |
| `tracer_to_otlp(spans, trace_no)` | `str` | OTLP JSON lines, own-cost view |
| `tracer_to_otlp_subtree(spans, trace_no)` | `str` | OTLP JSON lines, subtree-cost view |
| `tracer_export_file(path, spans, trace_no)` | `i64` | write the own-cost lines to a file sink |

## A note on the module-boundary cost accessors

`span_subtree_cost` returns a freshly-built `ComputeCost`. Returning a newly
constructed struct from one module and reading its scalar fields in *another*
module miscompiles under the current Cranelift JIT (the receiving module reads
the i64 fields at the wrong offsets). The scalar accessors (`span_subtree_tokens`
and friends) return `i64`, which is stable across module boundaries, so the OTLP
exporter pulls cost through those. `span_subtree_cost` is exact when called from
an entry/same-module caller (the tests exercise both paths).

## Run

```bash
# from the repository root
kryos test --path ecosystem/kryos-trace
kryos run  ecosystem/kryos-trace/demo_trace.kry

# regenerate the agent-loop manifest fixture the demo/tests reference
kryos manifest --caps ecosystem/kryos-agent-loop/src/lib.kry \
  > ecosystem/kryos-trace/tests/fixtures/agent_loop.caps.json
```

## License

Apache-2.0. See `LICENSE`.
