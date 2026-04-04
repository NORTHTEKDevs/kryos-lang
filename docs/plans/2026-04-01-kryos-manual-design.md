# Kryos Language Manual — Design Document

**Date:** 2026-04-01
**Status:** Approved

## Goal

Create a comprehensive developer manual for the Kryos programming language. The manual serves as the primary reference for developers using Kryos — covering syntax, semantics, stdlib, and advanced features.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Audience | Developers using Kryos | Not beginners or marketing — practical reference for active users |
| Location | `kryos-lang/docs/` directory | Single source of truth, versioned with the language |
| Stdlib depth | Full API reference | Every function documented with signature, description, examples, edge cases |
| Scope | Language only | IDE gets its own manual in kryos-code repo |
| Tone | Practical guide | Conversational but focused, explains "why" not just "what", code-heavy |

## Structure

```
docs/
├── README.md                    — Manual index / table of contents
├── 01-getting-started.md        — Install, hello world, first program
├── 02-variables-and-types.md    — let/let mut, type system, inference, literals
├── 03-functions.md              — fn, parameters, returns, closures, higher-order, pipe
├── 04-control-flow.md           — if/elif/else, for, while, break/continue, match
├── 05-structs-and-enums.md      — struct, enum, impl blocks, methods, pattern matching
├── 06-ownership.md              — Move semantics, borrowing, mut rules, why no GC
├── 07-error-handling.md         — try/catch/throw, self-healing runtime, recovery
├── 08-traits-and-generics.md    — trait, impl Trait for Type, generic functions, bounds
├── 09-concurrency.md            — spawn, actors, message passing, sleep
├── 10-capabilities.md           — @capabilities, sandboxing, security model, auditing
├── 11-comptime.md               — comptime blocks, compile-time evaluation, use cases
├── 12-modules-and-packages.md   — use, module system, package manager
├── 13-ffi.md                    — Python FFI, C FFI, interop patterns
├── 14-ai-runtime.md             — Tensors, autodiff, agents, probability types, streams
├── 15-codegen.md                — LLVM compilation, building native binaries
├── stdlib/
│   ├── README.md                — Stdlib overview, module index
│   ├── core-builtins.md         — println, len, push, range, to_string, math, strings
│   ├── io.md                    — std::io
│   ├── net.md                   — std::net
│   ├── crypto.md                — std::crypto
│   ├── term.md                  — std::term
│   ├── server.md                — std::server
│   ├── collections.md           — std::collections
│   ├── json.md                  — std::json
│   ├── map.md                   — std::map
│   ├── string.md                — std::string
│   ├── process.md               — std::process
│   ├── regex.md                 — std::regex
│   ├── email.md                 — std::email
│   ├── auth.md                  — std::auth
│   ├── stripe.md                — std::stripe
│   ├── db.md                    — std::db
│   ├── claude.md                — std::claude
│   ├── config.md                — std::config
│   ├── datetime.md              — std::datetime
│   ├── set.md                   — std::set
│   └── math.md                  — std::math
└── appendix/
    ├── keywords.md              — All 41 keywords
    ├── operators.md             — Operator precedence table
    ├── attributes.md            — All @ attributes
    └── coming-from.md           — Quick reference for Python/Rust/JS/C developers
```

## Chapter Format

Each chapter follows:
1. **Opening** — What and why (2-3 sentences)
2. **Core concepts** — Mental model with code examples
3. **Practical examples** — Real patterns developers will use
4. **"Coming from X" callouts** — Inline notes for Python/Rust devs where concepts diverge
5. **Common mistakes** — Pitfalls with fixes
6. **Reference** — Quick-lookup summary of syntax/functions in the chapter

## Stdlib Function Format

```markdown
### function_name

`function_name(param: Type, param2: Type) -> ReturnType`

Description of what it does and when to use it.

**Example:**

    let result = function_name(arg1, arg2)
    println(to_string(result))

**Edge cases:**
- What happens with empty input
- What happens on error

**See also:** related_function, other_function
```

## Estimated Size

- 15 language chapters: ~200-400 lines each (~4,500 lines)
- 22 stdlib references: ~150-300 lines each (~4,000 lines)
- 4 appendix files: ~100-200 lines each (~600 lines)
- Total: ~9,000-10,000 lines across 41 files

## Source Material

All content derived from the actual Kryos language implementation:
- `kryos/compiler/` — lexer, parser, interpreter, codegen, types, ownership, capabilities
- `kryos/stdlib/` — 22 module implementations with function signatures
- `kryos/runtime/` — tensor, agents, probable, streams, lineage, cost
- `kryos/lsp/` — LSP server with 103+ builtin completions
- `tests/programs/` — 19 .kry test programs demonstrating features
- `examples/` — 3 complete example programs
- `README.md` — Existing language overview

## Secondary Benefit

Manual content feeds directly into Kryos LLM training data — the more comprehensive and correct the manual, the better the fine-tuned model understands Kryos.
