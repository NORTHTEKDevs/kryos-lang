# AI Runtime

Kryos has a built-in runtime for AI and machine learning workloads. Instead of bolting ML onto a general-purpose language, Kryos treats tensors, probability, agents, streams, lineage, and cost tracking as first-class concepts.

## Implementation Status

| Feature | Status | Backend |
|---------|--------|---------|
| Tensors (creation, math, reductions, linalg) | **Implemented** | Native Rust FFI (`kryos-rt/tensor.rs`) |
| ML ops (softmax, cross-entropy, MSE, relu, sigmoid) | **Implemented** | Native Rust FFI |
| Probable\<T\> | **Implemented** | Pure Kryos (`stdlib/probable.kry`) |
| Reactive Streams | **Implemented** | Pure Kryos (`stdlib/stream.kry`) |
| Data Lineage (Tracked) | **Implemented** | Pure Kryos (`stdlib/tracked.kry`) |
| Cost Tracking / Budget | **Implemented** | Pure Kryos (`stdlib/cost.kry`) |
| Agent framework | **Implemented** | Pure Kryos (`stdlib/agent.kry`) |
| Automatic Differentiation (GradTensor) | Roadmap | Requires computation graph |
| GPU acceleration | Roadmap | Requires CUDA/Metal backend |
| @differentiable decorator | Roadmap | Requires compiler support |
| @timed decorator | Roadmap | Requires compiler support |

## Tensors

The tensor runtime provides N-dimensional arrays with shape tracking, broadcasting, element-wise operations, reductions, linear algebra, and ML-specific ops. All tensor functions are backed by native Rust FFI in `kryos-rt/src/tensor.rs`, registered in both the Cranelift JIT and LLVM AOT backends.

All the functions below live in `std::tensor` and must be imported: `use std::tensor::{tensor_zeros, tensor_ones, ...}`. Internally each one wraps a raw `kryos_tensor_*` FFI extern (declared in the module itself) and marshals pointers/`f64` payloads correctly — do not hand-declare your own `extern { fn kryos_tensor_* }` block and call the raw externs directly: they expect a pointer to the array's *data buffer*, not the array handle/header that a plain cast yields, and scalar-returning raw externs hand back `f64` bits packed into an `i64` that only the module's internal decoder unpacks. Always go through the `tensor_*` wrapper functions.

### Creating Tensors

```
let z = tensor_zeros([2, 3])        // 2x3 tensor of zeros
let o = tensor_ones([4, 4])          // 4x4 tensor of ones
let r = tensor_rand([3, 3])          // uniform random [0, 1)
let n = tensor_randn([3, 3])         // normal distribution
let eye = tensor_eye(3)              // 3x3 identity matrix
let seq = tensor_arange(0, 10, 1)    // [0, 1, ..., 9] -- start/end/step are i64
```

Tensor handles are i64 values (pointers to heap-allocated `KryosTensor` structs). The runtime manages memory; call `tensor_free(handle)` when done.

### Element-Wise Operations

```
let sum = tensor_add(a, b)
let diff = tensor_sub(a, b)
let prod = tensor_mul(a, b)
let quot = tensor_div(a, b)
let power = tensor_pow(a, b)
let scaled = tensor_scale(t, 2.0)
```

Broadcasting supports same-shape and scalar operations.

### Unary Math Operations

```
tensor_exp(t)       // e^x
tensor_log(t)       // natural log
tensor_sqrt(t)      // square root
tensor_tanh(t)      // hyperbolic tangent
tensor_sigmoid(t)   // 1 / (1 + e^-x)
tensor_relu(t)      // max(0, x)
tensor_neg(t)       // -x
```

### Reductions

```
tensor_sum(t)       // sum all elements -> f64
tensor_mean(t)      // mean -> f64
tensor_max(t)       // max element -> f64
tensor_min(t)       // min element -> f64
tensor_argmax(t)    // index of max -> i64
tensor_argmin(t)    // index of min -> i64
```

These already return real `f64`/`i64` values — the wrapper decodes the raw FFI's bit-packed scalar internally, so no manual unpacking is needed.

### Linear Algebra

```
let c = tensor_matmul(a, b)    // matrix multiply
let t = tensor_transpose(a)     // 2D transpose
```

Matrix multiplication supports:
- 2D x 2D: `[M,K] x [K,N] -> [M,N]`
- 2D x 1D: `[M,K] x [K] -> [M]`
- 1D x 1D: dot product -> scalar (1-element tensor)

### Shape Operations

```
tensor_reshape(t, new_shape)   // reshape (supports -1 inference); ndim comes from len(new_shape)
tensor_flatten(t)              // flatten to 1D
tensor_ndim(t)                 // number of dimensions
tensor_numel(t)                // total element count
tensor_shape_dim(t, dim)       // size of dimension
```

### ML-Specific Operations

```
tensor_softmax(logits, dim)             // softmax along last dim (dim = -1)
tensor_cross_entropy(logits, targets)   // cross-entropy loss
tensor_mse_loss(predictions, actuals)   // mean squared error
```

`tensor_softmax` returns a normal tensor handle you can pass to `tensor_get`/`tensor_numel`/etc. `tensor_cross_entropy` and `tensor_mse_loss` are the one exception to the "reductions already decode to f64" rule above: they return the raw scalar loss as `f64` bits packed into an `i64` (there is currently no public function to unpack it), so **do not** pass their return value to `tensor_get`/`tensor_free`/`tensor_numel` — doing so treats the bit pattern as a pointer and crashes. Use them only to feed a value you print as an opaque diagnostic (`to_string(loss_handle)`) or as a placeholder until a public decoder is added.

### Neural Network Example

```
use std::tensor::{tensor_rand, tensor_randn, tensor_matmul, tensor_relu, tensor_softmax, tensor_numel}

fn main() {
    // 2-layer network: input(4) -> hidden(8) -> output(3)
    let x = tensor_rand([2, 4])
    let w1 = tensor_randn([4, 8])
    let w2 = tensor_randn([8, 3])

    // Forward: relu(X @ W1) then softmax(H @ W2)
    let hidden = tensor_relu(tensor_matmul(x, w1))
    let probs = tensor_softmax(tensor_matmul(hidden, w2), -1)

    println("Output: " + to_string(tensor_numel(probs)) + " probabilities")
}
```

Output:

```
Output: 6 probabilities
```

(2 rows x 3 classes = 6 elements; each row of `probs` sums to 1.0.)

## Automatic Differentiation (Roadmap)

Reverse-mode autodiff through `GradTensor` is planned. The design wraps tensors in gradient-tracking wrappers, builds a computation graph during forward pass, and computes gradients via `.backward()`.

Target API:

```
@differentiable

let x = GradTensor.from_list([2.0, 3.0])
let w = GradTensor.from_list([0.5, -0.5])
let loss = (x * w).sum()
loss.backward()
// x.grad = [0.5, -0.5], w.grad = [2.0, 3.0]
```

This requires a computation graph runtime and compiler support for the `@differentiable` decorator. Both are on the roadmap.

## Agents

Agents are first-class autonomous entities with persistent memory, tools, alignment modes, and audit trails. Implemented in `stdlib/agent.kry`.

### Creating an Agent

```
let agent = agent_new("researcher", "Find relevant papers")
```

### Agent Memory

Every agent has three types of memory:

- **Working memory**: Short-term, cleared between tasks
- **Semantic memory**: Learned facts that persist across tasks
- **Episodic memory**: Append-only log of past actions with timestamps

```
let mut agent = agent   // mutating a field requires `let mut`
agent.memory = agent.memory.remember("query", "transformers", "working")
let value = agent.memory.recall("query")
agent.memory = agent.memory.clear_working()
```

### Alignment Modes

```
let safe_agent = agent_with_alignment("assistant", "Help users", ALIGNMENT_STRICT)
let standard = agent_with_alignment("worker", "Process data", ALIGNMENT_STANDARD)
let minimal = agent_with_alignment("scraper", "Collect data", ALIGNMENT_MINIMAL)
let full = agent_with_alignment("autonomous", "Run free", ALIGNMENT_UNRESTRICTED)
```

### Tools

```
fn web_search(query: str) -> str {
    // ... implementation
    return "results for " + query
}

let agent = agent.add_tool("search", web_search, "Search the web")
let result = agent.use_tool("search", "Kryos documentation")
```

### Child Agents and Swarms

```
let child = agent.spawn_child("worker", "Process batch 1")

let agent_1 = agent_new("a1", "task1")
let agent_2 = agent_new("a2", "task2")
let swarm = agent_swarm("analysis_team")
let swarm = swarm.add(agent_1)
let swarm = swarm.add(agent_2)
```

### Lifecycle

```
let mut agent = agent
agent = agent.pause()
agent = agent.resume()
agent = agent.terminate()
```

Agent states: `CREATED`, `RUNNING`, `PAUSED`, `COMPLETED`, `FAILED`, `TERMINATED`.

## Probable\<T\>

Confidence-aware values for AI predictions. Implemented in `stdlib/probable.kry`. Operations are free generic functions, not methods — there is no `impl Probable<T>` block (the checker doesn't yet support generic `impl`), so call `is_confident(p, x)` rather than `p.is_confident(x)`.

```
use std::probable::{probable, certain, is_confident, or_else, require_confidence}

let result = probable("cat", 0.92)
let sure = certain("yes")

if is_confident(result, 0.8) {
    // act on the result
}

let safe = or_else(result, "unknown")
let required = require_confidence(result, 0.9)  // throws if below
println(result.value)         // "cat" -- fields are still accessed with `.`
println(to_string(result.confidence))  // 0.92
```

### Ensemble

```
use std::probable::{majority_vote, best_of}

let consensus = majority_vote(predictions)   // highest summed-confidence value wins
let best = best_of(predictions)              // single highest-confidence prediction
```

## Reactive Streams

Lazy, composable stream processing. Implemented in `stdlib/stream.kry`.

```
let result = stream_from_range(0, 1000)
    .filter(fn(x) { return x % 2 == 0 })
    .map(fn(x) { return x * x })
    .take(10)
    .collect()
```

Available operations: `map`, `filter`, `take`, `skip`, `collect`, `reduce`, `count`, `first`, `last`, `for_each`, `any`, `all`, `sum`, `min`, `max`.

## Data Lineage

Track data provenance for AI safety and compliance. Implemented in `stdlib/tracked.kry`. Like `Probable<T>`, `Tracked<T>` operations are free functions, not methods (no `impl Tracked<T>` block) — call `explain(t)`/`to_json(t)`, not `t.explain()`/`t.to_json()`.

```
use std::tracked::{tracked_source, transform, inference, explain, to_json}

let raw_data = "raw customer rows"
let data = tracked_source(raw_data, "database", "Customer records Q4")
let clean_result = "cleaned customer rows"
let cleaned = transform(data, clean_result, "clean", "Remove nulls")
let result = "prediction: churn=low"
let predicted = inference(cleaned, "gpt-4", result, 0.87)
println(explain(predicted))
let json = to_json(predicted)
```

## Cost Tracking

Budget enforcement for AI compute costs. Implemented in `stdlib/cost.kry`.

```
let budget = budget_new(10.0, 100000, 500)  // $10, 100k tokens, 500 API calls
let tracker = cost_tracker_new(budget)

let tracker = tracker.record_api_call(0.003, 1500)
println(tracker.total.to_string())

// Budget enforcement: throws BudgetExceeded if over limit
let budget = budget.charge(ComputeCost {
    wall_time_ms: 150.0,
    tokens_used: 1500,
    api_calls: 1,
    money_usd: 0.003,
    energy_kwh: 0.001
})
```
