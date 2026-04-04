# Getting Started

This chapter gets you from zero to running Kryos programs. By the end you will have the toolchain installed, understand the CLI, and have written a real program -- not just hello world.

## Installation

Kryos requires Python 3.10 or later. NumPy is optional but recommended for fast tensor operations.

### From source (recommended)

Clone the repo and install in editable mode so `kryos` is available as a CLI command:

```bash
git clone https://github.com/FrostbyteDevTeam/kryos-lang.git
cd kryos-lang
pip install -e .
```

This registers the `kryos` command globally. Editable mode means changes to the source take effect immediately -- useful if you are hacking on the language itself.

### Verify the install

```bash
kryos version
```

You should see output like:

```
Kryos v0.1.0
License: Community
```

### Optional: Rust VM

For faster execution, build the Rust-based VM runner:

```bash
cd rust
cargo build --release -p kryos-runner
```

When the `kryos-runner` binary is present, `kryos run` uses it automatically. Without it, Kryos falls back to the Python tree-walking interpreter. Both produce identical results -- the Rust path is just faster.

### Optional: LLVM toolchain

To compile Kryos programs to native binaries, install `llc` and `clang` from the LLVM project. Without them, `kryos build` still emits `.ll` files (LLVM IR text) -- you just cannot link them into executables.

## Hello World

Create a file called `hello.kry`:

```
println("Hello, Kryos!")
```

Run it:

```bash
kryos run hello.kry
```

Output:

```
Hello, Kryos!
```

That is the entire program. No `main` function, no imports, no boilerplate. `println` is a built-in -- always available, no `use` statement needed.

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

The `kryos` command has 17 subcommands. Here is what each one does and when you would use it.

### Running and building

| Command | What it does |
|---------|-------------|
| `kryos run <file.kry>` | Execute a program. Uses the Rust VM if available, otherwise the Python interpreter. Pass `--no-heal` to disable self-healing error recovery. |
| `kryos build <file.kry>` | Compile to LLVM IR (`.ll` file). If `llc` and `clang` are installed, also links a native binary. Pass `--emit-ir` to only emit IR without attempting native compilation. |
| `kryos check <file.kry>` | Type-check and run a capability audit. Reports violations (functions accessing capabilities they did not declare) and prints a capability map of every function. |
| `kryos repl` | Start an interactive REPL session. Supports multi-line input (detects unclosed braces/parens). Type `exit` or Ctrl+D to quit. |
| `kryos test [dir]` | Run `.kry` test files in a directory. Tests use `// expect:` comments to assert expected output. Defaults to `tests/programs` if no directory is given. |
| `kryos bundle <file.kry>` | Bundle a program into a self-contained deployment package with launcher scripts and a Dockerfile. |

### Code assistance

| Command | What it does |
|---------|-------------|
| `kryos validate <file.kry>` | AI-assisted code validation. Checks for correctness issues and suggests fixes. Pass `--fix` to auto-apply corrections. |
| `kryos migrate <file>` | Convert code from Python, JavaScript, Rust, C, Go, or Java into Kryos. Auto-detects the source language (override with `--lang`). Use `-o output.kry` to write the result to a file. |
| `kryos heal-report <file.kry>` | Run a program with self-healing enabled and print a diagnostic report of every auto-correction the runtime made. |

### Project management

| Command | What it does |
|---------|-------------|
| `kryos init [path]` | Create a new project with a `kryos.toml` manifest. Defaults to the current directory. |
| `kryos add <package>` | Add a dependency to `kryos.toml`. |
| `kryos remove <package>` | Remove a dependency from `kryos.toml`. |
| `kryos deps` | List all project dependencies. |
| `kryos install` | Install all dependencies declared in `kryos.toml`. |
| `kryos publish` | Publish the current package to the local registry (`~/.kryos/packages/`). |

### Tooling

| Command | What it does |
|---------|-------------|
| `kryos lsp` | Start the Language Server Protocol server (JSON-RPC over stdin/stdout). Configure your editor to use this for `.kry` files to get diagnostics, hover info, completions, go-to-definition, and document symbols. |
| `kryos license` | Show license status. Use `--activate <key>` to activate a license, `--deactivate` to remove it, or `--tiers` to see what each tier includes. |
| `kryos version` | Print the Kryos version and current license tier. |

## The REPL

The REPL is useful for testing small ideas without creating a file:

```bash
kryos repl
```

```
Kryos v0.1.0 REPL
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

When you run `kryos init`, it creates a `kryos.toml` manifest:

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
        main.kry        # entry point
        utils.kry       # utility module
    tests/
        programs/
            test_math.kry     # test file with // expect: assertions
            test_strings.kry
    examples/
        demo.kry
```

Conventions:

- **`src/`** for source files. The entry point is typically `main.kry`.
- **`tests/programs/`** for test files. Each test file uses `// expect: <value>` comments that `kryos test` checks against actual output.
- **`examples/`** for example programs.
- **`kryos.toml`** at the project root declares the package name, version, and dependencies.

Dependencies are installed to `~/.kryos/packages/` and resolved via semver matching. Use `kryos add <package>` to declare them and `kryos install` to fetch them.

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
- The three example programs in the `examples/` directory show real patterns: `demo.kry` (language features), `neural_net.kry` (AI runtime), and `kryos_bootstrap.kry` (a tokenizer written in Kryos).
