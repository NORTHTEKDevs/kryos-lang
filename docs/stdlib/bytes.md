# std::bytes

Codepoint-indexed helpers for scanning a **byte buffer** string -- the
latin-1 model used by `chr`/`char_from`/`byte_at`/`base64_encode`/
`base64_decode`, where one Unicode codepoint (`0..255`) represents one
LOGICAL byte. Every function here walks by codepoint (via
`std::utf8::step_at`), matching `byte_at`'s own contract, rather than
iterating raw UTF-8 byte offsets `0..len(s)` -- `len(s)` overcounts once a
buffer holds any logical byte `>= 0x80` (it needs 2 UTF-8 bytes to encode),
so a raw byte-offset walk silently misreads every value past the first high
byte. If you are inspecting binary payloads (archive formats, checksums,
JWT signatures, WebSocket frames) this is the module to reach for instead of
hand-rolling the walk yourself.

```kryos
use std::bytes
```

Not listed in `docs/stdlib/README.md`'s module index prior to this entry,
and never referenced from any file under `examples/` -- genuinely
undiscoverable from the public docs surface before this page existed. See
`examples/showcase/karc.kry` for a full worked example (a binary archive
tool) built around this exact byte-buffer model.

---

## Functions

### find_byte

`find_byte(s: str, needle_byte: i64) -> i64`

Index (codepoint / logical-byte position, matching `byte_at`) of the first
occurrence of `needle_byte` in `s`, or `-1` if not found.

```kryos
use std::bytes::{find_byte}

let buf: str = chr(10) + chr(200) + chr(30)
println(to_string(find_byte(buf, 200)))   // 1
```

---

### find_seq

`find_seq(haystack: str, needle: str) -> i64`

Index (codepoint / logical-byte position) of the first occurrence of the
byte sequence `needle` inside `haystack`, or `-1`. Compares by logical byte
VALUE, not raw UTF-8 bytes, so it is correct regardless of each matched
byte's own UTF-8 encoding width.

---

### compare

`compare(a: str, b: str) -> i64`

Lexicographic comparison by logical byte value: `-1` if `a < b`, `0` if
equal, `1` if `a > b`.

---

### is_ascii

`is_ascii(s: str) -> bool`

`true` if every logical byte in `s` has code `< 128`.

---

## See also

- `byte_at`, `chr`, `char_from` -- [Core Builtins](core-builtins.md)
- `std::utf8::is_valid`, `std::utf8::codepoint_count`, `std::utf8::step_at`
- `examples/showcase/karc.kry` -- a binary archive tool exercising this
  module end to end, including the invalid-UTF-8 trap `byte_at` itself falls
  into (documented in that file's header and in `core-builtins.md`).
