# Kryos Programming Language

**A universal systems language with native AI, quantum, and security capabilities.**

Built by [FrostByte Digital](https://frostbytedigital.io). Proprietary — all rights reserved.

## Quick Start

```bash
# Requirements: Python 3.10+
# Optional: numpy (for fast tensor operations)

# Run a Kryos program
python kryos_cli.py run examples/demo.kry

# Interactive REPL
python kryos_cli.py repl

# Run test suite
python kryos_cli.py test tests/programs

# Check capabilities and security audit
python kryos_cli.py check examples/demo.kry

# Compile to LLVM IR
python kryos_cli.py build examples/demo.kry --emit-ir

# Migrate Python code to Kryos
python kryos_cli.py migrate tests/sample_python.py

# View license info
python kryos_cli.py license --tiers
```

## Install as CLI tool

```bash
pip install -e .
kryos run examples/demo.kry
```

## What Kryos Does

- **AI-native**: Tensors, autodiff, probability types, agents as language primitives
- **Self-healing**: Runtime auto-fixes errors (division by zero, type mismatches, index bounds)
- **Secure by default**: Capability-based security enforced at compile time
- **Universal**: Systems programming, AI/ML, networking, embedded, quantum — one language
- **Interoperable**: Real FFI to Python and C (working today), planned for Rust/Go/JS

## Project Structure

```
kryos/
├── compiler/           # Core language implementation
│   ├── lexer.py        # Tokenizer (80+ token types)
│   ├── parser.py       # Recursive descent + Pratt expression parser
│   ├── ast_nodes.py    # 60+ AST node types
│   ├── types.py        # Type system with inference
│   ├── interpreter.py  # Tree-walking interpreter with self-healing
│   ├── codegen.py      # LLVM IR code generation
│   ├── capabilities.py # Security: capability enforcement + audit
│   ├── licensing.py    # 4-tier licensing system
│   ├── self_heal.py    # Self-healing runtime engine
│   ├── ai_assist.py    # Code validator, migration engine, error explainer
│   └── packages.py     # Package manager (kryos.toml)
├── runtime/            # AI-native runtime libraries
│   ├── tensor.py       # Tensors + reverse-mode autodiff
│   ├── probable.py     # Probable<T> — first-class probability type
│   ├── agents.py       # Autonomous agent runtime
│   ├── streams.py      # Reactive data streams
│   ├── lineage.py      # Data provenance tracking
│   ├── cost.py         # Computation cost tracking + budgets
│   └── ffi.py          # Foreign function interface (Python + C)
├── lsp/                # IDE support
│   ├── server.py       # Language Server Protocol server
│   └── protocol.py     # JSON-RPC + LSP types
├── cli.py              # CLI entry point (17 commands)
├── cli_commands/       # Package management commands
├── tests/              # Test infrastructure
examples/
├── demo.kry            # Language feature showcase
├── neural_net.kry      # Neural network forward pass
└── kryos_bootstrap.kry # Self-hosting: Kryos lexer in Kryos
tests/
├── programs/           # 14 .kry test programs
├── test_capabilities.py # 20 security tests
├── test_licensing.py   # 45 licensing tests
└── test_ai_runtime.py  # 77 AI runtime tests
```

## Test Suite

```bash
# All tests (156 total)
python -c "from kryos.tests.test_runner import run_tests; run_tests('tests/programs')"
python tests/test_capabilities.py
python tests/test_licensing.py
python tests/test_ai_runtime.py
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `kryos run <file>` | Run a .kry program |
| `kryos build <file>` | Compile to LLVM IR / native |
| `kryos check <file>` | Type check + capability audit |
| `kryos repl` | Interactive REPL |
| `kryos test <dir>` | Run test suite |
| `kryos migrate <file>` | Convert Python/JS/Rust/C/Go/Java to Kryos |
| `kryos validate <file>` | Validate code (AI-assist) |
| `kryos heal-report <file>` | Run with self-healing report |
| `kryos license` | Manage license |
| `kryos init` | Create new project |
| `kryos add <pkg>` | Add dependency |
| `kryos remove <pkg>` | Remove dependency |
| `kryos deps` | List dependencies |
| `kryos install` | Install dependencies |
| `kryos publish` | Publish to local registry |
| `kryos lsp` | Start LSP server for IDE |
| `kryos version` | Show version + license tier |

## License Tiers

| Tier | Price | Key Capabilities |
|------|-------|-----------------|
| **Community** | Free | Full language, CPU/WASM, basic GPU, Python FFI, self-healing |
| **Pro** | $499/mo | Optimizing compiler, GPU codegen, C FFI, autonomous agents |
| **Enterprise** | $50K-$500K/yr | Quantum, raw memory, syscall, formal verification, FIPS crypto |
| **Cloud** | Usage-based | Managed infra, all Pro features, per-compile pricing |

## Status

- **17,000 lines** of implementation
- **156 tests** passing
- **5 commits** on master
- Bootstrap compiler in Python, LLVM IR codegen ready
- Next milestone: Build LLM with Kryos tensor runtime
