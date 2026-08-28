# 16 · The standard library tour

After this chapter you will know which of the 66 `std::*` modules to reach
for by domain, without having to grep `compiler/stdlib/` or guess at a
function name. This chapter is reference-style on purpose -- read it once
end to end to build a map, then come back to the relevant section whenever
a task needs a module you have not used yet. The exhaustive per-symbol
reference for any module lives at [`docs/STDLIB.md`](../../STDLIB.md) and
[`docs/stdlib/`](../../stdlib/README.md); this chapter is the guided walk
that tells you which page to open.

Every module here is imported explicitly with `use std::<module>::{...}` --
none of it is a global builtin. (The always-available builtins --
`println`, `len`, `to_string`, `push`, and friends -- are cataloged in
[`docs/STDLIB.md`](../../STDLIB.md)'s section 1 and used throughout this
book already.) Remember [Chapter 15](15-modules-and-packages.md)'s flat
namespace rule as you import from more than one module at once: two
modules below share a name often enough that it is worth scanning for a
collision before you commit to an import list.

## A worked example touching three domains

Real programs mix modules. Here is one program combining JSON handling
(a global builtin, not a `std::json` import -- more on that below) with
`std::iter`'s functional helpers:

```kryos
use std::iter::{map, filter, sum}

fn main() {
    let doc = json_parse("{{\"name\": \"kryos\", \"stars\": 42}}")
    let name = json_to_str(json_get(doc, "name"))
    let stars = json_to_int(json_get(doc, "stars"))
    println(name + " has " + to_string(stars) + " stars")

    let nums: [i64] = [1, 2, 3, 4, 5]
    let doubled = map(nums, |n| n * 2)
    let evens = filter(doubled, |n| n % 4 == 0)
    println("total: " + to_string(sum(evens)))
}
```

Output:

```
kryos has 42 stars
total: 12
```

Two things worth noticing: `json_parse`/`json_get`/`json_to_str`/
`json_to_int` are global builtins (Chapter 8 covers the `{{`/`}}`
literal-brace escaping the JSON string literal needed here), while `map`/
`filter`/`sum` are `std::iter` imports that infer their closure parameter
types from the array they operate on -- no annotation needed on `n` in
either closure.

## Collections and iteration (11 modules)

Beyond the core `[T]`/`map<K, V>` builtins (Chapter 7), these modules give
you named data structures and a functional-iteration toolkit.

| Module | What it gives you |
|---|---|
| `std::collections` | `List<T>`/`Set<T>`/`Stack<T>`/`Queue<T>`/`Deque<T>`/`Dict<K,V>` generic wrapper types -- annotate the `let` (`let ls: List<str> = List.new()`) so `T` binds from construction. |
| `std::iter` | The functional toolkit: `map`, `filter`, `fold`, `reduce`, `scan`, `sum`, `enumerate`, `zip`, `unzip`, `chunks`, `windows`, `flat_map`, and more -- all infer element types from the array they operate on. |
| `std::deque` | A double-ended queue over `[i64]` -- push/pop at either end; front-side ops are O(N). |
| `std::queue` | A plain FIFO queue over `[i64]`. |
| `std::stack` | A plain LIFO stack over `[i64]`. |
| `std::heap` | A binary min-heap (priority queue) over `[i64]`. |
| `std::set` | A sorted-array set of `i64` with O(log N) lookup. |
| `std::trie` | An ASCII prefix tree -- prefix search, autocomplete-shaped problems. |
| `std::lru` | An LRU cache backed by parallel arrays. |
| `std::interval` | Sorted `[start, end)` interval-set operations -- scheduling/overlap problems. |
| `std::slice_ops` | `take`/`drop`/`partition`/`is_sorted`/`bsearch` over `[i64]`, pure functions with no hidden state. |

## Strings, text, and data interchange (10 modules)

| Module | What it gives you |
|---|---|
| `std::string` | Higher-level string ops beyond the core builtins -- `find` (substring search; there is no global `index_of`, per Chapter 8), plus more. |
| `std::strext` | Extended string ops that didn't fit the core builtin set or `std::string`. |
| `std::bytes` | Byte-level operations over strings, treating characters as raw bytes -- see the latin-1 byte-buffer caveat in Chapter 8 before reaching for this on real UTF-8 text. |
| `std::utf8` | Codepoint-aware helpers (`codepoint_count`, `is_valid`) for when byte-indexed operations (`substr`, `len`) are not what you want on multibyte text. |
| `std::fuzzy` | Fuzzy string matching -- Levenshtein distance, Jaro-Winkler, `closest`-match lookup. |
| `std::re` | Regular expressions. |
| `std::csv` | RFC-4180 CSV: `parse`, `parse_line`, `to_line`, `serialize`. |
| `std::json` | Ergonomic wrappers around the `json_*` global builtins (which you will often reach for directly, as in this chapter's worked example) -- see `std::json`'s own export list rather than assuming a `json_`-prefixed name lives there too. |
| `std::fmt` | `format(template, args: [any])` and other pretty-print helpers -- note `format`'s `any`-erasure caveat for non-`i64` arguments (Chapter 9's `any` limitation); prefer `+` and per-value `to_string` until that lands. |
| `std::numfmt` | Number formatting: hex, binary, padded, byte-count-with-unit. |

## Math and numeric (9 modules)

| Module | What it gives you |
|---|---|
| `std::math` | Constants (`pi`, `e`) and functions beyond the core `sqrt`/`pow`/trig builtins. |
| `std::mathx` | Extended integer math -- `gcd`/`lcm` and similar (mind the `i64::MIN` magnitude limit noted in `CLAUDE.md`'s gotcha list). |
| `std::matrix` | Small dense `i64` matrices, stored row-major as a flat `[i64]`. |
| `std::tensor` | N-dimensional tensor operations, FFI-backed. |
| `std::stat` | Running statistics (mean, variance, and similar) over `[i64]` arrays. |
| `std::histogram` | A fixed-bucket histogram. |
| `std::diff_ops` | Longest-common-subsequence and edit-count, pure Kryos, O(m·n). |
| `std::semver` | Semantic-version parsing and comparison (`MAJOR.MINOR.PATCH[-pre][+build]`) -- what `kryos pkg` itself uses for dependency resolution (Chapter 15). |
| `std::random` | A deterministic, seedable PRNG -- `new_rng`, `next_i64`, `random_f64`, `shuffle`. Reach for this over an ambient global-random builtin whenever a test needs reproducible output. |

## Time and flow control (5 modules)

| Module | What it gives you |
|---|---|
| `std::datetime` | Calendar dates, formatting, and duration arithmetic beyond the raw `time_now*` builtins. |
| `std::duration` | Duration arithmetic and human-readable formatting (`"2h 15m"` style). |
| `std::backoff` | Exponential backoff for retry loops -- pure: it returns the next delay, your code does the sleeping. |
| `std::ratelimit` | A token-bucket rate limiter. |
| `std::circuit` | A circuit breaker (CLOSED/OPEN/HALF_OPEN) for wrapping a flaky dependency. |

## OS, filesystem, and terminal (7 modules)

| Module | What it gives you |
|---|---|
| `std::io` | File I/O, buffered readers/writers, and console I/O beyond the core `file_read`/`file_write` builtins. |
| `std::fs` | Higher-level filesystem helpers built on the `file_*` builtins -- `read_file` throws instead of panicking, unlike the raw builtin (Chapter 12). |
| `std::os` | Platform detection: `name`, `arch`, `is_linux`, and similar. |
| `std::path` | Path manipulation with filesystem awareness. |
| `std::pathext` | Pure string-based path manipulation with no filesystem syscalls -- prefer this when you are just parsing a path string, not touching disk. |
| `std::process` | Process spawn, environment access, and pipe wrappers -- needs the `process` capability, same as the raw builtins it wraps. |
| `std::term` | Terminal control: size, raw mode, cursor movement, ANSI color/style, key reading. |

## Networking and data services (4 modules)

| Module | What it gives you |
|---|---|
| `std::net` | TCP networking and a simple HTTP client -- the `http_get`/`http_post` functions referenced throughout Chapter 11 live here. |
| `std::http` | A higher-level HTTP request/response abstraction on top of `std::net`. |
| `std::smtp` | A minimal SMTP-over-implicit-TLS (SMTPS) client for sending mail. |
| `std::db` | A SQLite-flavored database client. |

## Concurrency (4 modules)

Covered in depth in [Chapter 13](13-concurrency.md); listed here for
completeness of the module map.

| Module | What it gives you |
|---|---|
| `std::chan` | Higher-level wrappers over the `chan`/`send`/`recv` builtins -- `WaitGroup`, `Semaphore`, fan-out/fan-in, `select` case builders. |
| `std::sync` | `Mutex`, `AtomicInt`/`AtomicBool`, `Once`, `WaitGroup`, `SpinLock`. |
| `std::stream` | Reactive/pull-based streams for continuous data processing, pure Kryos. |
| `std::semaphore` | A single-threaded counting semaphore (distinct from `std::sync`'s primitives, which are cross-thread). |

## Security and hashing (4 modules)

| Module | What it gives you |
|---|---|
| `std::crypto` | Hashing, random-byte generation, `uuid_v4`/`uuid_parse` beyond the raw `sha256`/`base64_*` builtins. |
| `std::jwt` | JSON Web Tokens (RFC 7519): HS256 and EdDSA signing/verification. |
| `std::hash` | Non-cryptographic hash functions -- for hash tables and checksums, not security. |
| `std::bloom` | A Bloom filter for probabilistic set membership. |

## Errors, results, and logging (3 modules)

| Module | What it gives you |
|---|---|
| `std::result` | `Result<T, E>` helpers beyond the language's own `Ok`/`Err`/`?` support (Chapter 12). |
| `std::option` | `Option<T>` helpers beyond `Some`/`None` (Chapter 2). |
| `std::log` | Single-line structured logging. |

## AI/agent runtime (6 modules)

An experimental surface for building LLM-backed tools and agents directly
in Kryos, distinct from the compiler-internals AI-runtime work described in
[`docs/14-ai-runtime.md`](../../14-ai-runtime.md).

| Module | What it gives you |
|---|---|
| `std::agent` | An agent runtime with memory, tool invocation, and an alignment-level enforcement knob (`ALIGNMENT_STRICT` refuses tools matching a risky-verb deny-list; treat this as a heuristic guardrail, not a sandbox -- pair it with real `@capabilities` for hard enforcement). |
| `std::agent_bridge` | Governance bridges across `tracked`, `cost`, `probable`, and `llm` -- lets an agent ask, in one call, whether an action is within budget/lineage/confidence bounds. |
| `std::llm` | Chat-completion clients: OpenAI-compatible and Anthropic, with budget-aware `chat`/`complete`/`chat_within` variants. |
| `std::tracked` | Data-lineage tracking for AI safety/compliance -- know where a value came from. |
| `std::cost` | Computation cost tracking and budget enforcement for AI workloads. |
| `std::probable` | Confidence-tagged values, for carrying an AI prediction's uncertainty alongside its result. |

## Low-level and platform (2 modules)

| Module | What it gives you |
|---|---|
| `std::ffi` | A pure-Kryos surface over the runtime's `dl*` dynamic-loading helpers, for calling into `extern "C"` functions -- needs the `ffi` capability (Chapter 19 goes deep here). |
| `std::wasm` | Language-level bindings for the `--backend wasm` browser host imports (`dom_set_text`, `canvas_fill_rect`, and similar) -- Chapter 18 covers the wasm backend itself. |

## Testing (1 module)

| Module | What it gives you |
|---|---|
| `std::test` | Test-framework primitives beyond the `@test` annotation and `assert`/`assert_eq` builtins used throughout this book -- Chapter 17 covers `kryos test` end to end. |

## Common mistakes

**Reaching for a `std::json::json_*`-named function.** The global
`json_parse`/`json_get`/`json_to_str`/`json_to_int` builtins need no
import at all; `std::json`'s own exports are a separate, smaller wrapper
layer with different names. Check which one you actually want before
adding a `use`.

**Forgetting the `{{`/`}}` escape inside a JSON string literal.** Every
Kryos string interpolates (Chapter 8); a raw `{` inside a `json_parse(...)`
argument tries to open an interpolation. Escape it or build the string
another way.

**Assuming `std::bytes` is UTF-8-safe.** It is a latin-1 byte-buffer model
(codepoints 0-255 map to bytes); anything with a codepoint above `0xFF`
truncates silently. Use `std::utf8` for real multibyte text.

## Exercises

1. Pick one module from a domain you have not used yet in this book (say,
   `std::backoff` or `std::trie`) and write a 10-15 line program that
   imports and calls at least two of its functions. Run it.
2. Rewrite this chapter's worked example using `std::json`'s own exports
   instead of the global `json_*` builtins. Confirm the collision rules
   from Chapter 15 (does importing `std::json` alongside `std::iter`
   create any naming conflicts here?).
3. Find a module in this chapter whose one-line description does not
   satisfy your curiosity, and open its source under `compiler/stdlib/` to
   read the header comment and function list directly.

## Summary

- All 66 `std::*` modules are explicit `use` imports -- nothing here is a
  global builtin, and [`docs/STDLIB.md`](../../STDLIB.md) is the exhaustive
  per-symbol reference this chapter maps you into.
- Collections/iteration, strings/text/serialization, math, time/flow
  control, OS/filesystem, networking, concurrency, security/hashing,
  errors/logging, the AI/agent surface, low-level/platform, and testing
  are the eleven domains this tour organizes the 66 modules into.
- `std::json`'s own exports and the global `json_*` builtins are two
  different surfaces with different names -- know which one a piece of
  code is actually calling.
- The AI/agent modules (`agent`, `agent_bridge`, `llm`, `tracked`, `cost`,
  `probable`) are a real but experimental layer -- treat `agent`'s
  alignment enforcement as a heuristic, not a substitute for
  `@capabilities`.
- When in doubt about a specific function's signature, `docs/STDLIB.md`
  and `docs/stdlib/<module>.md` are the ground truth; this chapter is the
  map, not the reference.

Next: [Building and testing real programs](17-building-and-testing.md)
