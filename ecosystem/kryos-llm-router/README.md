# kryos-llm-router

A multi-provider LLM router that enforces **governance per route**. You register
providers, define routes with a selection policy, and every dispatch goes through
a single certified entry point that:

- **selects** a provider by policy (`cheapest` / `lowest_latency` / `capability_match`),
- is `@capabilities(net)` -- provably touches the network and **nothing else**
  (a stray `file_write`/`env_get` in a route is a *compile error*),
- is gated by a per-route `@budget(tokens, calls)` frame that **refuses before
  dispatch** when exhausted (the provider is never contacted), and
- returns the answer as a `Tracked<str>` whose **provenance names the provider**
  that produced it.

## Why Kryos

A router in TypeScript or Python can pick a provider and tally costs, but the
three things that make a router *trustworthy* live outside the language and rot:

| Guarantee | Elsewhere | Here |
|---|---|---|
| "this route only talks to the network" | code review, hope | `@capabilities(net)` -- a route that grows an io/process effect **does not compile** |
| "this route cannot overspend" | a counter someone remembers to check | `@budget(tokens, calls)` -- a runtime frame the language enforces; an exhausted budget **throws/refuses before dispatch** |
| "which provider produced this answer" | a side log you have to trust | `Tracked<str>` -- the answer *carries* its lineage: route -> selection -> provider |

No shipping language puts capability authority, a spend budget, and value
provenance in one function signature. Kryos does, and this router is what that
looks like for the most common AI-infra component.

```kryos
@capabilities(net)
@budget(tokens = 100000, calls = 8)
fn governed_route(providers: [Provider], route: Route, cfg: LlmConfig, msg: str, est_tokens: i64) -> RouteOutcome
```

## How a route flows

1. `select_provider(providers, route)` -- pure compute. Filters to providers that
   support the route's required features and fit the cost ceiling, then applies
   the policy. Returns `-1` if nothing is eligible (the route is refused, not crashed).
2. **Budget gate, before any dispatch.** The router asks the active `@budget`
   frame (the same primitive `std::llm::chat` charges): is there a model call
   left? would the estimated tokens cross the ceiling? If exhausted, it **refuses
   and returns** -- the transport is never invoked.
3. **Dispatch** to the selected provider, then charge the actual token usage
   against the frame.
4. Return a `RouteOutcome` whose `answer: Tracked<str>` lineage records the route,
   the selection decision (provider, policy, granted capabilities, cost), and the
   dispatch. `provenance_names_provider(outcome, "beta")` answers "who produced
   this" from the value itself.

### The two fields the spec leans on

- **`supports`** -- the *features* a provider can serve (`"chat"`, `"tools"`,
  `"vision"`, `"json"`, `"long_context"`). A route's `requires` is matched against this.
- **`caps`** -- the Kryos *governance capabilities* a provider is granted
  (`["net"]` for a hosted API, `["net","process"]` for one that runs tools
  locally). This travels into the answer's provenance, so the lineage records the
  authority under which the answer was produced.

The `capability_match` policy picks the **least over-provisioned** eligible
provider (the smallest feature set that still satisfies the route) -- least
surplus authority for the job.

## Layout

| File | Role | Capability surface |
|---|---|---|
| `src/routing.kry` | pure policy engine: `Provider`/`Route`, `select_provider`, eligibility, cost | `@capabilities()` -- empty, provably pure |
| `src/router.kry` | governance layer: `RouteOutcome`, `route_run`, `governed_route`, provenance | `@capabilities(net)` on the entry points only |
| `tests/test_routing.kry` | `@test` gates for selection (offline, `kryos test`) | -- |
| `router_verify.kry` | `kryos run` driver: `@budget` refusal + `Tracked` provenance (offline) | -- |
| `fixtures/leaky_route.kry` | compile-fail proof: io in a net-only route is rejected | -- |

`Tracked<str>` is built by direct struct construction (logical order, no
`time_now`) rather than `std::tracked::tracked_source`, so the net-only envelope
stays clean of the `time` capability under `--strict-capabilities`.

## Run it

```bash
KRYOS=C:/Users/Krist/projects/active/kryos-lang/compiler/target/release/kryos.exe
cd ecosystem/kryos-llm-router

# 1. governance surface: @budget refusal + Tracked provenance (offline, mock transport)
"$KRYOS" run router_verify.kry

# 2. pure policy engine: selection / eligibility (offline)
"$KRYOS" test --path tests

# 3. strict-capabilities: the whole library is net-only (no io/process/time)
"$KRYOS" check --strict-capabilities src/router.kry

# 4. compile-fail proof: a net-only route may NOT touch the filesystem
"$KRYOS" check fixtures/leaky_route.kry      # -> error[E0505]: file_write requires `io`
```

Everything is offline: a mock transport that **counts its invocations** stands in
for the network, so the budget refusal is proven by showing the transport is
never called once the frame is exhausted. No API key is read, no provider is
contacted.

## Test evidence (actual output)

`kryos run router_verify.kry` -- exit 0:

```
== kryos-llm-router governance verification ==
[1] policy selection
  ok   cheapest policy routes to beta
  ok   lowest_latency policy routes to gamma
  ok   capability_match policy routes to alpha
[2] provenance lineage names the provider
  ok   answer value comes from the selected provider
  ok   provenance NAMES the producing provider (beta)
  ok   provenance does NOT name a provider that did not produce it
  ...
[3] @budget call-axis refusal before dispatch
  ok   3rd route is REFUSED (budget exhausted)
  ok   transport invoked exactly TWICE -- 3rd refused BEFORE dispatch
  ok   refusal reason is the @budget exhaustion
[4] @budget token-axis refusal before dispatch
  ok   an over-estimate route is refused on the token axis
  ok   transport NEVER invoked -- refused BEFORE dispatch
[5] generous budget control
  ok   a generous budget lets the route complete
[6] unroutable request is refused with provenance
  ok   a route no provider supports is refused
== ALL 24 CHECKS PASSED ==
```

`kryos test --path tests` -- exit 0:

```
  PASS test_cheapest_picks_lowest_cost (0.1ms)
  PASS test_lowest_latency_picks_fastest (0.0ms)
  PASS test_capability_match_picks_tightest_fit (0.0ms)
  PASS test_required_feature_filters (0.0ms)
  PASS test_cost_ceiling_excludes (0.0ms)
  PASS test_no_eligible_returns_minus_one (0.0ms)
  PASS test_supports_all_and_cost (0.0ms)
Tests: 7 passed, 0 failed, 0 skipped, 7 total
```

`kryos check fixtures/leaky_route.kry` -- exit 1 (the compile-time guarantee):

```
error[E0505]: builtin `file_write` requires `io` capability
 --> ecosystem/kryos-llm-router/fixtures/leaky_route.kry:22:5
error: check failed: 1 error, 0 warnings
```

### Negative control

The `kryos run` driver fails loudly -- a green run is meaningful. Inverting one
assertion (`DISPATCH_COUNT == 2` -> `== 3`) on a copy:

```
  FAIL transport invoked exactly TWICE -- 3rd refused BEFORE dispatch
[exit 1]
```

Then the unmodified driver returns to `== ALL 24 CHECKS PASSED ==` (exit 0).

## Honest limitations / unknowns

- **Offline by construction.** The proofs use a mock transport. `governed_route`
  is the live entry point (it injects `std::llm::chat`), but no test contacts a
  real provider -- the refusal proof works precisely because the transport is
  never reached when the budget is exhausted, so its identity (mock vs `chat`)
  does not matter to the guarantee.
- **The runtime refusal surfaces as a value, not an exception.** `route_run`
  consults the budget frame (`kryos_budget_try_call` -- the same hook `chat`
  uses) and returns a `RouteOutcome { refused: true, ... }` rather than throwing.
  That is a deliberate API choice (a structured refusal is better router UX than
  an exception on every budget edge); the underlying `@budget` *attribute* still
  throws on exhaustion when a function lets it (that is the language primitive --
  see kryos-resilient-llm for the throw-path proof). Here, "refuse before
  dispatch" is proven by the transport-invocation count staying put.
- **`kryos run`, not `kryos build --release`, for the driver.** `governed_route`
  injects the live transport via a closure that captures an `LlmConfig`; the LLVM
  AOT backend currently miscompiles that struct-capturing lambda (a type
  mismatch), so the driver is JIT-only. Selection and the offline engine are
  backend-clean; only the live-`chat` closure path is AOT-affected, and tests
  never take it.
- **Cost is integer micro-USD.** `to_string(f64)` is unsupported on the JIT, so
  money is computed and reported as `i64` micro-USD; `cost_per_1k` stays `f64`
  internally for comparison only (never stringified).
- **`est_tokens` is a caller estimate.** The token-axis pre-check compares the
  caller's estimate against the frame's remaining tokens. Actual usage is charged
  after dispatch; an estimate that is too low can still overshoot, which is
  recorded as a `budget_exceeded` lineage entry on the (already produced) answer.
- **Selection is single-shot, synchronous.** No failover-on-error, no streaming,
  no concurrent fan-out across providers, and the latency value is a static
  per-provider hint, not a live measurement. Those are deliberate non-goals for
  this MVP.

## License

Apache-2.0 (see `LICENSE`).
