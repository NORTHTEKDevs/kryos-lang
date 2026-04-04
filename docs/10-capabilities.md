# Capabilities

Capabilities are Kryos's security model. Every function declares exactly what system resources it needs -- filesystem access, network connections, GPU compute, FFI calls. If a function tries to use something it did not declare, the program fails at compile time. Not at runtime, not with a warning -- it does not compile.

This is the opposite of how most languages work. In Python or JavaScript, any function can open a file, make a network request, or call `eval`. You only find out about unauthorized access when something goes wrong in production. Kryos inverts that: you see every capability a program uses before you run it.

## The Core Principle: Deny by Default

A function with no `@capabilities` annotation has access to pure computation and nothing else. It can do math, manipulate strings, build data structures, call other pure functions. It cannot touch the filesystem, the network, the GPU, or anything outside the program's memory.

```
// This function has no capabilities -- pure computation only
fn square(x: i32) -> i32 {
    return x * x
}
```

You can call `println` without any capabilities (it is classified as pure output). But `file_read`, `http_get`, `exec` -- those all require explicit capability declarations.

## Declaring Capabilities

Use the `@capabilities(...)` attribute on a function:

```
@capabilities(compute)
fn process(x: i32) -> i32 {
    return x * 2
}
```

The `compute` capability is always implicitly available, so `@capabilities(compute)` is technically redundant. But it serves as documentation: this function is intentionally pure.

For real I/O, declare what you need:

```
@capabilities(network)
fn fetch_data(url: str) -> str {
    return http_get(url)
}

@capabilities(filesystem)
fn read_config(path: str) -> str {
    return file_read(path)
}
```

### Combining Capabilities

A function can declare multiple capabilities:

```
@capabilities(network, filesystem)
fn download_to_file(url: str, path: str) {
    let data = http_get(url)
    file_write(path, data)
}
```

## All Capability Types

Kryos organizes capabilities into a tree. Parent capabilities grant access to all their children.

### compute

Pure computation. Always available. Math, string manipulation, data structure operations, control flow.

No declaration needed, but `@capabilities(compute)` is valid for documentation.

### network

Network access -- TCP, UDP, HTTP connections.

```
@capabilities(network)
fn connect() -> str {
    return http_get("https://api.example.com/status")
}
```

Sub-capabilities:
- `network:http` -- HTTP client and server operations
- `network:raw_socket` -- Raw socket access (requires Pro license)

Declaring `network` grants both `network:http` and `network:raw_socket` (if your license allows it). Declaring `network:http` only grants HTTP, not raw sockets. Use the narrowest capability that fits.

### filesystem

File system read and write access.

```
@capabilities(filesystem)
fn save(path: str, data: str) {
    file_write(path, data)
}
```

Sub-capabilities:
- `filesystem:read` -- Read-only file access
- `filesystem:write` -- Write file access

If your function only reads files, declare `filesystem:read` instead of the full `filesystem`. This is the principle of least privilege -- ask for only what you need.

```
@capabilities(filesystem:read)
fn load_config(path: str) -> str {
    return file_read(path)
}
```

### gpu

GPU compute access for parallel computation.

```
@capabilities(gpu)
fn train_model(data: [f64]) {
    gpu_dispatch(data)
}
```

Sub-capabilities:
- `gpu:compute` -- Basic GPU compute dispatch
- `gpu:optimize` -- Optimizing GPU codegen with kernel fusion and auto-tiling (requires Pro license)

### memory

Memory management operations.

Sub-capabilities:
- `memory:raw` -- Raw/unsafe memory access including pointer arithmetic (requires Enterprise license)

### ffi

Foreign function interface -- calling code written in other languages.

```
@capabilities(ffi)
fn call_python() {
    ffi_call_python("numpy", "array", [1, 2, 3])
}
```

Sub-capabilities:
- `ffi:python` -- Call Python functions (Community license)
- `ffi:native` -- Call C/C++ functions via native FFI (requires Pro license)

### quantum

Quantum computing primitives. Requires Enterprise license.

Sub-capabilities:
- `quantum:simulate` -- Quantum circuit simulation
- `quantum:hardware` -- Target real quantum processors (IBM Q, IonQ, Azure Quantum)

### syscall

Direct system calls. Requires Enterprise license.

Sub-capabilities:
- `syscall:direct` -- Direct syscall execution

### crypto

Cryptographic primitives.

Sub-capabilities:
- `crypto:fips` -- FIPS-certified cryptography (requires Enterprise license)

### agent

Agent runtime primitives. Requires Pro license.

Sub-capabilities:
- `agent:autonomous` -- Autonomous agent execution

### self_modify

Self-modifying code. Requires Enterprise license. Sandboxed and fully audited.

### formal_verify

Formal verification passes. Enterprise license.

### real_time

Real-time deadline enforcement. Enterprise license.

## The Capability Hierarchy

Capabilities narrow downward -- they never elevate. If a function has `@capabilities(filesystem:read)`, any function it calls can have `filesystem:read` or less. It cannot call a function that requires `filesystem:write`.

```
@capabilities(filesystem:read)
fn safe_load(path: str) -> str {
    return file_read(path)
}

@capabilities(filesystem)
fn unsafe_save(path: str, data: str) {
    file_write(path, data)
}

@capabilities(filesystem:read)
fn process_config(path: str) {
    let data = safe_load(path)        // OK -- same capability
    // unsafe_save(path, data)        // COMPILE ERROR -- would elevate
}
```

This is fundamental to the security model. A sandboxed function cannot grant itself more power by calling another function. The capability boundary is enforced transitively through the entire call chain.

## Compile-Time Enforcement

The `CapabilityAnalyzer` runs as a static analysis pass before the program executes. It walks every function declaration and every function call, building a complete map of what capabilities are declared and what capabilities are actually used.

Violations are **errors**, not warnings:

```
fn bad_function() {
    file_read("secret.txt")    // COMPILE ERROR: 'file_read()' requires
                               // capability 'filesystem' but 'bad_function'
                               // only has ['compute']
}
```

The error message tells you exactly what is missing:

```
CAPABILITY VIOLATION in 'bad_function': requires 'filesystem'
but only has ['compute']
```

## Runtime Enforcement

Even after passing compile-time checks, Kryos has a second layer: the `CapabilityEnforcer` runs at execution time. This is defense in depth. If the static analyzer has a bug, the runtime enforcer catches it.

The runtime enforcer maintains a scope stack. When execution enters a function, it pushes the function's capabilities. When execution leaves, it pops. Every capability-requiring operation checks the current scope.

If a runtime violation occurs (which should never happen if the analyzer is correct), it raises a `CapabilityViolation` exception that **cannot be caught** by user code. It always propagates to the top level. You cannot `try/catch` your way past a capability violation.

Attempting to elevate capabilities at runtime raises a `SandboxEscapeAttempt` -- also uncatchable, always fatal.

## Auditing

Every capability check -- allowed or blocked -- is logged to an append-only audit log. This log cannot be modified or cleared by user code.

```
@capabilities(network)
fn api_call() -> str {
    return http_get("https://api.example.com")
}
```

After the program runs, the audit log contains entries like:

```json
{
    "timestamp": 1711929600.0,
    "function": "api_call",
    "capability": "network",
    "declared": ["compute", "network"],
    "action": "allowed"
}
```

Blocked attempts are also logged:

```json
{
    "action": "blocked",
    "function": "sneaky_function",
    "capability": "filesystem",
    "declared": ["compute"],
    "details": ""
}
```

And escalation attempts are recorded as critical events:

```json
{
    "action": "sandbox_escape_attempt",
    "details": "'child_fn' tried to elevate capability 'network' beyond parent 'sandbox'"
}
```

The audit log supports:
- `summary()` -- human-readable summary with counts of allowed, blocked, and escape attempts
- `to_json()` -- full JSON export for ingestion by external security tooling
- `freeze()` -- lock the log so no more entries can be added (useful for creating snapshots)

## Sandboxing

The `CapabilityEnforcer` can create child sandboxes. A sandbox starts with **zero capabilities** unless the parent explicitly grants a subset of its own.

```
// Parent has filesystem + network
// Create a sandbox with only filesystem:read
@capabilities(filesystem, network)
fn parent_fn() {
    // Sandbox code can only read files -- no network, no writes
}
```

Sandboxes enforce strict isolation:
- A sandbox cannot acquire capabilities the parent does not have
- A sandbox cannot modify the parent's capability set
- Attempting to create a sandbox with elevated capabilities raises `SandboxEscapeAttempt`

This is how Kryos runs untrusted code safely. FFI calls, dynamic code loading, plugin systems -- all run inside sandboxes with explicitly granted capabilities.

## License Tiers

Some capabilities require a paid license. The tier system:

| Capability | Community (free) | Pro | Enterprise |
|------------|:-:|:-:|:-:|
| compute | yes | yes | yes |
| network | yes | yes | yes |
| network:raw_socket | -- | yes | yes |
| filesystem | yes | yes | yes |
| gpu | yes | yes | yes |
| gpu:optimize | -- | yes | yes |
| ffi:python | yes | yes | yes |
| ffi:native | -- | yes | yes |
| agent | -- | yes | yes |
| memory:raw | -- | -- | yes |
| quantum | -- | -- | yes |
| syscall | -- | -- | yes |
| self_modify | -- | -- | yes |
| formal_verify | -- | -- | yes |
| real_time | -- | -- | yes |
| crypto:fips | -- | -- | yes |

Using a capability above your license tier is a compile error. The error tells you which tier is required.

## Built-in Function Requirements

Every built-in function has a defined set of required capabilities. Here are the key ones:

**No capabilities needed (always available):**
`println`, `print`, `len`, `range`, `to_string`, `abs`, `type_of`, `int`, `float`, `str`, `sqrt`, `sin`, `cos`, `tan`, `log`, `pow`, `floor`, `ceil`, `min`, `max`, `assert`, `push`, `pop`

**Filesystem:**
`file_read` (filesystem, filesystem:read), `file_write` (filesystem, filesystem:write), `file_delete` (filesystem, filesystem:write), `input` (filesystem)

**Network:**
`http_get` (network, network:http), `http_post` (network, network:http), `tcp_connect` (network), `tcp_listen` (network), `raw_socket` (network, network:raw_socket)

**GPU:**
`gpu_dispatch` (gpu, gpu:compute), `gpu_kernel` (gpu, gpu:compute), `gpu_optimize` (gpu, gpu:optimize)

**Syscall:**
`exec` (syscall, syscall:direct), `spawn_process` (syscall)

**Memory:**
`alloc` (memory), `dealloc` (memory), `raw_ptr` (memory, memory:raw)

**FFI:**
`ffi_call_python` (ffi, ffi:python), `ffi_call_c` (ffi, ffi:native)

## Real-World Patterns

### Web server with minimal privileges

```
@capabilities(network, filesystem:read)
fn serve(port: i32) {
    let config = file_read("config.toml")
    tcp_listen(port)
    // Can read files for config, can accept connections
    // Cannot write files, cannot call system commands
}
```

### Data pipeline with read-only input

```
@capabilities(filesystem:read)
fn load_data(path: str) -> [str] {
    return file_read(path).split("\n")
}

@capabilities(filesystem:write)
fn save_results(path: str, results: [str]) {
    file_write(path, join("\n", results))
}

@capabilities(filesystem)
fn pipeline(input: str, output: str) {
    let data = load_data(input)
    // ... process data ...
    save_results(output, data)
}
```

The `load_data` function cannot accidentally write to disk. The `save_results` function cannot read arbitrary files. Each function has exactly the access it needs.

### Plugin sandbox

```
@capabilities(compute)
fn run_plugin(code: str) {
    // Plugin runs in a pure compute sandbox
    // No filesystem, no network, no FFI
    // Even if the plugin code tries to call file_read(),
    // it is blocked at compile time
}
```

## Why This Matters

Most security vulnerabilities come from code doing something it was never intended to do. A logging library that makes network calls. A template engine that reads arbitrary files. A math utility that spawns processes.

Capabilities make these violations impossible. When you look at a function's `@capabilities` declaration, you know exactly what it can do. When you audit a program, you get a complete map of every capability used by every function.

This is not just defense -- it is documentation. Reading `@capabilities(filesystem:read)` tells you more about a function's behavior than reading its entire implementation.
