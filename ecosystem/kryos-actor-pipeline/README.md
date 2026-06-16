# kryos-actor-pipeline

Typed, capability-attenuated actor pipeline stages.

A staged pipeline `source -> transform -> sink` where each stage is an actor —
one OS thread joined to its neighbours by an MPMC channel — and each stage
carries its own `@capabilities` annotation. The sink is the only stage granted
`io`; the source and transform are pure `compute`. That gives Erlang-style
per-actor isolation with native LLVM-AOT speed *and* a per-stage authority
contract the compiler can report.

Erlang has the actors but not the speed or the capability typing; Rust/tokio has
the speed but actors are a library with no capability story. Here the authority
contract is a first-class, machine-checkable property: `kryos manifest --caps`
prints exactly which stage may touch `io`.

## How it works

A pipeline is a linear chain of N stages joined by N+1 channels:

```
head --[stage 0]--> ch1 --[stage 1]--> ch2 ... --[stage N-1]--> tail
```

A `Stage` is `(name, caps, run)`:

- `name` — the stage's identifier
- `caps` — its declared capability set as runtime data (`[str]`), mirroring the
  compile-time `@capabilities` annotation on its launcher
- `run` — a `fn(i64, i64) -> i64` launcher: given `(in_ch, out_ch)` it `spawn`s
  one OS thread that drains `in_ch`, applies a transform, and forwards to `out_ch`

`pipeline_run(stages)` allocates the channels, launches every stage between
consecutive channels, and returns the `head`/`tail` handles. The driver is the
upstream producer: it `feed`s raw items into `head` and `drain`s ordered results
from `tail`.

```
let p = pipeline_run([ingest, square, report])
feed(p.head, items)            // send every item, then one end-of-stream marker
let results = drain(p.tail, len(items))
```

### Graceful shutdown (end-of-stream protocol)

Termination uses an in-band end-of-stream sentinel, `eos()` (reserved value
`i64::MIN`). The producer sends every item, then one `eos()`. Each stage forwards
`eos()` downstream and returns. The compiler injects `kryos_spawn_wait_all` at the
end of `main`, which joins every stage thread before the process exits — so the
pipeline drains and shuts down cleanly with no orphaned threads.

Why a sentinel and not `close`: the language-level `recv` builtin maps to
`kryos_chan_recv_i64`, which returns `0` on a closed-and-empty channel — the same
value a real `0` payload has. There is no way to distinguish "closed" from "the
value 0" through the blocking `recv` surface, and `close_chan` is not a linkable
symbol in this toolchain. An in-band sentinel is the portable, unambiguous
shutdown signal. **Constraint: payloads must never equal `eos()` (`i64::MIN`).**

### Capability contract

Each stage launcher carries `@capabilities(...)`. In the demo:

```
run_ingest   @capabilities(compute)   v -> v + 1     normalize tick to id
run_square   @capabilities(compute)   v -> v * v     the transform
run_report   @capabilities(io)        println; forward   the ONLY io stage
```

`kryos manifest --caps demo_pipeline.kry` reports exactly:

```
fn run_ingest: [compute]
fn run_report: [io]
fn run_square: [compute]
```

Only `run_report` carries `io`. `kryos manifest --caps --deny net demo_pipeline.kry`
exits 0 (no stage holds `net`); `--deny io` exits 1 (the sink does). The
capability names mirror the `Capability` enum in
`compiler/crates/kryos-capabilities/src/model.rs`.

## Layout

```
kryos.toml                 package manifest, [capabilities] allowed = ["compute", "io"]
src/stage.kry              Stage struct, eos protocol, feed/drain, caps_render
src/pipeline.kry           Pipeline, pipeline_run([Stage]), print_authority
demo_pipeline.kry          3-stage source -> transform -> sink demo (manifest target)
tests/test_pipeline.kry    multi-threaded end-to-end gate (run via `kryos run`)
tests/test_units.kry       @test unit gates for the pure surface (no spawn)
```

## Run it

```
kryos test --path ecosystem/kryos-actor-pipeline           # @test units + file compile gates
kryos run  ecosystem/kryos-actor-pipeline/tests/test_pipeline.kry   # multi-thread e2e gate
kryos run  ecosystem/kryos-actor-pipeline/demo_pipeline.kry          # the demo
kryos manifest --caps ecosystem/kryos-actor-pipeline/demo_pipeline.kry
```

## Testing note (toolchain limitation)

`kryos test`'s `@test` harness JIT currently **miscompiles `spawn` closures**: it
leaks the capture count into the closure's argument list and Cranelift rejects it
with `mismatched argument count for ... got 3, expected 2`. The same `spawn` code
compiles and runs correctly under `kryos run` (JIT) and `kryos build --release`
(LLVM AOT). A file that contains *any* `@test` function fails to compile if the
file contains *any* `spawn`, so the suite is split:

- **`tests/test_units.kry`** — `@test` gates for everything that does not spawn:
  the EOS sentinel, `caps_render`, `Stage` metadata, and `feed`/`drain` + channel
  FIFO ordering on a single channel. These run as real executed `@test` gates.
- **`tests/test_pipeline.kry`** — the multi-threaded 3-stage end-to-end run, with
  `assert` gates, driven by `kryos run` (and verified under `kryos build
  --release`). It also compiles cleanly as a `kryos test` "file test".

Both backends produce identical, correctly-ordered output (verified). See the
top of each test file for the exact commands.

## MVP scope

- `Stage` abstraction over `spawn` + `chan`: input channel, output channel,
  `run` launcher.
- Linear pipeline builder `pipeline_run([stage_a, stage_b, ...])`.
- Per-stage `@capabilities`; a demo where the sink is the only stage with `io`.
- Graceful shutdown via the EOS protocol + the auto-injected `kryos_spawn_wait_all`.
- Tests: a 3-stage pipeline processes N items end to end, in order.

## Out of scope (deferred)

- **Backpressure.** The runtime's channels are **unbounded** (a `Mutex` + `VecDeque`
  with no capacity limit). A slow sink cannot slow a fast source — the queue
  between them grows without bound. This is fine for finite batches; for an
  unbounded or bursty source a slow sink can balloon memory. Bounded channels are
  not implemented in the runtime today; **do not rely on backpressure here.**
- Fan-out / fan-in topologies — the chain is strictly linear (one producer, one
  consumer per channel, which is also what guarantees FIFO ordering).
- Supervision trees, restart strategies, distribution.
- Typed (non-`i64`) channel payloads. The channel ABI is `i64`-shaped; richer
  payloads would marshal through a side table.

## Notes on the data model

Stages are spawned by named `run` launchers that call a **named** transform (or
inline it). A transform cannot be a `fn`-value captured into the `spawn` block:
the codegen does not thread `fn`-typed captures into a spawn closure (it emits an
unresolved `<closure>` / `handler` symbol on both backends). `fn`-values *do*
work as struct fields called from normal code, which is why `pipeline_run` can
iterate `[Stage]` and call `stages[i].run(...)` — that call happens on the driver
thread, never inside a spawn.

## License

Apache-2.0. See `LICENSE`.
