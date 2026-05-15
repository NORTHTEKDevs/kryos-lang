# Universal-language stress test

These showcases were written to prove that Kryos can write **anything** — not
just web servers and agents, but arbitrary classes of programs: parsers,
interpreters, numerical code, network clients, text processing, simulations.

Every program here is pure Kryos, runnable today with `kryos run <file>.kry`.

| File | What it proves |
|---|---|
| `calc.kry` | Recursive-descent expression parsing with proper precedence and mutual recursion between functions. |
| `csv.kry` | Real data wrangling: file I/O, string splitting, integer parsing, group-by aggregation. |
| `brainfuck.kry` | You can write a complete language interpreter in 60 lines of Kryos. Runs "Hello World!" |
| `life.kry` | 2D grid algorithms — Conway's Game of Life with glider and blinker on a 20×20 grid. |
| `api_client.kry` | Outbound HTTPS: fetches a real public API, parses the JSON response, walks the tree. |
| `regression.kry` | Numerical computing: linear regression by gradient descent. Learns y=3x+7 from noisy samples. |
| `template.kry` | Mustache-style `{{var}}` templating engine for text generation. |
| `regex.kry` | Tiny regex engine: literals, `.`, `*`, `^`, `$`. Every test case correct. |

Run any of them with:

```bash
kryos run examples/showcase/extra/calc.kry
```

These join the core showcases in `examples/showcase/` — REST API, MCP server,
LLM agent, static site generator, persistent KV store, parallel worker pool,
markdown converter, and the kdoc documentation extractor — to demonstrate
that Kryos genuinely deserves the "universal language" claim.
