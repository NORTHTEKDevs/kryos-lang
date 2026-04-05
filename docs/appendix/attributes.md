# Attributes Reference

> **Implementation Status:** The `@name(args)` annotation syntax is fully parsed and attached to functions, structs, enums, and actors. `@capabilities` is the only attribute with compile-time enforcement (via the `kryos-capabilities` crate). All other attributes listed below (`@compute`, `@export`, `@differentiable`, `@zero_copy`, `@real_time`, `@target`, `@layout`, `@no_std`) are **parsed but not enforced** -- they are reserved for future compiler passes.

Attributes in Kryos use the `@name` or `@name(args)` syntax and are placed directly before a declaration (function, struct, etc.). They provide metadata that affects compilation, runtime behavior, or capability gating.

## @capabilities

Declare required runtime capabilities for a function. The capability system gates access to sensitive operations (network, filesystem, GPU, etc.) based on the project's permission tier.

```kryos
@capabilities("network", "filesystem")
fn download_file(url: str, path: str) -> bool {
    let data = http_get(url)
    file_write(path, data)
    return true
}
```

Sub-capabilities use colon notation:

```kryos
@capabilities("network:raw_socket")
fn ping(host: str) -> i64 {
    // raw socket access
}
```

Available capabilities include: `network`, `filesystem`, `gpu`, `process`, `ffi:python`, `ffi:native`, `network:raw_socket`, and others depending on the tier.

---

## @compute

Specify the compute target for a function. Directs the compiler/runtime to execute the function on a specific device.

```kryos
@compute(device="cuda")
fn matrix_multiply(a: Tensor, b: Tensor) -> Tensor {
    return a @ b
}

@compute(device="tpu")
fn train_step(model: Tensor, data: Tensor) -> Tensor {
    // TPU-accelerated training
}
```

Supported `device` values: `"cpu"`, `"cuda"`, `"tpu"`, `"wasm"`.

---

## @export

Mark a function for public API export. Exported functions are included in the module's public interface and can be called from other modules or through FFI boundaries.

```kryos
@export
fn process_data(input: str) -> str {
    return transform(input)
}

// Internal helper -- not exported
fn transform(s: str) -> str {
    return upper(s)
}
```

---

## @differentiable

Enable automatic differentiation for a function. The compiler generates gradient computation code, enabling the function to participate in backpropagation during ML training.

```kryos
@differentiable
fn loss(predicted: Tensor, actual: Tensor) -> f64 {
    let diff = predicted - actual
    return tensor_mean(diff * diff)
}

@differentiable
fn forward(weights: Tensor, input: Tensor) -> Tensor {
    return tensor_softmax(weights @ input)
}
```

Functions marked `@differentiable` must only use operations that have defined gradients.

---

## @zero_copy

Enable zero-copy semantics for function parameters. The runtime passes data by reference without copying, avoiding allocation overhead for large data structures.

```kryos
@zero_copy
fn analyze(data: Tensor) -> f64 {
    return tensor_mean(data)
}
```

Use this when the function reads but does not mutate its input. Combining `@zero_copy` with mutation will produce undefined behavior.

---

## @real_time

Mark a function as real-time safe. The compiler enforces that no heap allocations, garbage collection pauses, or unbounded operations occur within the function body.

```kryos
@real_time
fn audio_callback(buffer: [f32], sample_rate: i64) -> [f32] {
    // No allocations allowed here
    for i in 0..len(buffer) {
        buffer[i] = buffer[i] * 0.5
    }
    return buffer
}
```

Violations (calling allocating functions, unbounded loops) are reported as compile-time warnings.

---

## @target

Restrict compilation to a specific target platform. The function is only compiled and included when building for the specified target.

```kryos
@target("wasm")
fn browser_alert(msg: str) {
    // WASM-only implementation
}

@target("linux")
fn use_epoll(fd: i64) -> i64 {
    // Linux-specific syscall
}
```

Supported targets: `"wasm"`, `"linux"`, `"macos"`, `"windows"`, `"cuda"`, `"metal"`.

---

## @layout

Specify the memory layout for a struct. Controls how fields are arranged in memory, important for FFI interop and performance-critical code.

```kryos
@layout("packed")
struct PackedHeader {
    magic: u32,
    version: u16,
    flags: u8,
}

@layout("aligned", align=16)
struct SimdVector {
    data: [f32],
}
```

Layout options: `"packed"` (no padding), `"aligned"` (with alignment), `"c"` (C-compatible layout).

---

## @no_std

Compile a function or module without the standard library. Useful for bare-metal, embedded, or kernel code where stdlib functions are unavailable.

```kryos
@no_std
fn kernel_entry() {
    // Only core language features available
    // No println, file_read, http_get, etc.
}
```

## Combining Attributes

Multiple attributes can be stacked on a single declaration:

```kryos
@capabilities("gpu")
@compute(device="cuda")
@differentiable
@zero_copy
fn train_batch(weights: Tensor, data: Tensor) -> Tensor {
    return tensor_softmax(weights @ data)
}
```

Attributes are parsed in order and applied to the immediately following declaration. Placing attributes without a following declaration is a parse error.
