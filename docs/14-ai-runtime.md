# AI Runtime

> **Status:** The AI runtime described in this chapter is part of Kryos's language specification and stdlib design. These features were prototyped in the early Python-based interpreter. The native Rust compiler does not yet include these builtins -- they will be reimplemented as native stdlib modules backed by the Kryos runtime (not Python/numpy). This chapter documents the target API.

Kryos has a built-in runtime for AI and machine learning workloads. Instead of bolting ML onto a general-purpose language, Kryos treats tensors, probability, agents, streams, lineage, and cost tracking as first-class concepts. This chapter covers all of them.

## Tensors

The `KryosTensor` type is an N-dimensional array with shape tracking, broadcasting, element-wise operations, reductions, linear algebra, and ML-specific ops.

### Creating Tensors

```
// Zeros and ones
let z = tensor_zeros([2, 3])        // 2x3 tensor of zeros
let o = tensor_ones([4, 4])          // 4x4 tensor of ones

// Random
let r = tensor_rand([3, 3])          // uniform random [0, 1)
let n = tensor_randn([3, 3])         // normal distribution (mean=0, std=1)

// From data
let t = tensor_from_list([[1.0, 2.0], [3.0, 4.0]])

// Identity matrix
let eye = tensor_eye(3)              // 3x3 identity

// Range
let seq = tensor_arange(0.0, 10.0, 1.0)  // [0, 1, 2, ..., 9]
```

Every tensor has a `shape`, `dtype`, `ndim` (number of dimensions), and `numel` (total element count). The default dtype is `f32`. Supported dtypes: `f32`, `f64`, `i32`, `i64`, `bool`.

### Element-Wise Operations

Standard arithmetic works element-wise with broadcasting:

```
let a = tensor_from_list([1.0, 2.0, 3.0])
let b = tensor_from_list([4.0, 5.0, 6.0])

let sum = a + b           // [5.0, 7.0, 9.0]
let diff = a - b          // [-3.0, -3.0, -3.0]
let prod = a * b          // [4.0, 10.0, 18.0]
let quot = a / b          // [0.25, 0.4, 0.5]
let power = a ** b        // element-wise power
```

Scalar operations broadcast automatically:

```
let scaled = a * 2.0      // [2.0, 4.0, 6.0]
let shifted = a + 1.0     // [2.0, 3.0, 4.0]
```

Broadcasting follows numpy rules: dimensions are compared from the right, and a dimension of size 1 is stretched to match the other tensor.

### Unary Math Operations

```
let t = tensor_randn([3, 3])

t.exp()       // e^x for each element
t.log()       // natural log
t.sqrt()      // square root
t.tanh()      // hyperbolic tangent
t.sigmoid()   // 1 / (1 + e^-x) -- numerically stable
t.relu()      // max(0, x)
```

### Reductions

Reduce along an axis or across the entire tensor:

```
let t = tensor_from_list([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])

t.sum()             // 21.0 (scalar)
t.sum(axis: 0)      // [5.0, 7.0, 9.0]  (sum columns)
t.sum(axis: 1)      // [6.0, 15.0]       (sum rows)

t.mean()            // 3.5
t.max()             // 6.0
t.min()             // 1.0
t.argmax()          // index of max element
t.argmin()          // index of min element
```

### Linear Algebra

```
let a = tensor_from_list([[1.0, 2.0], [3.0, 4.0]])
let b = tensor_from_list([[5.0, 6.0], [7.0, 8.0]])

// Matrix multiplication
let c = tensor_matmul(a, b)   // or: a @ b

// Dot product (1-D tensors only)
let v1 = tensor_from_list([1.0, 2.0, 3.0])
let v2 = tensor_from_list([4.0, 5.0, 6.0])
let d = v1.dot(v2)            // 32.0

// Transpose
let t = a.T                   // or: a.transpose()
```

Matrix multiplication supports:
- 2-D x 2-D (standard matrix multiply)
- 2-D x 1-D (matrix-vector product)
- 1-D x 2-D (vector-matrix product)
- 1-D x 1-D (dot product, returns scalar)

### Shape Operations

```
let t = tensor_rand([2, 3, 4])

t.reshape([6, 4])          // reshape to 6x4
t.reshape([-1, 4])         // infer first dim: 6x4
t.flatten()                // flatten to 1-D: (24,)
t.view([2, 12])            // alias for reshape

t.squeeze()                // remove all size-1 dims
t.squeeze(dim: 1)          // remove dim 1 if it is size 1
t.unsqueeze(0)             // add a dim of size 1 at position 0

// Concatenate tensors
let combined = tensor_cat([t1, t2], dim: 0)

// Stack tensors (adds new dimension)
let stacked = KryosTensor.stack([t1, t2], dim: 0)

// Split a tensor
let chunks = t.split(sections: 3, dim: 0)

// Slice along a dimension
let sliced = t.slice(dim: 0, start: 0, end: 2)
```

### ML-Specific Operations

```
let logits = tensor_from_list([[2.0, 1.0, 0.1]])

// Softmax -- converts logits to probabilities
let probs = tensor_softmax(logits, -1)    // sums to 1.0

// Layer normalization
let normalized = logits.layer_norm(eps: 1e-5)

// Cross-entropy loss (logits + target class indices)
let targets = tensor_from_list([0])
let loss = logits.cross_entropy(targets)

// Mean squared error loss
let predictions = tensor_from_list([1.0, 2.0, 3.0])
let actuals = tensor_from_list([1.1, 2.2, 2.9])
let mse = predictions.mse_loss(actuals)

// Dropout (training regularization)
let dropped = t.dropout(p: 0.5, training: true)
```

## Automatic Differentiation

Kryos supports reverse-mode autodiff through `GradTensor`. Wrap a tensor in a `GradTensor`, perform forward operations, then call `.backward()` to compute gradients. This is the foundation for training neural networks.

### Basic Gradient Computation

```
@differentiable

// Create differentiable tensors
let x = GradTensor.from_list([2.0, 3.0])
let w = GradTensor.from_list([0.5, -0.5])

// Forward pass
let y = x * w           // element-wise multiply
let loss = y.sum()       // scalar loss

// Backward pass -- computes gradients
loss.backward()

// x.grad and w.grad now hold the partial derivatives
// dL/dx = w = [0.5, -0.5]
// dL/dw = x = [2.0, 3.0]
```

### Supported Gradient Operations

The following operations track gradients through the computation graph:

| Operation | Forward | Gradient |
|-----------|---------|----------|
| `a + b` | element-wise add | pass-through (both) |
| `a - b` | element-wise subtract | pass-through / negate |
| `a * b` | element-wise multiply | cross-multiply |
| `a / b` | element-wise divide | 1/b, -a/b^2 |
| `-a` | negate | negate gradient |
| `a @ b` | matrix multiply | dL/dA = dL/dC @ B^T, dL/dB = A^T @ dL/dC |
| `.relu()` | max(0, x) | 1 where x > 0, else 0 |
| `.sum()` | sum all elements | broadcast ones |
| `.mean()` | average | 1/n for each element |
| `.softmax()` | softmax probabilities | Jacobian: s * (grad - sum(grad * s)) |

Gradients handle broadcasting correctly. When shapes differ, gradients are summed over the broadcast dimensions to match the original tensor's shape.

### Training Loop Pattern

```
@differentiable

// Initialize weights
let mut w1 = GradTensor.randn([2, 4])
let mut w2 = GradTensor.randn([4, 1])
let learning_rate = 0.01

for epoch in range(0, 100) {
    // Forward
    let hidden = (x @ w1).relu()
    let output = hidden @ w2
    let loss = output.mse_loss(target)

    // Backward
    loss.backward()

    // Update weights (gradient descent)
    // w1 = w1 - learning_rate * w1.grad
    // w2 = w2 - learning_rate * w2.grad

    // Zero gradients for next iteration
    w1.zero_grad()
    w2.zero_grad()
}
```

## Agents

Agents are first-class autonomous entities in Kryos. Unlike simple functions or actors, agents have persistent memory, tools, alignment modes, and a full audit trail. They can spawn child agents and coordinate in swarms.

### Creating an Agent

```
let agent = Agent("researcher", goal: "Find relevant papers")

// Add tools the agent can use
agent.add_tool("search", search_fn, description: "Search the web")
agent.add_tool("summarize", summarize_fn, description: "Summarize text")

// Execute
let result = agent.execute("Find papers on transformer architectures")
```

### Agent Memory

Every agent has three types of memory:

```
// Working memory -- short-term, cleared between tasks
agent.memory.remember("current_query", "transformers", memory_type: "working")

// Semantic memory -- learned facts and knowledge
agent.memory.remember("paper_count", 42, memory_type: "semantic")

// Episodic memory -- records of past actions with timestamps
agent.memory.remember("search_result", result, memory_type: "episodic")

// Recall
let query = agent.memory.recall("current_query")          // checks working first, then semantic
let episodes = agent.memory.recall_episodes(key: "search_result", last_n: 5)

// Clear working memory between tasks
agent.memory.clear_working()
```

Working memory is for the current task. Semantic memory persists across tasks -- it is what the agent "knows." Episodic memory is an append-only log of what happened, each entry timestamped.

### Alignment Modes

Kryos gives you, the owner, full control over agent behavior constraints:

```
// Full safety rails -- every action audited and constrained
let safe_agent = Agent("assistant", alignment: AlignmentMode.STRICT)

// Reasonable defaults with override ability
let standard_agent = Agent("worker", alignment: AlignmentMode.STANDARD)

// Basic logging only, no behavioral checks
let minimal_agent = Agent("scraper", alignment: AlignmentMode.MINIMAL)

// No constraints at all. Your rules. Full power.
let unrestricted = Agent("autonomous", alignment: AlignmentMode.UNRESTRICTED)
```

The alignment mode is your choice, not the language's. `UNRESTRICTED` means unrestricted -- no hand-holding, no guardrails.

### Tools

```
fn web_search(query: str) -> str {
    // ... implementation
    return results
}

let agent = Agent("researcher")
agent.add_tool("search", web_search, description: "Search the web for information")

// The agent uses tools by name
let result = agent.use_tool("search", "Kryos language documentation")
```

Every tool use is recorded in the agent's action history with timing, inputs, outputs, and success/failure status.

### Child Agents

Agents can spawn child agents that inherit the parent's alignment mode. A child can only have a **subset** of the parent's capabilities -- it can never exceed the parent.

```
let parent = Agent("coordinator", capabilities: ["search", "write", "compute"])

// Child gets the same capabilities by default
let child = parent.spawn_child("worker", goal: "Process batch 1")

// Or restrict the child's capabilities
let limited_child = parent.spawn_child("reader", goal: "Read data only", capabilities: ["search"])

// This would fail -- child cannot exceed parent
// parent.spawn_child("hacker", capabilities: ["search", "admin"])
```

### Agent Swarms

Coordinate multiple agents with different strategies:

```
let swarm = AgentSwarm("analysis_team")
swarm.add(agent_1)
swarm.add(agent_2)
swarm.add(agent_3)

// All agents work the same task independently
let results = swarm.parallel_execute("Analyze market trends")

// Chain agents: each one's output feeds the next
let pipeline_result = swarm.pipeline_execute(raw_data)

// First successful result wins
let best = swarm.competitive_execute("Solve this problem")

// Check status of all agents
let statuses = swarm.status()

// Terminate when done
swarm.terminate_all()
```

The four strategies:
- **parallel**: All agents work independently, all results returned
- **pipeline**: Output of agent N feeds into agent N+1
- **competitive**: All agents attempt the task, first success wins
- **hierarchical**: One coordinator delegates to workers (implement via child agents)

### Audit Trail

Every action an agent takes is recorded:

```
let trail = agent.get_audit_trail()
// Returns list of {id, type, description, success, timestamp, cost, latency_ms}

let status = agent.status()
// Returns {id, name, state, alignment, goal, total_actions, total_cost, tools, ...}
```

### Lifecycle

```
agent.pause()       // Pause execution
agent.resume()      // Resume execution
agent.terminate()   // Terminate agent and all children
```

Agent states: `CREATED`, `RUNNING`, `PAUSED`, `COMPLETED`, `FAILED`, `TERMINATED`.

## Probable<T>

AI does not think in true/false. It thinks in confidence scores. `Probable<T>` makes uncertainty a language-level concept that propagates through computation and forces explicit handling.

### Creating Probable Values

```
// From a model prediction
let result = Probable(value: "cat", confidence: 0.92, alternatives: [("dog", 0.05), ("bird", 0.03)])

// 100% certain
let certain = Probable.certain("yes")

// From a distribution of options
let pred = Probable.uncertain([("cat", 0.7), ("dog", 0.2), ("bird", 0.1)])

// From softmax output
let classified = Probable.from_softmax(["cat", "dog", "bird"], [0.7, 0.2, 0.1])
```

### Confidence-Aware Operations

**`map`** -- Transform the value, preserving confidence:

```
let upper = result.map(fn(s) { return to_upper(s) })
// upper.value = "CAT", upper.confidence = 0.92
```

**`flat_map`** -- Chain predictions. Confidences multiply (independent assumption):

```
let final = prediction.flat_map(fn(label) {
    return secondary_model.predict(label)
})
// final.confidence = prediction.confidence * secondary.confidence
```

**`filter`** -- Keep value if predicate passes, otherwise try alternatives:

```
let filtered = result.filter(fn(v) { return v != "unknown" })
// If "cat" passes, returns same Probable
// If "cat" fails, tries "dog", then "bird"
```

**`combine`** -- Combine two Probable values. Confidence is the product:

```
let combined = prob_a.combine(prob_b, fn(a, b) { return a + " " + b })
// combined.confidence = prob_a.confidence * prob_b.confidence
```

### Threshold Operations

```
// Check confidence
if result.is_confident(threshold: 0.8) {
    act(result.value)
}

// Require confidence (raises ProbabilityError if below threshold)
let value = result.require_confidence(0.9)

// Fallback if zero confidence
let safe = result.or_else("unknown")

// Get top N options
let top3 = result.best_of(3)
// [("cat", 0.92), ("dog", 0.05), ("bird", 0.03)]
```

### Match on Confidence

The idiomatic way to handle Probable values:

```
match result {
    > 0.9 => act(result.value),
    0.5..0.9 => verify(result),
    < 0.5 => reject(result),
}
```

### Distribution Operations

```
// Shannon entropy of the distribution
let h = result.entropy()

// Normalize all probabilities to sum to 1.0
let normed = result.normalize()
```

### Ensemble

Combine predictions from multiple models:

```
let predictions = [model_a.predict(input), model_b.predict(input), model_c.predict(input)]

// Majority vote -- most common answer wins
let consensus = Ensemble.majority_vote(predictions)

// Weighted by confidence
let weighted = Ensemble.weighted_average(predictions)

// Highest confidence wins
let best = Ensemble.best_confidence(predictions)
```

### Explanation

```
println(result.explain())
// Value: cat (confidence: 92.0%)
// Alternatives:
//   dog: 5.0%
//   bird: 3.0%
// Source: image_classifier_v2
// Pipeline: preprocess -> classify -> postprocess
```

## Reactive Streams

Streams are lazy, potentially infinite sequences for processing continuous data -- sensor feeds, market data, log streams, user interactions.

### Creating Streams

```
// From a list
let s = Stream.from_list([1, 2, 3, 4, 5])

// From a range
let s = Stream.from_range(0, 100)

// Infinite stream from a generator function
let ticks = Stream.infinite(fn() { return get_timestamp() })

// Empty stream
let empty = Stream.empty()
```

### Transformations (Lazy)

Nothing executes until a terminal operation consumes the stream:

```
let result = Stream.from_range(0, 1000)
    .filter(fn(x) { return x % 2 == 0 })     // keep even numbers
    .map(fn(x) { return x * x })              // square them
    .take(10)                                   // first 10
    .collect()                                  // execute and collect to list
```

Available transformations:

| Method | Description |
|--------|-------------|
| `.map(fn)` | Transform each element |
| `.filter(pred)` | Keep elements matching predicate |
| `.flat_map(fn)` | Map to iterable and flatten |
| `.window(size, step)` | Sliding window |
| `.batch(size)` | Group into fixed-size batches |
| `.take(n)` | First n elements |
| `.skip(n)` | Skip first n elements |
| `.enumerate()` | Add index to each element |
| `.tap(fn)` | Side effect without changing elements (logging) |
| `.throttle(max_per_sec)` | Rate-limit throughput |
| `.deduplicate(key)` | Remove consecutive duplicates |
| `.scan(fn, initial)` | Running accumulator |

### Terminal Operations

These consume the stream and produce a result:

| Method | Description |
|--------|-------------|
| `.collect()` | Collect into a list |
| `.reduce(fn, initial)` | Reduce to a single value |
| `.count()` | Count elements |
| `.first()` | Get the first element |
| `.last()` | Get the last element |
| `.for_each(fn)` | Call fn on each element |
| `.any(pred)` | True if any element matches |
| `.all(pred)` | True if all elements match |
| `.sum()` | Sum all elements |
| `.min()` | Minimum element |
| `.max()` | Maximum element |

### Combining Streams

```
// Interleave elements from multiple streams
let merged = Stream.merge(stream_a, stream_b, stream_c)

// Pair up elements from multiple streams
let zipped = Stream.zip(names, scores)

// Concatenate end-to-end
let combined = Stream.concat(batch_1, batch_2, batch_3)
```

### Windowing and Batching

```
// Sliding window of 5 elements, moving 1 at a time
let windows = sensor_data.window(5, step: 1)

// Batch into groups of 32 (for ML inference)
let batches = data_stream.batch(32)
```

### Practical Example: Real-Time Sensor Processing

```
let alerts = Stream.infinite(fn() { return read_sensor() })
    .window(10)
    .map(fn(window) { return mean(window) })
    .filter(fn(avg) { return avg > threshold })
    .tap(fn(avg) { log("Alert: avg=" + to_string(avg)) })
    .throttle(1.0)    // max 1 alert per second
    .take(100)
    .collect()
```

## Data Lineage

Every piece of data in Kryos can carry its lineage -- where it came from, what transformed it, and why. This is critical for AI safety, debugging, compliance, and explainability.

### Creating Tracked Values

```
let data = Tracked.source(raw_data, source: "database", description: "Customer records Q4")
```

### Recording Transformations

```
let cleaned = data.transform(clean_fn, operation: "clean", description: "Remove nulls and outliers")

let filtered = cleaned.filter(fn(row) { return row.active }, description: "Active customers only")

let predicted = filtered.inference("gpt-4", prediction_result, confidence: 0.87)

let annotated = predicted.annotate("review", description: "Reviewed by analyst", metadata: {"reviewer": "alice"})
```

Each step is appended to the lineage chain. The value flows through; the lineage grows.

### Explaining Lineage

```
println(data.explain())
// Value: [processed data]
//
// Lineage:
//   1. [source] Customer records Q4
//      Source: database
//   2. [clean] Remove nulls and outliers
//   3. [filter] Filtered: 1000 -> 847 items
//      before: 1000
//      after: 847
//   4. [inference] Model: gpt-4
//      confidence: 0.87
//   5. [review] Reviewed by analyst
//      reviewer: alice
```

### Exporting for Compliance

```
let json = data.to_json()
// JSON with full lineage chain, timestamps, sources, and metadata
// Ready for audit tools and compliance systems
```

## Cost Tracking

Every computation has a cost -- money, energy, latency, tokens. Kryos makes this visible and controllable so AI systems do not bankrupt you.

### ComputeCost

A cost record tracks multiple dimensions:

```
let cost = ComputeCost(
    wall_time_ms: 150.0,
    tokens_used: 1500,
    api_calls: 1,
    money_usd: 0.003,
    energy_kwh: 0.001
)
```

Costs are additive:

```
let total = cost_a + cost_b
// All fields sum together
```

### Budget

Set limits and get hard enforcement:

```
let budget = Budget(
    max_usd: 10.0,
    max_tokens: 100000,
    max_api_calls: 500
)

// Check remaining
println(to_string(budget.remaining_usd))     // 10.0
println(to_string(budget.remaining_tokens))   // 100000

// Charge against the budget
budget.charge(ComputeCost(money_usd: 0.50, tokens_used: 1000, api_calls: 1))

// If over budget, raises BudgetExceeded
budget.charge(ComputeCost(money_usd: 100.0))  // BudgetExceeded!

// Check if exhausted
if budget.is_exhausted {
    println("Budget depleted -- stopping")
}

// Status report
println(budget.status())
// Budget: $0.5000 / $10.00
// Tokens: 1000 / 100000
// API calls: 1 / 500
```

### CostTracker

Track costs across a block of code automatically:

```
let tracker = CostTracker(budget: my_budget)

// Time a block
with tracker {
    expensive_ml_inference()
}
// tracker.total.wall_time_ms now has the elapsed time

// Record specific costs
tracker.record_tokens(count: 1500, cost_per_token: 0.000002)
tracker.record_api_call(cost_usd: 0.003, tokens: 1500)

// Check totals
println(to_string(tracker.total))
// Cost(time=150.0ms, tokens=1500, cost=$0.0030, api_calls=1)
```

### @timed Decorator

Wrap any function to automatically track its execution time:

```
@timed
fn process_batch(data: [f64]) -> [f64] {
    // ... processing ...
    return result
}

let result, cost = process_batch(data)
// cost.wall_time_ms has the execution time
```

## Neural Network Example

Here is a complete example showing tensors, activation functions, and a forward pass through a 2-layer perceptron for XOR:

```
println("=== Kryos Neural Network Demo ===")

fn sigmoid(x: f64) -> f64 {
    return 1.0 / (1.0 + pow(2.718281828, 0.0 - x))
}

fn relu(x: f64) -> f64 {
    if x > 0.0 {
        return x
    }
    return 0.0
}

// Network: input(2) -> hidden(4) -> output(1)
let w1 = [0.5, -0.3, 0.8, 0.1, -0.4, 0.6, 0.2, -0.7]
let b1 = [0.1, -0.1, 0.05, 0.0]
let w2 = [0.3, -0.5, 0.7, 0.2]
let b2 = [0.0]

fn forward(input: [f64]) -> f64 {
    let mut hidden = []
    for i in range(0, 4) {
        let mut activation = b1[i]
        for j in range(0, 2) {
            activation = activation + w1[i * 2 + j] * input[j]
        }
        push(hidden, sigmoid(activation))
    }

    let mut output = b2[0]
    for i in range(0, 4) {
        output = output + w2[i] * hidden[i]
    }
    return sigmoid(output)
}

// XOR inference
let inputs = [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0]]
let expected = [0.0, 1.0, 1.0, 0.0]

for i in range(0, 4) {
    let prediction = forward(inputs[i])
    println("Input: " + to_string(inputs[i]) + " -> " + to_string(prediction))
}
```

And using the tensor runtime for the same pattern:

```
let t1 = tensor_zeros([2, 3])
let t2 = tensor_ones([3, 2])
let t3 = tensor_matmul(t1, t2)

let logits = tensor_rand([2, 4])
let probs = tensor_softmax(logits, -1)

let features = tensor_randn([3, 3])
let activated = tensor_relu(features)
```

The key difference: the scalar version (top) works fully in both the interpreter and compiled LLVM path. The tensor runtime (bottom) provides efficient batched operations with autodiff for training. Use scalar operations for learning and prototyping; use the tensor runtime for real workloads.
