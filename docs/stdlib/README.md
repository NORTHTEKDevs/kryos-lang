# Kryos Standard Library

The Kryos standard library ships with every installation. Core builtins are available in every `.kry` program without imports. Stdlib modules provide domain-specific functions and are loaded via `use` statements or auto-registered as globals.

> **Implementation Status:** Core builtins and many stdlib modules are implemented as `.kry` files in `compiler/stdlib/`. Some stdlib modules have corresponding native Rust FFI implementations in `kryos-rt` and `kryos-stdlib-native`. Modules marked as "FFI-backed" have native implementations; modules marked as "Pure Kryos" are implemented entirely in `.kry` files; modules marked as "Planned" have documentation but no implementation yet.

## Module Index

| Module | Status | Description |
|--------|--------|-------------|
| [Core Builtins](core-builtins.md) | **Implemented** | Always-available functions: I/O, math, strings, arrays, conversion, assert |
| [std.string](string.md) | **Implemented** | String manipulation: `upper`, `lower`, `trim`, `split`, `join`, `replace`, `contains`, `starts_with`, `ends_with`, `repeat`, `pad_left`, `pad_right` |
| [std.math](math.md) | **Implemented** | Extended math: `round`, `log10`, `random`, `pi`, `e` (beyond core `sin`, `cos`, `tan`, `log`, `pow`, `sqrt`, `floor`, `ceil`, `min`, `max`, `abs`) |
| [std.collections](collections.md) | **Implemented** | Higher-order: `map`, `filter`, `reduce`, `sort`, `reverse`, `zip`, `enumerate`, `find`, `any`, `all`, `flat_map`, `sum`, `count` |
| [std.map](map.md) | **Built-in type** | `map<K, V>` needs no import: `m[k]`, `m[k] = v`, `contains(m, k)`, `keys(m)`, `len(m)` |
| [std.set](set.md) | **Implemented** | Sorted-array set primitives: `insert`, `contains`, `remove`, `lower_bound` (or `Set` from std.collections) |
| [std.json](json.md) | **Implemented** | JSON: `parse`, `stringify`, `pretty_print`, `get`, `set`, `to_str`/`to_int`/...; constructors `json_string`, `json_object`, ... |
| [std.io](io.md) | **Implemented** (FFI-backed) | File I/O: `file_read`, `file_write`, `file_append`, `file_exists`, `file_delete`, `dir_list`, `dir_create` |
| [std.process](process.md) | **Implemented** (FFI-backed) | Process: `env_get`, `env_has`, `exit`, `argc`, `argv`, `args`, `command` (subprocess builder) |
| [std.net](net.md) | **Implemented** | HTTP client, WebSocket, TCP, URL encoding |
| [std.crypto](crypto.md) | **Implemented** | SHA-256, SHA-512, MD5, HMAC, Base64, hex, random bytes, UUID |
| [std.regex](regex.md) | **Implemented** | Regular expressions (`std::re`): `compile`, `is_match`, `find`, `find_all`, `replace`, `replace_all`, `split` |
| [std.datetime](datetime.md) | **Implemented** | Date/time: `now`, `now_utc`, `now_iso`, `timestamp`, `timestamp_millis`, `format_timestamp`, `sleep` |
| [std.term](term.md) | **Implemented** (FFI-backed) | Terminal: `clear`, `cursor_move`, `size`, `color`, `read_key`, `raw_enable`/`raw_disable`, `draw_box` |
| [std.bytes](bytes.md) | **Implemented** (Pure Kryos) | Codepoint-indexed byte-buffer scanning: `find_byte`, `find_seq`, `compare`, `is_ascii` -- for latin-1-model binary payloads (see `byte_at`) |
| std.tensor | **Implemented** (FFI-backed) | N-dimensional tensors: creation, math, reductions, linear algebra, ML ops. See [AI Runtime](../14-ai-runtime.md). |
| std.stream | **Implemented** (Pure Kryos) | Reactive streams: `map`, `filter`, `take`, `skip`, `reduce`, `collect`. See [AI Runtime](../14-ai-runtime.md). |
| std.probable | **Implemented** (Pure Kryos) | Confidence-aware values, ensemble methods. See [AI Runtime](../14-ai-runtime.md). |
| std.agent | **Implemented** (Pure Kryos) | Agent framework with memory, tools, alignment. See [AI Runtime](../14-ai-runtime.md). |
| std.tracked | **Implemented** (Pure Kryos) | Data lineage tracking. See [AI Runtime](../14-ai-runtime.md). |
| std.cost | **Implemented** (Pure Kryos) | Budget enforcement for AI compute. See [AI Runtime](../14-ai-runtime.md). |
| std.server | Planned | HTTP server framework |
| [std.db](db.md) | Planned | Database connectivity |
| std.auth | Planned | Authentication utilities |
| std.config | Planned | Configuration and environment |
| std.email | Planned | Email sending |
| std.claude | Planned | Anthropic Claude API integration |
| std.stripe | Planned | Stripe payments integration |

## Additional stdlib files (no separate docs)

These `.kry` files are part of the stdlib but documented inline or as part of other modules:

| File | Description |
|------|-------------|
| `chan.kry` | Channel operations (documented in [Concurrency](../09-concurrency.md)) |
| `fs.kry` | Filesystem operations (documented in [std.io](io.md)) |
| `fmt.kry` | String formatting utilities |
| `http.kry` | HTTP client (documented in [std.net](net.md)) |
| `iter.kry` | Iterator utilities |
| `option.kry` | Option type |
| `os.kry` | OS-level operations |
| `path.kry` | Path manipulation |
| `re.kry` | Regex implementation (documented in [std.regex](regex.md)) |
| `result.kry` | Result type |
| `sync.kry` | Synchronization primitives |
| `test.kry` | Test framework utilities |

## Architecture

Stdlib functions are implemented at two levels:

1. **Native Rust FFI** (`kryos-rt` and `kryos-stdlib-native` crates) -- Performance-critical operations: strings, arrays, maps, tensors, file I/O, process management, terminal control. These are `#[no_mangle] pub extern "C" fn` functions linked into every binary.

2. **Pure Kryos** (`compiler/stdlib/*.kry`) -- Higher-level abstractions built on top of native FFI: collections, streams, agents, probability, cost tracking. These are `.kry` source files that wrap native calls or implement logic purely in Kryos.

Core builtins (190+) are registered in the compiler's builtin table and available without imports. Stdlib module functions are accessible via `use` statements.
