# std::tensor

N-dimensional tensor operations backed by native FFI. Tensor handles are raw `i64` identifiers managed by the Kryos runtime. The runtime owns tensor memory; do not attempt to dereference or free handles directly.

```kryos
use std::tensor
```

---

## Overview

Tensors are referenced by opaque `i64` handles. All creation, mutation, and computation functions accept and return these handles. The shape of a tensor is described by an array of `i64` dimension sizes.

```kryos
use std::tensor

// Create a 3x4 matrix of zeros
let t = tensor_zeros([3, 4])
```

---

## Creation

### tensor_zeros

`tensor_zeros(shape: [i64]) -> i64`

Allocate a tensor of the given shape filled with `0.0`.

---

### tensor_ones

`tensor_ones(shape: [i64]) -> i64`

Allocate a tensor of the given shape filled with `1.0`.

---

### tensor_rand

`tensor_rand(shape: [i64]) -> i64`

Allocate a tensor of the given shape filled with uniform random values in `[0.0, 1.0)`.

---

### tensor_randn

`tensor_randn(shape: [i64]) -> i64`

Allocate a tensor of the given shape filled with values drawn from a standard normal distribution (mean 0, standard deviation 1).

---

### tensor_eye

`tensor_eye(n: i64) -> i64`

Allocate an `n x n` identity matrix.

---

### tensor_arange

`tensor_arange(start: f64, end: f64, step: f64) -> i64`

Allocate a 1-D tensor containing evenly spaced values from `start` up to but not including `end`, advancing by `step`.

**Example:**
```kryos
use std::tensor

let zeros    = tensor_zeros([2, 3])
let identity = tensor_eye(4)
let range    = tensor_arange(0.0, 1.0, 0.1)   // 10 elements: [0.0, 0.1, ..., 0.9]
```

---

## Operations

### tensor_matmul

`tensor_matmul(a: i64, b: i64) -> i64`

Compute the matrix product of `a` and `b`. Both tensors must be 2-D and their inner dimensions must match.

**Example:**
```kryos
use std::tensor

let a = tensor_ones([2, 3])
let b = tensor_ones([3, 4])
let c = tensor_matmul(a, b)   // shape [2, 4], all values 3.0
```

---

### tensor_softmax

`tensor_softmax(t: i64, dim: i64) -> i64`

Apply the softmax function along dimension `dim`. The output sums to 1.0 along that dimension.

**Example:**
```kryos
use std::tensor

let logits  = tensor_rand([1, 10])   // raw scores for 10 classes
let probs   = tensor_softmax(logits, 1)
```

---

### tensor_relu

`tensor_relu(t: i64) -> i64`

Apply the ReLU activation element-wise: `max(0.0, x)` for each element.

**Example:**
```kryos
use std::tensor

let x = tensor_randn([32, 128])
let y = tensor_relu(x)   // negative values become 0.0
```

---

### tensor_sigmoid

`tensor_sigmoid(t: i64) -> i64`

Apply the sigmoid activation element-wise: `1.0 / (1.0 + exp(-x))`.

**Example:**
```kryos
use std::tensor

let logit = tensor_randn([8, 1])
let prob  = tensor_sigmoid(logit)   // all values in (0.0, 1.0)
```

---

## Complete Example

```kryos
use std::tensor

// Two-layer forward pass (no training)
let input      = tensor_rand([1, 784])    // 28x28 image flattened
let weights1   = tensor_randn([784, 128])
let weights2   = tensor_randn([128, 10])

// Hidden layer
let hidden = tensor_relu(tensor_matmul(input, weights1))   // shape [1, 128]

// Output layer
let logits = tensor_matmul(hidden, weights2)               // shape [1, 10]
let probs  = tensor_softmax(logits, 1)                     // shape [1, 10]

// probs holds the predicted class probabilities for 10 classes
```

---

## Notes

- Tensor handles become invalid if the runtime frees the backing allocation. Do not hold handles across GC boundaries in long-running programs without pinning.
- All operations are eager -- no lazy evaluation graph is built.
- GPU acceleration, gradient computation, and model serialization are provided by `std::ml` (see `docs/stdlib/ml.md`).
