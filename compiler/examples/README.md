# Kryos Example Programs

Example programs demonstrating Kryos language features. Each can be run with:

```
cargo run --release -- run examples/<name>.kry
```

## Examples

| File | Description |
|------|-------------|
| `hello.kry` | Hello world with string variables, concatenation, and `len()` |
| `fibonacci.kry` | Recursive and iterative Fibonacci, computes fib(25) both ways and verifies they match |
| `calculator.kry` | Enum-based calculator (Add/Sub/Mul/Div) with pattern matching, chained operations |
| `word_count.kry` | Count words in text by iterating characters with `substr()` and counting spaces |
| `grep.kry` | Search for a substring pattern across lines of text using a `contains_word` function |
| `shapes.kry` | Shape enum (Circle, Rectangle, Triangle) with area, perimeter, and size classification |
| `channels.kry` | Spawn 3 concurrent workers that send results on a channel, main collects and sums |
| `proof.kry` | Comprehensive proof program exercising 17 language features end-to-end |
| `markdown.kry` | Markdown-to-text converter demonstrating string manipulation, helpers, loops |
| `http_api.kry` | In-memory task-list REST API -- GET/POST routing, JSON responses, HTTP status codes |
| `mcp_server.kry` | Model Context Protocol server over stdio -- JSON-RPC 2.0, tool dispatch, MCP handshake |
| `ai_agent.kry` | Research agent using `std::agent` + `std::http` -- tool composition, Anthropic Messages API |

## Language Features Covered

- Variables: `let`, `let mut`, assignment
- Types: `i64`, `f64`, `str`, `bool`
- Control flow: `if`/`elif`/`else`, `while`, `for..in`, `break`, `continue`
- Functions: parameters, return types, recursion, higher-order functions
- Data structures: arrays, structs, enums with payloads
- Pattern matching: `match` with enum variant destructuring
- String operations: concatenation (`+`), `len()`, `substr()`, `to_string()`
- Math: `sqrt()`, arithmetic operators
- Concurrency: `spawn`, `chan()`, `send()`, `recv()`
- HTTP: `std::http` server with routing, request parsing, JSON responses
- Agent framework: `std::agent` with tool registration, composition, and LLM integration
- MCP: JSON-RPC 2.0 protocol server over stdio for MCP client compatibility
- JSON: `std::json` parse/stringify with full RFC 8259 escape handling
