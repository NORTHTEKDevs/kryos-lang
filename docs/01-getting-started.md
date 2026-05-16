# Getting Started

This chapter gets you from zero to running Kryos programs. By the end you will have the toolchain installed, understand the CLI, and have written a real program -- not just hello world.

## Installation

Kryos is a native Rust compiler (21 crates). You need Rust 1.75+ and Cargo to build it.

### From source (recommended)

Clone the repo and build the compiler:

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release
```

The `kryos` binary is produced at `compiler/target/release/kryos`. Add it to your PATH or symlink it:

```bash
# Linux/macOS
sudo ln -s $(pwd)/target/release/kryos /usr/local/bin/kryos

# Windows (PowerShell, as admin)
Copy-Item target\release\kryos.exe C:\Windows\kryos.exe
```

### Verify the install

```bash
kryos version
```

You should see output like:

```
kryos 2.3.0
```

### Optional: LLVM toolchain

Debug builds use the Cranelift backend (fast compilation, no external dependencies). Release builds (`--release`) use LLVM for optimized native code. To use the LLVM backend, install `llc` and `clang` from the LLVM project.

Without LLVM installed, `kryos build` still works -- it uses Cranelift. You only need LLVM for `kryos build --release`.

## Hello World

Create a file called `hello.kry`:

```
fn main() {
    println("Hello, Kryos!")
}
```

Run it:

```bash
kryos run hello.kry
```

Output:

```
Hello, Kryos!
```

Every compiled program needs a `fn main()` as its entry point. `println` is a built-in -- always available, no `use` statement needed.

The file extension is `.kry`. The CLI warns (but still runs) if you use a different extension.

## A Real First Program

Hello world proves the toolchain works. Now write something with substance. This program computes Fibonacci numbers using recursion, models a 2D point with a struct, and prints a formatted table:

```
// fibonacci.kry

// Recursive Fibonacci
fn fibonacci(n: i32) -> i32 {
    if n <= 1 {
        return n
    }
    return fibonacci(n - 1) + fibonacci(n - 2)
}

// A struct with an impl block
struct Point {
    x: f64,
    y: f64
}

impl Point {
    fn distance_to(self: Point, other: Point) -> f64 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        return sqrt(dx * dx + dy * dy)
    }
}

fn main() {
    // Print a Fibonacci table
    println("n\tFib(n)")
    println("--\t------")
    for i in range(0, 10) {
        println(to_string(i) + "\t" + to_string(fibonacci(i)))
    }

    // Use the struct
    let a = Point { x: 0.0, y: 0.0 }
    let b = Point { x: 3.0, y: 4.0 }
    println("\nDistance from origin to (3, 4): " + to_string(a.distance_to(b)))
}
```

Run it:

```bash
kryos run fibonacci.kry
```

A few things to notice:

- **No semicolons.** Kryos uses newlines as statement terminators.
- **`if`/`elif`/`else`, never `else if`.** The keyword is `elif`.
- **`let` is immutable by default.** Use `let mut` when you need to reassign.
- **`to_string()` for concatenation.** Kryos does not auto-coerce types in string concatenation -- you convert explicitly.
- **`impl` blocks add methods.** The first parameter is `self: TypeName`.
- **`for i in range(0, 10)`** iterates from 0 to 9 (exclusive upper bound).
- **Built-ins like `sqrt`, `println`, `to_string` are always available.** No imports needed for core functions.

## CLI Commands

The `kryos` command has 10 subcommands. Here is what each one does and when you would use it.

### Running and building

| Command | What it does |
|---------|-------------|
| `kryos run <file.kry>` | Compile and execute a program in one step. Debug builds use Cranelift (fast), `--release` uses LLVM (optimized). |
| `kryos build <file.kry>` | Compile to a native binary. By default uses Cranelift AOT; pass `--release` for LLVM-optimized output. Use `-o <path>` to set the output path and `--target <triple>` for cross-compilation. |
| `kryos check <file.kry>` | Type-check, ownership analysis, and capability audit without producing a binary. Reports violations and prints a capability map of every function. |
| `kryos repl` | Start an interactive REPL session. Supports multi-line input (detects unclosed braces/parens). Type `exit` or Ctrl+D to quit. |
| `kryos test [dir]` | Run `.kry` test files in a directory. Tests use `// expect:` comments to assert expected output and `// expect-error:` for expected failures. Use `// skip` to skip a test. Defaults to `tests/` if no directory is given. |

### Build flags

| Flag | What it does |
|---------|-------------|
| `--release` | Use the LLVM backend for optimized native code (requires LLVM installed). |
| `--emit-mir` | Dump the MIR (mid-level intermediate representation) for inspection. |
| `--emit-llvm` | Dump the generated LLVM IR text. |
| `--verbose` | Print each compilation stage as it runs. |
| `-o <path>` | Set the output binary path. |
| `--target <triple>` | Cross-compile for a specific target (e.g., `x86_64-unknown-linux-gnu`). |

### Code formatting and tooling

| Command | What it does |
|---------|-------------|
| `kryos fmt [file.kry]` | Format `.kry` source files (rewrites in place). Pass `--check` for diff mode (exits non-zero if changes needed). |
| `kryos bindgen <header>` | Generate Kryos FFI bindings from C headers. |
| `kryos lsp` | Start the Language Server Protocol server (JSON-RPC over stdin/stdout). Configure your editor to use this for `.kry` files to get diagnostics, hover info, completions, go-to-definition, and document symbols. |
| `kryos version` | Print the Kryos version. |

### Package management

| Command | What it does |
|---------|-------------|
| `kryos pkg init` | Create a new project with a `kryos.toml` manifest. |
| `kryos pkg add <package>` | Add a dependency to `kryos.toml`. |
| `kryos pkg remove <package>` | Remove a dependency from `kryos.toml`. |
| `kryos pkg update` | Update all dependencies to their latest compatible versions. |
| `kryos pkg lock` | Regenerate the lockfile from `kryos.toml`. |

## The REPL

The REPL is useful for testing small ideas without creating a file:

```bash
kryos repl
```

```
kryos 2.3.0 REPL
Type 'exit' or Ctrl+D to quit.

kryos> let x = 42
kryos> println(x * 2)
84
kryos> fn square(n: i32) -> i32 {
  ...>     return n * n
  ...> }
kryos> println(square(x))
1764
```

The REPL detects unclosed braces, brackets, and parentheses and shows a continuation prompt (`...>`) until you close them. State persists across lines within a session -- variables and functions you define stay available.

## File Extension

Kryos source files use the `.kry` extension. The CLI warns if you pass a file with a different extension, but still runs it.

## Project Structure

When you run `kryos pkg init`, it creates a `kryos.toml` manifest:

```toml
[package]
name = "my-project"
version = "0.1.0"

[dependencies]
```

A typical Kryos project looks like this:

```
my-project/
    kryos.toml          # project manifest
    src/
        main.kry        # entry point (must define fn main())
        utils.kry       # utility module
    tests/
        test_math.kry         # test file with // expect: assertions
        test_strings.kry      # test file with // expect-error: assertions
    examples/
        demo.kry
```

Conventions:

- **`src/`** for source files. The entry point is `main.kry` and must define `fn main()`.
- **`tests/`** for test files. Each test file uses annotation comments that `kryos test` checks:
  - `// expect: <value>` -- assert that the program prints this line
  - `// expect-error: <message>` -- assert that compilation fails with this error
  - `// skip` -- skip this test file
- **`examples/`** for example programs.
- **`kryos.toml`** at the project root declares the package name, version, and dependencies.

Use `kryos pkg add <package>` to add dependencies and `kryos pkg update` to update them.

## Editor Support

Run the built-in LSP server for editor integration:

```bash
kryos lsp
```

The LSP server communicates over stdin/stdout using JSON-RPC and supports:

- **Diagnostics** -- parse errors and type warnings on open, change, and save
- **Hover** -- type information and documentation for identifiers
- **Completion** -- keywords, built-in types, functions, and project symbols
- **Go to definition** -- jump to function, struct, and enum declarations
- **Document symbols** -- outline of all declarations in the current file

Configure your editor to launch `kryos lsp` as the language server for `.kry` files. No external dependencies required.

## What is Next

Now that you have the toolchain running:

- [Variables and Types](02-variables-and-types.md) covers `let`/`let mut`, type annotations, and the full type system.
- [Functions](03-functions.md) covers `fn` declarations, closures, lambdas, and first-class functions.
- [Core Built-ins](stdlib-core.md) lists every function available without imports.
- [Compilation Pipeline](15-codegen.md) explains how the compiler transforms your code into native binaries.
