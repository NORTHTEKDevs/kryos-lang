# Kryos

A systems programming language designed for clarity, safety, and performance.

Kryos compiles to native code via Cranelift, features an ownership system for memory safety without garbage collection, and provides channel-based concurrency out of the box.

## Quick Start

```bash
# Build the compiler (use --release, debug builds are memory-intensive)
cargo build --release -j 4

# Run a program
cargo run --release -- run examples/hello.kry

# Compile to a native binary
cargo run --release -- compile examples/fibonacci.kry -o fib
./fib

# Run tests in the current project
cargo run --release -- test
```

## Language Features

```kryos
// Variables and type inference
let name = "Kryos"
let mut counter = 0

// Functions with type annotations
fn factorial(n: i64) -> i64 {
    if n <= 1 { return 1 }
    return n * factorial(n - 1)
}

// Structs with methods
struct Point {
    x: f64,
    y: f64
}

impl Point {
    fn distance(self, other: Point) -> f64 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        sqrt(dx * dx + dy * dy)
    }
}

// Enums with payloads and pattern matching
enum Shape {
    Circle(f64),
    Rect(f64, f64),
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.14159 * r * r,
        Shape::Rect(w, h) => w * h,
    }
}

let c = Shape.Circle(5.0)
println(to_string(area(c)))

// Channel-based concurrency
fn main() {
    let ch = chan()
    spawn {
        send(ch, 42)
    }
    let result = recv(ch)
    println(to_string(result))
}
```

## Architecture

Kryos is a 21-crate Rust workspace:

| Crate | Purpose |
|-------|---------|
| `kryos-lexer` | Tokenizer with full Unicode support |
| `kryos-parser` | Recursive-descent parser producing AST |
| `kryos-ast` | AST data structures |
| `kryos-types` | Bidirectional type checker |
| `kryos-ownership` | Ownership and borrow analysis |
| `kryos-mir` | Mid-level IR with CFG basic blocks |
| `kryos-codegen-cranelift` | Native codegen via Cranelift |
| `kryos-codegen-llvm` | LLVM backend (experimental) |
| `kryos-rt` | Runtime library (builtins, allocator, GC) |
| `kryos-lsp` | Language Server Protocol implementation |
| `kryos-package` | Package manager (kryos.toml, dependency resolution) |
| `kryos-cli` | Command-line interface |
| `kryos-driver` | Compilation pipeline orchestrator |
| `kryos-test-runner` | Native test framework |
| `kryos-fmt` | Code formatter |
| `kryos-doc` | Documentation generator |
| `kryos-linker` | Platform-specific linking |
| `kryos-bindgen` | C FFI binding generator |
| `kryos-errors` | Diagnostic infrastructure |
| `kryos-capabilities` | Capability-based security |
| `kryos-stdlib-native` | Native standard library |

## Self-Hosting

Kryos is self-hosting: the compiler is rewritten in Kryos itself (18,700+ lines across 13 modules in `self-host/`). The self-hosted compiler can tokenize source files and is progressing toward full parsing and compilation.

```bash
# Run the self-hosted compiler
cargo run --release -- run self-host/main.kry -- ast examples/hello.kry
```

## Testing

```bash
# Run the full Rust test suite (810+ tests)
cargo test --release -j 4

# Run Kryos-native tests
cargo run --release -- test
```

## Examples

See [`examples/`](examples/) for 9 example programs covering:
- Hello world and string manipulation
- Fibonacci (recursive + iterative)
- Enum-based calculator with pattern matching
- Word counting with character iteration
- Grep-style text search
- Shape geometry with enums
- Channel-based concurrency
- Comprehensive feature proof
- Markdown-to-text converter

## Builtins

| Function | Signature | Description |
|----------|-----------|-------------|
| `println` | `(str) -> void` | Print line to stdout |
| `print` | `(str) -> void` | Print without newline |
| `len` | `(any) -> i64` | Length of string/array |
| `push` | `(arr, val) -> void` | Append to array |
| `pop` | `(arr) -> val` | Remove last element |
| `to_string` | `(any) -> str` | Convert to string |
| `parse_int` | `(str) -> i64` | Parse integer |
| `parse_float` | `(str) -> f64` | Parse float |
| `substr` | `(str, start, end) -> str` | Substring |
| `contains` | `(str, needle) -> bool` | String contains |
| `starts_with` | `(str, prefix) -> bool` | String prefix check |
| `ends_with` | `(str, suffix) -> bool` | String suffix check |
| `trim` | `(str) -> str` | Trim whitespace |
| `to_upper` | `(str) -> str` | Uppercase |
| `to_lower` | `(str) -> str` | Lowercase |
| `replace` | `(str, from, to) -> str` | String replace |
| `split` | `(str, delim) -> [str]` | Split string |
| `join` | `([str], sep) -> str` | Join array |
| `sqrt` | `(f64) -> f64` | Square root |
| `min` | `(a, b) -> i64` | Minimum |
| `max` | `(a, b) -> i64` | Maximum |
| `chan` | `() -> channel` | Create channel |
| `send` | `(chan, val) -> void` | Send on channel |
| `recv` | `(chan) -> val` | Receive from channel |
| `spawn` | `{ block }` | Spawn concurrent task |
| `file_read` | `(path) -> str` | Read file |
| `file_write` | `(path, content) -> void` | Write file |
| `args` | `() -> [str]` | CLI arguments |
| `exit` | `(code) -> !` | Exit process |
| `assert` | `(cond, msg) -> void` | Assert or panic |

## License

Proprietary. Copyright FrostByte Digital.
