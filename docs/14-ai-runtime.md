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

### Creating Tensors

```
let z = tensor_zeros([2, 3])        // 2x3 tensor of zeros
let o = tensor_ones([4, 4])          // 4x4 tensor of ones
let r = tensor_rand([3, 3])          // uniform random [0, 1)
let n = tensor_randn([3, 3])         // normal distribution
let eye = tensor_eye(3)              // 3x3 identity matrix
let seq = tensor_arange(0.0, 10.0, 1.0)  // [0, 1, ..., 9]
```

Tensor handles are i64 values (pointers to heap-allocated `KryosTensor` structs). The runtime manages memory; call `kryos_tensor_free(handle)` when done.

### Element-Wise Operations

```
let sum = kryos_tensor_add(a, b)
let diff = kryos_tensor_sub(a, b)
let prod = kryos_tensor_mul(a, b)
let quot = kryos_tensor_div(a, b)
let power = kryos_tensor_pow(a, b)
let scaled = kryos_tensor_scale(t, 2.0)
```

Broadcasting supports same-shape and scalar operations.

### Unary Math Operations

```
kryos_tensor_exp(t)       // e^x
kryos_tensor_log(t)       // natural log
kryos_tensor_sqrt(t)      // square root
kryos_tensor_tanh(t)      // hyperbolic tangent
kryos_tensor_sigmoid(t)   // 1 / (1 + e^-x)
kryos_tensor_relu(t)      // max(0, x)
kryos_tensor_neg(t)       // -x
```

### Reductions

```
kryos_tensor_sum(t)       // sum all elements -> f64 (as i64 bits)
kryos_tensor_mean(t)      // mean -> f64 (as i64 bits)
kryos_tensor_max(t)       // max element -> f64 (as i64 bits)
kryos_tensor_min(t)       // min element -> f64 (as i64 bits)
kryos_tensor_argmax(t)    // index of max
kryos_tensor_argmin(t)    // index of min
```

Note: scalar returns use the i64 slot model (f64 bits reinterpreted as i64).

### Linear Algebra

```
let c = kryos_tensor_matmul(a, b)    // matrix multiply
let t = kryos_tensor_transpose(a)     // 2D transpose
```

Matrix multiplication supports:
- 2D x 2D: `[M,K] x [K,N] -> [M,N]`
- 2D x 1D: `[M,K] x [K] -> [M]`
- 1D x 1D: dot product -> scalar

### Shape Operations

```
kryos_tensor_reshape(t, new_shape, ndim)  // reshape (supports -1 inference)
kryos_tensor_flatten(t)                    // flatten to 1D
kryos_tensor_ndim(t)                       // number of dimensions
kryos_tensor_numel(t)                      // total element count
kryos_tensor_shape_dim(t, dim)             // size of dimension
```

### ML-Specific Operations

```
kryos_tensor_softmax(logits, dim)          // softmax along last dim
kryos_tensor_cross_entropy(logits, targets) // cross-entropy loss
kryos_tensor_mse_loss(predictions, actuals) // mean squared error
```

### Neural Network Example

```
extern {
    fn kryos_tensor_rand(shape_ptr: i64, ndim: i64) -> i64
    fn kryos_tensor_randn(shape_ptr: i64, ndim: i64) -> i64
    fn kryos_tensor_matmul(a: i64, b: i64) -> i64
    fn kryos_tensor_relu(handle: i64) -> i64
    fn kryos_tensor_softmax(handle: i64, dim: i64) -> i64
    fn kryos_tensor_numel(handle: i64) -> i64
}

fn main() {
    // 2-layer network: input(4) -> hidden(8) -> output(3)
    let w1_shape = [4, 8]
    let w2_shape = [8, 3]
    let x_shape = [2, 4]

    let x = kryos_tensor_rand(x_shape as i64, 2)
    let w1 = kryos_tensor_randn(w1_shape as i64, 2)
    let w2 = kryos_tensor_randn(w2_shape as i64, 2)

    // Forward: relu(X @ W1) then softmax(H @ W2)
    let hidden = kryos_tensor_relu(kryos_tensor_matmul(x, w1))
    let probs = kryos_tensor_softmax(kryos_tensor_matmul(hidden, w2), -1)

    println("Output: " + to_string(kryos_tensor_numel(probs)) + " probabilities")
}
```

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
    return results
}

let agent = agent.add_tool("search", web_search, "Search the web")
let result = agent.use_tool("search", "Kryos documentation")
```

### Child Agents and Swarms

```
let child = agent.spawn_child("worker", "Process batch 1")

let swarm = agent_swarm("analysis_team")
let swarm = swarm.add(agent_1)
let swarm = swarm.add(agent_2)
```

### Lifecycle

```
agent = agent.pause()
agent = agent.resume()
agent = agent.terminate()
```

Agent states: `CREATED`, `RUNNING`, `PAUSED`, `COMPLETED`, `FAILED`, `TERMINATED`.

## Probable\<T\>

Confidence-aware values for AI predictions. Implemented in `stdlib/probable.kry`.

```
let result = probable("cat", 0.92)
let certain = probable_certain("yes")

if result.is_confident(0.8) {
    // act on the result
}

let safe = result.or_else("unknown")
let required = result.require_confidence(0.9)  // throws if below
println(result.explain())
```

### Ensemble

```
let consensus = ensemble_majority_vote(predictions)
let best = ensemble_best_confidence(predictions)
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

Track data provenance for AI safety and compliance. Implemented in `stdlib/tracked.kry`.

```
let data = tracked_source(raw_data, "database", "Customer records Q4")
let cleaned = data.transform(clean_result, "clean", "Remove nulls")
let predicted = cleaned.inference("gpt-4", result, 0.87)
println(predicted.explain())
let json = predicted.to_json()
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
