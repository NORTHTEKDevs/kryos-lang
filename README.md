# Kryos

Kryos is a compiled systems language with a Rust-like ownership model, Zig-like compile-time evaluation, and a first-class AI runtime -- tensors with autodiff, autonomous agents, probability types, and reactive streams as language primitives. It compiles to LLVM IR for native binaries at C speed, or runs interpreted with self-healing error recovery. 22,000 lines of implementation, 156 tests, 190 built-in functions across 11 stdlib modules.

## Why Kryos

- **Ownership model with zero runtime cost.** Borrow checker runs at compile time. Use-after-move, double mutable borrow, mutation-while-borrowed -- caught before your code runs. No garbage collector, no reference counting, no runtime overhead.
- **Compile-time evaluation.** `comptime` blocks execute arbitrary Kryos code at compile time and embed the results as constants. Lookup tables, configuration, type-level computation -- zero cost at runtime.
- **AI-native runtime.** Tensors with reverse-mode autodiff, `Probable<T>` for confidence-aware computation, autonomous agents with persistent memory, reactive streams with windowing and backpressure. Not a library bolted on -- these are language primitives.
- **LLVM compilation.** Kryos AST compiles to LLVM IR text. Feed it to `llc` and `clang` for native binaries. No external bindings required -- the codegen emits plain IR.
- **Capability-based security.** Functions declare what they can access via `@capabilities`. No annotation means no access beyond pure computation. Enforced at compile time, auditable, deny-by-default.

## Quick Start

```bash
# Requirements: Python 3.10+
# Optional: numpy (for fast tensor operations)

# Install as a CLI tool
pip install -e .

# Run a program
kryos run examples/demo.kry

# Interactive REPL
kryos repl

# Compile to LLVM IR (requires LLVM toolchain for native binary)
kryos build examples/demo.kry --emit-ir

# Type check and capability audit
kryos check examples/demo.kry

# Run the test suite
kryos test tests/programs
```

Hello world:

```
println("Hello, Kryos!")
```

Run it:

```bash
kryos run hello.kry
```

Compile it:

```bash
kryos build hello.kry -o hello
./hello
```

## Language Features

### Variables and Types

Immutable by default. Explicit `mut` for mutability. Type annotations optional when the initializer makes the type obvious.

```
let x: i32 = 42
let name = "Kryos"
let mut counter = 0
counter = counter + 1

// Numeric types: i8, i16, i32, i64, i128, u8, u16, u32, u64, u128, f32, f64
// Other primitives: bool, str, char
// Domain types: Tensor, Vec, Map, Set, Option, Result, Secret, Qubit, Qureg
```

Integer literals support hex (`0xFF`), binary (`0b1010`), octal (`0o77`), and underscore separators (`1_000_000`). Float literals support scientific notation (`2.5e-3`).

### Functions

```
fn fibonacci(n: i32) -> i32 {
    if n <= 1 {
        return n
    }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

fn dot(a: Vector3, b: Vector3) -> f64 {
    return a.x * b.x + a.y * b.y + a.z * b.z
}
```

Functions are first-class values. Pass them as arguments, return them from other functions:

```
fn apply_twice(f: fn, x: i32) -> i32 {
    return f(f(x))
}

fn double(x: i32) -> i32 {
    return x * 2
}

println(apply_twice(double, 3))  // 12
```

### Control Flow

```
// if / elif / else
if temperature > 100 {
    println("boiling")
} elif temperature > 0 {
    println("liquid")
} else {
    println("frozen")
}

// while loops
let mut i = 0
while i < 10 {
    i = i + 1
}

// for loops with ranges
for i in range(0, 10) {
    println(i)
}

// for-in over arrays
for item in data {
    process(item)
}

// break and continue
for i in range(0, 100) {
    if i % 2 == 0 { continue }
    if i > 50 { break }
    println(i)
}
```

### Structs

```
struct Point {
    x: f64,
    y: f64
}

struct NeuralLayer {
    weights: Tensor,
    bias: Tensor,
    activation: str
}

let origin = Point { x: 0.0, y: 0.0 }
println(origin.x)
```

### Enums and Pattern Matching

Enums can carry associated data:

```
enum Color {
    Red,
    Green,
    Blue
}

enum Shape {
    Circle(f64),
    Rect(f64, f64)
}

let c = Color.Red
println(c == Color.Red)  // true
```

### Traits and Impl

```
trait Drawable {
    fn draw(self) -> void;
}

struct Circle {
    radius: f64
}

impl Circle {
    fn area(self: Circle) -> f64 {
        return 3.14159 * self.radius * self.radius
    }
}

let c = Circle { radius: 5.0 }
println(c.area())  // 78.53975
```

### Closures

Functions close over their defining scope:

```
fn make_adder(x: i32) -> fn {
    fn adder(y: i32) -> i32 {
        return x + y
    }
    return adder
}

let add5 = make_adder(5)
println(add5(10))  // 15
println(add5(20))  // 25
```

Lambda syntax for inline functions:

```
(x, y) => x + y
(x: i32) -> i32 => { return x * x }
```

### Generics

```
struct Point<T> {
    x: T,
    y: T
}

fn identity<T>(value: T) -> T {
    return value
}

trait Numeric {
    fn zero() -> Self;
}

fn sum<T: Numeric>(items: [T]) -> T {
    // ...
}
```

### Ownership and Borrowing

Kryos enforces memory safety at compile time with zero runtime cost. The ownership analyzer catches:

1. **Use-after-move** -- a value cannot be used after it has been moved to another binding.
2. **Single mutable borrow** -- at most one `&mut` reference at a time.
3. **No aliasing** -- no simultaneous `&T` and `&mut T` borrows.
4. **Mutation-while-borrowed** -- cannot assign to a variable while it is borrowed.
5. **Loop move detection** -- moving an outer variable inside a loop body is caught (second iteration would use-after-move).

Copy types (`i32`, `f64`, `bool`, `char`, and all numeric types) are exempt -- they are copied on assignment, not moved.

```
let data = [1, 2, 3]
let copy = data          // data is MOVED -- it is not a copy type
// println(data)         // ERROR: use of moved value 'data'

let x: i32 = 42
let y = x                // x is COPIED -- i32 is a Copy type
println(x)               // OK -- x is still valid
```

The borrow checker runs between parsing and codegen. It is conservative: it will reject valid programs rather than accept invalid ones.

### Compile-Time Evaluation (comptime)

`comptime` blocks execute at compile time. The result is embedded as a constant -- zero runtime cost.

```
let table = comptime {
    let mut result = []
    for i in range(0, 256) {
        push(result, i * i)
    }
    return result
}
// table is a constant array of 256 squared values -- computed once, at compile time
```

The comptime transformer walks the AST before codegen, replacing `comptime` blocks with their evaluated literal values. The interpreter runs in isolation for each block.

### Error Handling

Self-healing runtime catches and auto-corrects common errors:

- Division by zero -- returns 0 or a fallback
- Index out of bounds -- clamps to valid range
- Type mismatches -- coerces when safe (int to float, etc.)
- Null/none access -- substitutes fallback values

```
kryos heal-report program.kry
```

Produces a report of every self-healing action taken during execution, with location and fix description.

### String Interpolation

```
let name = "Kryos"
let version = 1
println("Welcome to {name} v{version}")
```

Supports nested expressions, escape sequences (`\n`, `\t`, `\u{1F600}`), and triple-quoted multiline strings (`""" ... """`).

### Operators

Arithmetic: `+ - * / % **`
Matrix multiply: `@`
Comparison: `== != < > <= >=`
Logical: `and or not`
Bitwise: `& | ^ ~ << >>`
Range: `..` (exclusive), `..=` (inclusive)
Pipe: `|>` (pipe operator)
Assignment: `= += -= *= /=`

### Attributes

Decorator-style annotations that control compiler behavior:

```
@capabilities("gpu", "network")
@compute(device="cuda")
@export
@differentiable
@zero_copy
@real_time
@no_std
@target("wasm")
@layout(packed)
```

### Actors and Concurrency

```
actor Counter {
    state count: i32 = 0;
    on increment(amount: i32) {
        count = count + amount
    }
    on get_count() -> i32 {
        return count
    }
}

spawn Counter { ... }
parallel for x in data { process(x) }
```

### Modules and Imports

```
use std::collections::HashMap
use std::io::{Read, Write}

mod math {
    fn square(x: i32) -> i32 { return x * x }
}

extern use "libcuda.so" as cuda
```

## Standard Library

11 modules, 114 functions. All available without imports.

| Module | Functions | Purpose |
|--------|-----------|---------|
| **Core** | 36 | Print, math, strings, arrays, type conversion, assertions |
| **std::collections** | 13 | map, filter, reduce, sort, reverse, zip, enumerate, find, any, all, flat_map, sum, count |
| **std::io** | 27 | File I/O, directories, paths, glob, environment variables, stdin, temp files |
| **std::net** | 15 | HTTP client, WebSocket client, TCP sockets, URL encoding |
| **std::term** | 17 | Terminal control, cursor, colors, raw mode, key reading, alt screen |
| **std::crypto** | 10 | SHA-256, SHA-512, MD5, HMAC, Base64, hex encoding, random bytes, UUID |
| **std::json** | 4 | Parse, stringify, get, has |
| **std::map** | 9 | Dictionary operations -- new, set, get, has, keys, values, remove, merge, from |
| **std::math** | 5 | round, log10, random, pi, e (extends core sin/cos/tan/log/pow/floor/ceil/sqrt/min/max/abs) |
| **std::string** | 10 | repeat, pad_left, pad_right, lines, to_int, to_float, index, count, to_upper, to_lower |
| **std::process** | 4 | exec, exec_capture, exec_timeout, sleep |

### Core Built-ins (always available)

**I/O:** `println`, `print`, `stdin_read`

**Math:** `abs`, `sqrt`, `sin`, `cos`, `tan`, `log`, `pow`, `floor`, `ceil`, `min`, `max`, `round`, `log10`, `random`, `pi`, `e`

**Strings:** `len`, `char_at`, `char_code`, `char_from`, `substr`, `contains`, `starts_with`, `ends_with`, `upper`, `lower`, `trim`, `split`, `join`, `replace`

**Arrays:** `push`, `pop`, `len`, `range`

**Type conversion:** `to_string`, `int`, `float`, `str`, `type_of`, `assert`

### std::io

```
// File operations
let content = file_read("data.txt")
file_write("output.txt", content)
file_append("log.txt", "entry\n")
let exists = file_exists("config.toml")
file_delete("temp.txt")
let lines = file_lines("data.csv")
file_copy("src.txt", "dst.txt")
file_move("old.txt", "new.txt")
let size = file_size("binary.dat")

// Directory operations
let entries = dir_list("./src")
dir_create("./build/output")
dir_remove("./tmp")
let files = glob("src/**/*.kry")

// Path utilities
let full = path_join("src", "main.kry")
let dir = path_dirname("/home/user/file.kry")
let base = path_basename("/home/user/file.kry")
let ext = path_extension("file.kry")
let resolved = path_resolve("../lib")

// Environment
let home = env_get("HOME")
env_set("KRYOS_DEBUG", "1")
let dir = cwd()
let tmp = temp_file()
let tmpdir = temp_dir()
```

### std::net

```
// HTTP
let body = http_get("https://api.example.com/data")
let response = http_post("https://api.example.com/submit", payload)
let data = http_get_json("https://api.example.com/users")
let result = http_post_json("https://api.example.com/create", json_data)
let custom = http_request("PUT", "https://api.example.com/item/1", body, headers)

// WebSocket
let ws = ws_connect("wss://stream.example.com")
ws_send(ws, "subscribe:prices")
let msg = ws_recv(ws)
ws_close(ws)

// TCP
let sock = tcp_connect("127.0.0.1", 8080)
tcp_send(sock, "GET / HTTP/1.1\r\n\r\n")
let data = tcp_recv(sock, 4096)
tcp_close(sock)

// URL
let encoded = url_encode("hello world")
let decoded = url_decode("hello%20world")
```

### std::crypto

```
let hash = sha256("message")
let hash512 = sha512("message")
let legacy = md5("message")
let mac = hmac_sha256("secret-key", "message")
let encoded = base64_encode("binary data")
let decoded = base64_decode(encoded)
let hex = hex_encode("bytes")
let raw = hex_decode(hex)
let bytes = random_bytes(32)
let id = uuid()
```

### std::term

```
term_clear()
term_write("status: OK")
term_move(5, 10)
term_hide_cursor()
term_show_cursor()
term_alt_screen()
term_main_screen()
let size = term_size()  // [columns, rows]
term_color("red")
term_reset()
term_bold("important")
term_dim("secondary")
term_underline("linked")
term_rgb(255, 128, 0)
term_raw_mode(true)
let key = term_read_key()
```

### std::collections

```
let doubled = map([1, 2, 3], (x) => x * 2)
let evens = filter([1, 2, 3, 4], (x) => x % 2 == 0)
let total = reduce([1, 2, 3], (acc, x) => acc + x, 0)
let sorted = sort([3, 1, 2])
let reversed = reverse([1, 2, 3])
let pairs = zip([1, 2], ["a", "b"])
let indexed = enumerate(["a", "b", "c"])
let found = find([1, 2, 3], (x) => x > 1)
let has_even = any([1, 3, 5], (x) => x % 2 == 0)
let all_pos = all([1, 2, 3], (x) => x > 0)
let flat = flat_map([[1, 2], [3]], (x) => x)
let total = sum([10, 20, 30])
let n = count([1, 2, 3])
```

## AI-Native Runtime

Kryos ships with a runtime designed for AI/ML workloads as language primitives, not library imports.

### Tensors and Autodiff

N-dimensional tensors with shape tracking, broadcasting, element-wise operations, reductions, linear algebra, and reverse-mode automatic differentiation. Uses numpy when available, falls back to pure Python.

```
// Create tensors
let zeros = tensor_zeros([3, 3])
let ones = tensor_ones([2, 4])
let random = tensor_rand([128, 64])
let normal = tensor_randn([32, 32])
let identity = tensor_eye(4)
let seq = tensor_arange(0, 10, 1)

// Operations
let product = tensor_matmul(a, b)
let activated = tensor_relu(hidden)
let probs = tensor_softmax(logits, -1)
let reshaped = tensor_reshape(t, [4, 8])
let concatenated = tensor_cat([t1, t2], 0)
let total = tensor_sum(t)
let average = tensor_mean(t)

// Reverse-mode autodiff
let w = GradTensor(tensor_rand([3, 3]))
// Forward pass, compute loss, call backward -- gradients propagate automatically
```

The tensor system supports:
- Shape inference and validation
- NumPy-style broadcasting
- Matmul, softmax, ReLU, reshape, concatenation
- Sum, mean, max, min reductions
- Random initialization (uniform, normal)
- Gradient tracking via `GradTensor`

### Agents

Autonomous entities with persistent memory (working, episodic, semantic), goal-directed behavior, tool use, and coordination protocols.

```
let agent = Agent("analyst", "Analyze market data")

// Memory persists across invocations
agent.remember("last_signal", "bullish", "semantic")
let signal = agent.recall("last_signal")

// Alignment modes: unrestricted, minimal, standard, strict
// YOU choose the guardrails. The owner controls the agent.
```

Agent features:
- Three memory types: working (short-term), episodic (action records), semantic (learned facts)
- Lifecycle states: created, running, paused, completed, failed, terminated
- Alignment modes from `@unrestricted` (no constraints) to `@alignment(strict)` (full audit)
- Multi-agent coordination via `AgentSwarm`

### Probability Types

`Probable<T>` makes uncertainty a first-class concept. Confidence scores propagate through computation automatically.

```
let prediction = Probable("cat", 0.92)

// Confidence-aware operations
let transformed = prediction.map(upper)     // confidence preserved
let chained = prediction.flat_map(classify) // confidences multiply
let safe = prediction.require_confidence(0.8)  // raises if below threshold
let fallback = prediction.or_else("unknown")

// Ensemble multiple predictions
let combined = ensemble_vote([model1_result, model2_result, model3_result])
let weighted = ensemble_weighted(predictions)

// Distribution analysis
let h = prediction.entropy()
let normalized = prediction.normalize()
let top3 = prediction.best_of(3)
```

Three ensemble strategies: `majority_vote`, `weighted_average`, `best_confidence`.

### Reactive Streams

Lazy, composable data streams for real-time processing. Support windowing, backpressure, parallel mapping, and merging.

```
let prices = Stream([100.5, 101.2, 99.8, 102.0])

let processed = prices
    .map((x) => x * 1.1)
    .filter((x) => x > 100.0)
    .take(10)
    .collect()

// Infinite streams
let sensor = Stream.infinite(read_sensor)
let windowed = sensor.window(60)  // 60-element sliding window

// Stream from range
let numbers = stream_range(0, 1000000)
```

### Data Lineage

Every value can carry its provenance -- where it came from, what transformed it, and why.

```
let raw = Tracked.source(load_data(), "sensors/temp.csv")
let cleaned = raw.transform(remove_nulls, "clean", "Remove null readings")
let normalized = cleaned.transform(normalize, "normalize", "Scale to [0,1]")

// Full audit trail
println(normalized.lineage)
```

### Cost Tracking

Every computation has a cost. Kryos makes it visible and enforceable.

```
let budget = Budget(max_usd=10.0, max_tokens=100000)
let tracker = CostTracker(budget)

// Track API calls, tokens, wall time, GPU seconds, energy
// Budget enforcement prevents runaway spending
```

`ComputeCost` tracks: wall time, CPU time, GPU seconds, tokens used, API calls, USD spent, energy in kWh.

## FFI -- Foreign Function Interface

### Python FFI (Community tier)

Import and call any Python package:

```
let np = py_import("numpy")
let arr = py_call(np, "array", [[1, 2], [3, 4]])
let det = py_call(np, "linalg.det", [arr])
let shape = py_attr(arr, "shape")
```

### C FFI (Pro tier)

Load shared libraries and call C functions with automatic type marshaling:

```
let lib = c_load("libm.so")
let result = c_call(lib, "sqrt", [144.0], ["f64"], "f64")
```

Supported types for marshaling: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `bool`, `str`, `ptr`, `void`, and fixed-size arrays (`[i32; 10]`).

## LLVM Compilation

Kryos compiles to LLVM IR, which can then be compiled to native machine code using the LLVM toolchain.

```bash
# Emit LLVM IR
kryos build program.kry --emit-ir

# Compile to native binary (requires llc + clang)
kryos build program.kry -o program
./program
```

The codegen supports:
- All integer and float types mapped to LLVM types (`i32` -> `i32`, `f64` -> `double`, `bool` -> `i1`, `str` -> `i8*`)
- Function declarations with parameters and return types
- Struct types with field access (GEP instructions)
- Enum types with discriminant tags
- String constants as global `[N x i8]` arrays
- Control flow: if/elif/else, while, for, break, continue
- Arithmetic, comparison, and logical operations
- Array allocation via `malloc`, element access via pointer arithmetic
- Printf/puts for output
- Lambda lifting (closures compiled to top-level functions)
- Impl methods compiled with name mangling (`Type_method`)
- Trait method resolution

The pipeline: source -> lexer -> parser -> ownership analysis -> comptime transform -> LLVM IR -> `llc` -> `clang` -> native binary.

## Capability-Based Security

Every function operates within declared capabilities. No declaration means no access beyond pure computation.

```
@capabilities(compute)
fn safe_math(x: i32) -> i32 {
    // Can only compute. No filesystem, no network, no FFI.
    return x * x + 1
}

@capabilities(compute, network)
fn fetch_data(url: str) -> str {
    return http_get(url)
}

@capabilities(compute, filesystem, network)
fn sync_files(remote: str, local: str) -> bool {
    let data = http_get(remote)
    file_write(local, data)
    return true
}
```

Capability categories:

| Capability | Tier | Description |
|-----------|------|-------------|
| `compute` | Community | Pure computation -- always available |
| `network` | Community | TCP/UDP/HTTP connections |
| `filesystem` | Community | File read/write |
| `filesystem:read` | Community | Read-only file access |
| `filesystem:write` | Community | Write file access |
| `memory` | Community | General memory access |
| `gpu` | Community | GPU compute |
| `ffi` | Community | Foreign function interface |
| `network:raw_socket` | Pro | Raw socket access |
| `memory:raw` | Pro | Raw/unsafe memory |
| `syscall` | Pro | Direct system calls |
| `quantum` | Enterprise | Quantum compute access |

The audit command exports a full report of every capability used by every function in a program:

```bash
kryos check program.kry
```

## Tooling

### CLI Commands (17)

| Command | Description |
|---------|-------------|
| `kryos run <file>` | Run a .kry program |
| `kryos build <file>` | Compile to LLVM IR / native binary |
| `kryos check <file>` | Type check + capability audit |
| `kryos repl` | Interactive REPL with syntax highlighting |
| `kryos test <dir>` | Run test suite (expects `// expect:` comments) |
| `kryos migrate <file>` | Convert Python/JS/Rust/C/Go/Java to Kryos |
| `kryos validate <file>` | AI-assist code validation |
| `kryos heal-report <file>` | Run with self-healing diagnostics |
| `kryos license` | Manage license tier |
| `kryos init [path]` | Create new project with `kryos.toml` |
| `kryos add <pkg>` | Add a dependency |
| `kryos remove <pkg>` | Remove a dependency |
| `kryos deps` | List project dependencies |
| `kryos install` | Install all dependencies |
| `kryos publish` | Publish to local package registry |
| `kryos lsp` | Start Language Server Protocol server |
| `kryos version` | Show version and license tier |

### LSP Server

Full Language Server Protocol implementation over JSON-RPC / stdin-stdout. Zero external dependencies.

Supported features:
- **Diagnostics** -- parse errors and type warnings published on open/change/save
- **Hover** -- type information and documentation for identifiers
- **Completion** -- keywords, built-in types, functions, and project symbols
- **Go to definition** -- jump to function, struct, and enum declarations
- **Document symbols** -- outline of all declarations in the current file

Launch via `kryos lsp` and configure your editor to use it for `.kry` files.

### Package Manager

Project configuration via `kryos.toml`:

```toml
[package]
name = "my-project"
version = "0.1.0"

[dependencies]
http-utils = "^1.2.0"
```

Commands: `kryos init`, `kryos add`, `kryos remove`, `kryos deps`, `kryos install`, `kryos publish`.

Semver version resolution. Local package registry at `~/.kryos/packages/`. Module resolution for `use` imports against installed packages.

### AI-Assist

The compiler includes a built-in code validator and migration engine:

- **Validation** -- verify generated code is correct before execution, with suggestions and auto-fixes
- **Migration** -- convert Python, JavaScript, Rust, C, Go, or Java code to idiomatic Kryos
- **Error explanation** -- plain-English explanations of what went wrong and why

```bash
kryos validate program.kry
kryos migrate legacy_code.py
```

## Performance

### Ownership Model

Memory safety enforced at compile time. No garbage collector, no reference counting, no runtime bookkeeping. The borrow checker is a static analysis pass that produces zero runtime code. When the ownership analysis passes, the generated binary has the same memory profile as hand-written C.

### Compile-Time Evaluation

`comptime` blocks are fully evaluated before codegen. The result is a constant literal embedded in the output. Lookup tables, configuration parsing, string processing -- computed once at compile time, accessed at zero cost forever.

### LLVM Backend

The codegen emits standard LLVM IR. The LLVM optimizer (`opt`) and code generator (`llc`) apply the same optimization passes used by Clang, Rust, and Swift: inlining, vectorization, dead code elimination, register allocation. The resulting binary runs at native speed.

### Self-Healing Runtime

When running interpreted (not compiled), the self-healing engine catches runtime errors and auto-corrects:

- Division by zero -> fallback value
- Index out of bounds -> clamped index
- Type mismatch -> safe coercion
- Null access -> substitution

Seven heal actions: retry, coerce, clamp, fallback, reconstruct, skip, substitute. Every action is logged with location, original error, and fix applied. The `heal-report` command produces a full diagnostic.

### Planned

- Cranelift JIT for fast development iteration
- GPU tensor acceleration (CUDA/ROCm)
- SIMD vectorization for tensor operations
- Formal verification for Enterprise tier

## Architecture

```
Source (.kry)
    |
    v
  Lexer (80+ token types, string interpolation, nested block comments)
    |
    v
  Parser (recursive descent + Pratt expression parsing, 60+ AST node types)
    |
    v
  Ownership Analyzer (borrow checker -- use-after-move, borrow conflicts, loop detection)
    |
    v
  Comptime Transformer (evaluates comptime blocks, replaces with literal constants)
    |
    v
  Type Checker (type resolution, inference, operator validation, capability tracking)
    |
    v
  +------------------+-------------------+
  |                  |                   |
  v                  v                   v
Interpreter      LLVM Codegen        LSP Server
(tree-walking,   (IR text ->         (diagnostics,
 self-healing,    llc -> clang ->     hover, completion,
 190 builtins)    native binary)      go-to-def, symbols)
```

Key implementation files:

| File | Lines | Purpose |
|------|-------|---------|
| `compiler/interpreter.py` | ~1200 | Tree-walking interpreter with self-healing |
| `compiler/parser.py` | ~1100 | Recursive descent + Pratt expression parser |
| `compiler/codegen.py` | ~1800 | LLVM IR code generation |
| `compiler/types.py` | ~900 | Type system with inference |
| `compiler/ownership.py` | ~920 | Borrow checker |
| `compiler/lexer.py` | ~600 | Tokenizer |
| `compiler/ast_nodes.py` | ~680 | 60+ AST node types |
| `compiler/capabilities.py` | ~500 | Security enforcement |
| `compiler/self_heal.py` | ~400 | Self-healing engine |
| `compiler/comptime.py` | ~216 | Compile-time evaluator |
| `compiler/ai_assist.py` | ~400 | Validator + migration engine |
| `compiler/packages.py` | ~500 | Package manager |
| `runtime/tensor.py` | ~800 | Tensor library + autodiff |
| `runtime/agents.py` | ~400 | Agent runtime |
| `runtime/probable.py` | ~314 | Probability type |
| `runtime/streams.py` | ~300 | Reactive streams |
| `cli.py` | ~500 | CLI entry point |
| `lsp/server.py` | ~500 | Language server |

Total: ~22,000 lines of Python. Bootstrap compiler targeting LLVM IR.

## Testing

156 tests across 14 `.kry` test programs and 8 Python test modules:

```bash
# Run .kry integration tests (14 programs with // expect: assertions)
kryos test tests/programs

# Run unit tests
python tests/test_capabilities.py     # 20 capability enforcement tests
python tests/test_licensing.py        # 45 licensing tier tests
python tests/test_ai_runtime.py       # 77 AI runtime tests (tensors, agents, streams, probable)
python tests/test_ownership.py        # Borrow checker tests
python tests/test_comptime.py         # Compile-time evaluation tests
python tests/test_codegen_extended.py # LLVM codegen tests
python tests/test_stdlib.py           # Standard library tests
python tests/test_stdlib_io.py        # File I/O tests
python tests/test_stdlib_net.py       # Networking tests
python tests/test_stdlib_crypto.py    # Crypto tests
python tests/test_stdlib_term.py      # Terminal control tests
python tests/test_stdlib_map.py       # Map/dict tests
python tests/test_stdlib_process.py   # Process execution tests
python tests/test_stdlib_string_ext.py # String extension tests
```

Test programs cover: basics, variables, functions, control flow, structs, strings, arrays, math, capabilities, closures, self-healing, advanced patterns, capability audit, enums.

## Examples

Three example programs are included:

- `examples/demo.kry` -- Language feature showcase: recursion, structs, higher-order functions, arrays, capabilities, string processing
- `examples/neural_net.kry` -- Two-layer perceptron with forward pass, XOR inference, and tensor runtime demo
- `examples/kryos_bootstrap.kry` -- A Kryos tokenizer written in Kryos itself, proving the language is expressive enough to build its own compiler

## License

Proprietary. Copyright FrostByte Digital. All rights reserved.

### License Tiers

| Tier | Price | Key Capabilities |
|------|-------|-----------------|
| **Community** | Free | Full language, CPU/WASM, basic GPU, Python FFI, self-healing |
| **Pro** | $499/mo | Optimizing compiler, GPU codegen, C FFI, autonomous agents, raw sockets |
| **Enterprise** | $50K-$500K/yr | Quantum, raw memory, syscall, formal verification, FIPS crypto |
| **Cloud** | Usage-based | Managed infrastructure, all Pro features, per-compile pricing |
