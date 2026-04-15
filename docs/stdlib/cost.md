# std::cost

Budget tracking and compute cost accounting for AI-powered applications. Provides structured cost records, budget enforcement with hard limits, and a cost tracker for aggregating usage across API calls, token consumption, and wall time.

```kryos
use std::cost
```

---

## Types

### ComputeCost

A snapshot of resource consumption for a single operation.

```kryos
struct ComputeCost {
    wall_time_ms: f64,
    tokens_used:  i64,
    api_calls:    i64,
    money_usd:    f64,
    energy_kwh:   f64
}
```

---

### Budget

Tracks accumulated spending against configured limits. `charge` throws `"BudgetExceeded"` when any limit is crossed.

```kryos
struct Budget {
    max_usd:           f64,
    max_tokens:        i64,
    max_api_calls:     i64,
    spent_usd:         f64,
    spent_tokens:      i64,
    spent_api_calls:   i64
}
```

---

### CostTracker

Aggregates multiple cost records against a budget.

```kryos
struct CostTracker {
    total:   ComputeCost,
    budget:  Budget,
    entries: [ComputeCost]
}
```

---

## ComputeCost Functions

### cost_zero

`cost_zero() -> ComputeCost`

Return a `ComputeCost` with all fields set to zero. Use as a starting accumulator.

---

### cost_add

`cost_add(a: ComputeCost, b: ComputeCost) -> ComputeCost`

Return a new `ComputeCost` whose fields are the element-wise sum of `a` and `b`.

**Example:**
```kryos
use std::cost

let c1 = ComputeCost { wall_time_ms: 120.0, tokens_used: 500, api_calls: 1, money_usd: 0.01, energy_kwh: 0.0001 }
let c2 = ComputeCost { wall_time_ms: 80.0,  tokens_used: 300, api_calls: 1, money_usd: 0.006, energy_kwh: 0.00006 }

let total = cost_add(c1, c2)
println(total.tokens_used)   // 800
println(total.money_usd)     // 0.016
```

---

### to_string (method)

`to_string() -> str`

Render the cost as a human-readable summary string.

**Example:**
```kryos
use std::cost

let c = ComputeCost { wall_time_ms: 250.0, tokens_used: 1200, api_calls: 3, money_usd: 0.024, energy_kwh: 0.0003 }
println(c.to_string())
// wall: 250ms | tokens: 1200 | calls: 3 | $0.0240 | 0.3000mWh
```

---

## Budget Functions

### budget_new

`budget_new(max_usd: f64, max_tokens: i64, max_api_calls: i64) -> Budget`

Create a `Budget` with the given hard limits. All `spent_*` fields start at zero.

---

### remaining_usd (method)

`remaining_usd() -> f64`

Return `max_usd - spent_usd`.

---

### remaining_tokens (method)

`remaining_tokens() -> i64`

Return `max_tokens - spent_tokens`.

---

### remaining_api_calls (method)

`remaining_api_calls() -> i64`

Return `max_api_calls - spent_api_calls`.

---

### is_exhausted (method)

`is_exhausted() -> bool`

Return `true` if any limit has been reached or exceeded.

---

### charge (method)

`charge(cost: ComputeCost)`

Add `cost` to the accumulated spend. Throws `"BudgetExceeded: <detail>"` if any limit is crossed after the charge.

---

### status (method)

`status() -> str`

Return a human-readable budget status string showing spent vs. limit for each dimension.

**Example:**
```kryos
use std::cost

let budget = budget_new(1.00, 50000, 100)

let op = ComputeCost { wall_time_ms: 0.0, tokens_used: 1200, api_calls: 2, money_usd: 0.024, energy_kwh: 0.0 }
budget.charge(op)

println(budget.remaining_usd())         // 0.976
println(budget.remaining_tokens())      // 48800
println(budget.is_exhausted())          // false
println(budget.status())
```

---

## CostTracker Functions

### cost_tracker_new

`cost_tracker_new(budget: Budget) -> CostTracker`

Create a new tracker bound to `budget`. `total` starts as `cost_zero()` and `entries` starts empty.

---

### record (method)

`record(cost: ComputeCost)`

Append `cost` to `entries`, add it to `total`, and call `budget.charge(cost)`.

---

### record_tokens (method)

`record_tokens(count: i64, cost_per_token: f64)`

Convenience method: record a cost of `count` tokens at `cost_per_token` USD per token. Sets `api_calls` to 0 and `wall_time_ms`/`energy_kwh` to 0.

---

### record_api_call (method)

`record_api_call(cost_usd: f64, tokens: i64)`

Convenience method: record a single API call costing `cost_usd` and consuming `tokens` tokens.

**Example:**
```kryos
use std::cost

let budget = budget_new(5.00, 200000, 500)
let tracker = cost_tracker_new(budget)

tracker.record_api_call(0.03, 1500)
tracker.record_api_call(0.02, 900)
tracker.record_tokens(500, 0.000002)

println(tracker.total.api_calls)    // 2
println(tracker.total.tokens_used)  // 2900
println(tracker.total.money_usd)    // 0.051
```

---

## Complete Example

```kryos
use std::cost

// Set a per-session budget
let budget = budget_new(2.00, 100000, 200)
let tracker = cost_tracker_new(budget)

// Simulate a pipeline of AI calls
let calls = [
    {tokens: 800,  usd: 0.016},
    {tokens: 1200, usd: 0.024},
    {tokens: 600,  usd: 0.012}
]

let i = 0
while i < len(calls) {
    let c = calls[i]
    tracker.record_api_call(c["usd"], c["tokens"])
    i = i + 1
}

println(tracker.total.to_string())
println(budget.status())

if budget.is_exhausted() {
    println("budget limit reached -- halting pipeline")
}
```
