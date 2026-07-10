# Kryos Examples

This directory contains example programs demonstrating various Kryos language features.

## Running Examples

All examples can be run from the `compiler/` directory with:

```bash
cargo run --release -j 4 -- run ../examples/<example_name>.kry
```

## Examples

### hello.kry
A simple program that demonstrates string variables and concatenation.

```bash
cargo run --release -j 4 -- run ../examples/hello.kry
```

Output: `Hello, Kryos!`

### calculator.kry
Demonstrates arithmetic operations, functions, string matching, and control flow. Implements a simple calculator supporting addition, subtraction, multiplication, division, modulo, and exponentiation.

```bash
cargo run --release -j 4 -- run ../examples/calculator.kry
```

### word_count.kry
Demonstrates structs, string operations, for loops, and the `len()` builtin. Shows how to format output and work with string data.

```bash
cargo run --release -j 4 -- run ../examples/word_count.kry
```

### fibonacci.kry
Demonstrates recursive functions. Computes the 10th Fibonacci number using a simple recursive approach.

```bash
cargo run --release -j 4 -- run ../examples/fibonacci.kry
```

Output: `fibonacci(10) = 55`

### grep.kry
Demonstrates arrays, string searching with `contains()`, and control flow. Searches for matching lines in a hardcoded list of strings.

```bash
cargo run --release -j 4 -- run ../examples/grep.kry
```

Output:
```
Lines containing "ap":
apple
grape
```

### shapes.kry
Demonstrates enums with payloads and pattern matching. Defines a Shape enum with Circle and Rectangle variants, and computes their areas.

```bash
cargo run --release -j 4 -- run ../examples/shapes.kry
```

Output:
```
Circle with radius 5.0: 78.54975
Rectangle 4.0 x 6.0: 24
```

### channels.kry
Demonstrates basic concurrency using `spawn()` to run code in separate execution contexts.

```bash
cargo run --release -j 4 -- run ../examples/channels.kry
```

Output:
```
Producer spawned
Consumer received: Hello from spawn
Consumer spawned
```

## Learning Path

1. Start with **hello.kry** to see basic variables and strings
2. Move to **calculator.kry** for functions and pattern matching
3. Try **word_count.kry** to learn about structs
4. Study **shapes.kry** for enums and advanced pattern matching
5. Check **fibonacci.kry** for recursion
6. Explore **grep.kry** for arrays and string operations
7. Finish with **channels.kry** for concurrency basics

For more detailed language documentation, see [GETTING_STARTED.md](../compiler/docs/GETTING_STARTED.md) and [language-reference.md](../docs/19-language-reference.md).
