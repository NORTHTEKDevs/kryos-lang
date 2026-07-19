# std::re

Regular expression matching, searching, replacement, and splitting. Pattern syntax is that of the Rust `regex` engine: full support for character classes, unicode classes (`\p{L}`), anchors, quantifiers, alternation, and capture groups -- but **no backreferences (`\1`) and no lookaround**; those patterns are rejected at `compile()` time.

```kryos
use std::re
```

---

## Types

### Regex

A compiled regular expression. Created with `compile()`. Reuse a `Regex` when applying the same pattern multiple times -- compilation has overhead.

### Match

A single match result returned by `find()` and the elements of `find_all()`.

| Field    | Type   | Description                               |
|----------|--------|-------------------------------------------|
| `text`   | `str`  | The matched substring                     |
| `start`  | `i64`  | Byte offset where the match begins        |
| `end`    | `i64`  | Byte offset immediately after the match   |
| `found`  | `bool` | Whether anything matched                  |

### Captures

Capture-group results returned by `captures()`.

| Field    | Type    | Description                                              |
|----------|---------|----------------------------------------------------------|
| `groups` | `[str]` | `groups[0]` is the whole match, `groups[i]` the i-th parenthesized group (`""` if the group did not participate) |
| `count`  | `i32`   | Number of explicit groups (excludes the whole match)     |
| `found`  | `bool`  | Whether anything matched (`groups` is empty when false)  |

---

## Compiling

### compile

`compile(pattern: str) -> Regex`

Compile a regular expression pattern. Throws a catchable error (`re error: invalid regex pattern: ...`) if the pattern is invalid or uses unsupported syntax (backreferences, lookaround).

**Example:**
```kryos
use std::re

let re = compile("\\d{3}-\\d{4}")
```

**Note:** Use a compiled `Regex` when calling `is_match`, `find`, `find_all`, etc. with the same pattern repeatedly. The standalone convenience functions (`is_match`, `find`, etc.) accept a raw pattern string and compile it each call -- fine for one-off use, but prefer `compile` in loops.

---

## Matching

### is_match

`is_match(pattern: str, input: str) -> bool`

Return `true` if `pattern` matches anywhere in `input`.

**Example:**
```kryos
use std::re

println(is_match("\\d+", "order 42"))        // true
println(is_match("^hello", "hello world"))   // true
println(is_match("^hello", "say hello"))     // false
```

---

### find

`find(pattern: str, input: str) -> Match`

Return the first `Match` of `pattern` in `input`. If no match is found, `match.found` is `false`, `match.text` is an empty string, and `match.start` / `match.end` are both `0` -- always check `found`, not a sentinel offset.

**Example:**
```kryos
use std::re

let m = find("\\d+", "item #42 qty 5")
println(m.text)    // "42"
println(m.start)   // 6
println(m.end)     // 8
```

---

### find_all

`find_all(pattern: str, input: str) -> [Match]`

Return all non-overlapping matches of `pattern` in `input` as an array of `Match` values.

**Example:**
```kryos
use std::re

let matches = find_all("\\d+", "scores: 95, 87, 73")
let i = 0
while i < len(matches) {
    println(matches[i].text)    // "95", "87", "73"
    i = i + 1
}
```

---

### captures

`captures(pattern: str, input: str) -> Captures`

Return the capture groups of the first match. `groups[0]` is the whole match; `groups[i]` is the i-th parenthesized group. A group that did not participate (e.g. an unmatched optional group) is `""`.

**Example:**
```kryos
use std::re

fn main() {
    let c = captures("(\\w+)=(\\d+)", "key=42 x=7")
    if c.found {
        println(c.groups[0])   // "key=42"
        println(c.groups[1])   // "key"
        println(c.groups[2])   // "42"
    }
}
```

A compiled `Regex` also has `re.captures(text)` and `re.captures_at(text, from)` (first match at or after byte offset `from`), plus `re.find_at(text, from)` for offset searches. `^` always anchors to the true start of the text, never to `from`.

---

### count

`count(pattern: str, input: str) -> i32`

Return the number of non-overlapping matches of `pattern` in `input`.

**Example:**
```kryos
use std::re

println(count("\\bthe\\b", "the cat and the dog"))   // 2
println(count("[aeiou]", "hello world"))              // 3
```

---

## Replacement

### replace

`replace(pattern: str, input: str, replacement: str) -> str`

Replace the first occurrence of `pattern` in `input` with `replacement`. Returns the modified string.

In the replacement, `$N` substitutes capture group N (`$0` is the whole match), `$$` is a literal dollar sign, and a `$N` that references a group the pattern doesn't have is left as-is.

**Example:**
```kryos
use std::re

let result = replace("\\d+", "version 1.2.3", "X")
println(result)   // "version X.2.3"
```

---

### replace_all

`replace_all(pattern: str, input: str, replacement: str) -> str`

Replace all non-overlapping occurrences of `pattern` in `input` with `replacement`. Group references work as in `replace()`: `$N`, `$$`, out-of-range left as-is.

**Example:**
```kryos
use std::re

let result = replace_all("\\s+", "too   many    spaces", " ")
println(result)   // "too many spaces"

let bracketed = replace_all("(\\d+)", "abc123def456", "[$1]")
println(bracketed)   // "abc[123]def[456]"
```

---

## Splitting

### split

`split(pattern: str, input: str) -> [str]`

Split `input` at every occurrence of `pattern` and return the array of parts.

**Example:**
```kryos
use std::re

let parts = split("\\s*,\\s*", "Alice, Bob,  Charlie,Dave")
println(parts)   // ["Alice", "Bob", "Charlie", "Dave"]

let tokens = split("\\s+", "  one two   three  ")
println(tokens)  // ["", "one", "two", "three", ""]
```

---

## Validation Helpers

These functions apply pre-built patterns for common validation tasks.

### is_email

`is_email(s: str) -> bool`

Return `true` if `s` looks like a valid email address.

**Example:**
```kryos
use std::re

println(is_email("alice@example.com"))   // true
println(is_email("not-an-email"))        // false
```

---

### is_url

`is_url(s: str) -> bool`

Return `true` if `s` looks like a valid URL (http or https).

**Example:**
```kryos
use std::re

println(is_url("https://example.com"))         // true
println(is_url("http://localhost:3000/api"))   // true
println(is_url("example.com"))                 // false
```

---

### is_ipv4

`is_ipv4(s: str) -> bool`

Return `true` if `s` is a valid IPv4 address.

**Example:**
```kryos
use std::re

println(is_ipv4("192.168.1.1"))   // true
println(is_ipv4("999.0.0.1"))     // false
println(is_ipv4("localhost"))     // false
```

---

### is_hex

`is_hex(s: str) -> bool`

Return `true` if `s` consists entirely of valid hexadecimal characters (`0-9`, `a-f`, `A-F`).

**Example:**
```kryos
use std::re

println(is_hex("deadBEEF"))   // true
println(is_hex("0xff"))       // false  (0x prefix is not hex-only)
println(is_hex("xyz"))        // false
```

---

## Escaping

### escape

`escape(s: str) -> str`

Escape all regex metacharacters in `s` so it can be used as a literal string in a pattern.

**Example:**
```kryos
use std::re

let user_input = "3.14 (approximately)"
let safe = escape(user_input)
println(safe)   // "3\\.14 \\(approximately\\)"

let pattern = "prefix-" + safe + "-suffix"
println(is_match(pattern, "prefix-3.14 (approximately)-suffix"))   // true
```

---

## Complete Example

```kryos
use std::re

// Validate user input
let email = "user@example.com"
if not is_email(email) {
    println("invalid email")
}

// Extract all numbers from a string
let log_line = "processed 142 records in 3 batches with 0 errors"
let numbers = find_all("\\d+", log_line)
let i = 0
while i < len(numbers) {
    println(numbers[i].text)   // "142", "3", "0"
    i = i + 1
}

// Normalize whitespace
let messy = "  hello    world  "
let clean = replace_all("\\s+", messy, " ")
println(clean)   // " hello world "

// Split on delimiter with optional surrounding whitespace
let csv_line = "Alice , 30 , engineer"
let fields = split("\\s*,\\s*", csv_line)
println(fields)   // ["Alice", "30", "engineer"]
```
