# Kryos Showcase Programs

Five flagship programs that demonstrate Kryos can build the things a real
systems / applications language is expected to build. Every program in
this directory compiles end-to-end and runs to completion under
`kryos run`.

| File                 | What it shows                                                              |
| -------------------- | -------------------------------------------------------------------------- |
| `cli_tool.kry`       | A grep-like CLI: argument parsing, file I/O, string scanning, exit codes.  |
| `parser.kry`         | A recursive-descent calculator parser with operator precedence + errors.   |
| `bytecode_vm.kry`    | A 13-opcode stack VM that interprets Kryos-emitted bytecode programs.      |
| `agent_runtime.kry`  | An LLM-style tool-use loop (deterministic planner; swap in a real model).  |
| `web_server.kry`     | A minimal HTTP/1.0 server using the TCP builtins (`tcp_listen`/`accept`).  |

---

## Running each program

### `cli_tool.kry` — `kgrep`

A POSIX-style grep clone with `-i` (case-insensitive) and `-n` (line
numbers).

```bash
kryos run examples/showcase/cli_tool.kry
kryos build examples/showcase/cli_tool.kry -o ./kgrep
./kgrep -n println examples/showcase/cli_tool.kry
```

Exit code is `0` if at least one line matched, `1` if none did, `2` on
usage error — matching `grep(1)` conventions.

### `parser.kry` — recursive-descent calculator

Parses and evaluates arithmetic expressions:

```
1 + 2 * (3 + 4) - 5         => 10
((1 + 2) * (3 + 4)) / 7     =>  3
-3 * (4 - 6)                =>  6
```

Demonstrates a complete lexer, the standard operator-precedence climb
(`expr → term → unary → primary`), and graceful error reporting that
includes the source column.

```bash
kryos run examples/showcase/parser.kry
```

### `bytecode_vm.kry` — stack VM

Defines a 13-opcode instruction set
(`PUSH POP DUP SWAP ADD SUB MUL LT EQ JMP JZ PRINT HALT`),
emits bytecode for three demo programs, and runs them on an
interpreter with a manual stack pointer:

```
sum 1..10    = 55
factorial(7) = 5040
fib(10)      = 55
```

Also prints a disassembly of `factorial(5)` for inspection.

```bash
kryos run examples/showcase/bytecode_vm.kry
```

### `agent_runtime.kry` — LLM tool-use loop

A complete agent harness: history-aware planner, tool registry
(`echo`, `calc`, `sysinfo`), and a step-bounded plan→call→observe
loop. The planner here is rule-based for reproducibility; swap in a
real model by rewriting `plan_next(...)`.

```bash
kryos run examples/showcase/agent_runtime.kry
```

Sample interaction:

```
user:      what is the square of 6
  [tool] calc(square, 6)
assistant: 36
```

### `web_server.kry` — HTTP/1.0 server

A minimal web server using only Kryos's TCP builtins. By default it
serves three requests on `127.0.0.1:8080` and exits, so it works in
CI; pass `forever` to keep it running.

Routes: `/` (HTML), `/hello` (text), `/json` (JSON), `/count`
(request counter), anything else → 404.

```bash
# In one terminal:
kryos run examples/showcase/web_server.kry

# In another:
curl http://127.0.0.1:8080/
curl http://127.0.0.1:8080/json
curl http://127.0.0.1:8080/count
```

---

## Notes

- All examples target Kryos 1.0.0 and use only stable language features
  documented in `docs/19-language-reference.md`.
- Where a builtin was missing or behaved differently than expected, the
  workaround is documented inline (see e.g. the `char_from(123)` use in
  `web_server.kry` for the literal `{` character, which would otherwise
  trigger string interpolation).
- The agent runtime intentionally uses parallel string arrays instead of
  a `struct Action { ... }` return type; the latter exposed a known
  ownership-leak bug tracked in `docs/BUGS.md`.
