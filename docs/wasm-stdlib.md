# Kryos WASM stdlib import surface

Starting with v2.3, the Kryos WASM backend imports a broader set of host
primitives that mirror the native-side stdlib. All imports live under the
`env` module. Strings and arrays use the **packed-i64 convention**:

```
packed_i64 = (length << 32) | (offset & 0xffffffff)
```

where `offset` is a byte position inside the module's exported linear
`memory` and `length` is the byte count.

The reference host runner is `examples/wasm_runner.js` (Node) and
`examples/wasm_web_runner.html` (browser). The canonical full-surface Node
host (all 31 env imports, used by CI's wasm-smoke job) is
`tools/wasm-host/run.mjs`:

```
kryos build app.kry --backend wasm -o app.wasm
node tools/wasm-host/run.mjs app.wasm
```

**WASI is not supported.** Kryos modules import from `env`, not
`wasi_snapshot_preview1`, so wasmtime/wasmer cannot run them. A real WASI
target is tracked as a future feature.

## Basic I/O (v0.2)

| Import | Signature | Notes |
|---|---|---|
| `kryos_print_i64` | `(i64) -> ()` | Print one i64 followed by newline. |
| `kryos_print_f64` | `(f64) -> ()` | Print one f64. |
| `kryos_print_str` | `(i32 off, i32 len) -> ()` | Read UTF-8 from memory. |
| `kryos_string_concat` | `(i32, i32, i32, i32) -> i64` | Returns packed string. |
| `kryos_array_new` | `(i32 count) -> i64` | Zero-init i64 array, packed handle. |
| `kryos_array_get` | `(i64 packed, i32 idx) -> i64` | |
| `kryos_array_set` | `(i64 packed, i32 idx, i64 val) -> ()` | |

## Web host (v0.4)

| Import | Signature | Notes |
|---|---|---|
| `kryos_dom_set_text` | `(i32, i32, i32, i32) -> ()` | id, text. |
| `kryos_dom_get_value` | `(i32, i32) -> i64` | Returns packed string. |
| `kryos_alert` | `(i32, i32) -> ()` | |
| `kryos_canvas_fill_rect` | `(i32,i32, i64,i64,i64,i64, i32,i32) -> ()` | id, x, y, w, h, color. |
| `kryos_canvas_clear` | `(i32, i32) -> ()` | |
| `kryos_fetch_text` | `(i32, i32) -> i64` | URL string in, packed string out. |

## Stdlib parity primitives (v2.3)

These cover the strings/arrays/JSON/regex/HTTP surface that the native
stdlib already provides. They are declared as imports by every WASM
build so that future Kryos→WASM stdlib bindings can lower directly to
them without further changes to the backend.

### Strings

| Import | Signature |
|---|---|
| `kryos_string_length` | `(i64) -> i32` |
| `kryos_string_slice` | `(i64, i32 start, i32 end) -> i64` |
| `kryos_string_to_upper` | `(i64) -> i64` |
| `kryos_string_to_lower` | `(i64) -> i64` |
| `kryos_string_trim` | `(i64) -> i64` |
| `kryos_string_index_of` | `(i64 haystack, i64 needle) -> i32` |
| `kryos_string_parse_int` | `(i64) -> i64` |
| `kryos_string_parse_float` | `(i64) -> f64` |

`index_of` returns `-1` when the needle is absent.

### Arrays

| Import | Signature |
|---|---|
| `kryos_array_length` | `(i64) -> i32` |
| `kryos_array_push` | `(i64, i64) -> i64` |
| `kryos_array_pop` | `(i64) -> i64` |

`push` returns a **new packed handle** because growth may reallocate. The old
handle is invalidated and must not be reused.

`pop` returns `-1` when the array is empty.

### JSON

| Import | Signature |
|---|---|
| `kryos_json_parse` | `(i64 packed_str) -> i64 handle` |
| `kryos_json_stringify` | `(i64 handle) -> i64 packed_str` |
| `kryos_json_get_int` | `(i64 handle, i32 key_off, i32 key_len) -> i64` |
| `kryos_json_get_str` | `(i64 handle, i32 key_off, i32 key_len) -> i64 packed_str` |

`json_parse` returns an opaque host-side handle. Negative handles indicate
parse failure. Handles outlive the WASM call but are scoped to the host
runner instance.

### Regex

| Import | Signature |
|---|---|
| `kryos_regex_test` | `(i64 pattern, i64 subject) -> i32` |
| `kryos_regex_replace` | `(i64 pattern, i64 subject, i64 replacement) -> i64` |

Pattern syntax is JavaScript-flavored regex when running against the
reference host runner; embedders are free to use any regex engine.

### HTTP

| Import | Signature |
|---|---|
| `kryos_http_fetch` | `(i32 m_off, i32 m_len, i32 u_off, i32 u_len, i32 b_off, i32 b_len) -> i64` |

The first three pairs are the HTTP method (e.g. `"GET"`, `"POST"`),
the URL, and an optional request body. Returns a packed response body
string. Browser embedders should implement this with `XMLHttpRequest`
(sync) or expose an async wrapper through the JS bridge. The Node
reference runner returns an empty body and logs the request to stderr.

## Capability vs binding

These imports are the **capability surface**: the WASM backend
guarantees them as available to every module. Wiring them to specific
Kryos built-ins (`str.upper()`, `[].push()`, `http.get()`, etc.) is
done by the `kryos-stdlib-wasm` shim which lowers AST → MIR calls into
these imports. Programs that only need the capability surface can call
the imports directly via FFI declarations.
