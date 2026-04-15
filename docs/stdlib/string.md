# std::string

Extended string manipulation functions. These complement the core string builtins (`len`, `split`, `join`, `trim`, `starts_with`, `ends_with`, `contains`, `replace`, `substr`, `upper`, `lower`) with operations for text formatting, padding, parsing, and searching.

> **Implementation Status:** These functions are planned for `std::string`. The import is parsed and the function signatures are reserved. Full runtime implementation is in progress.

```kryos
use std::string
```

---

### string_repeat

`string_repeat(s: str, n: i64) -> str`

Repeat a string `n` times.

**Example:**
```kryos
println(string_repeat("ha", 3))   // hahaha
println(string_repeat("-", 40))   // ----------------------------------------
```

**Edge cases:**
- `n = 0` returns an empty string.
- The input is coerced to a string.

---

### string_pad_left

`string_pad_left(s: str, width: i64) -> str`
`string_pad_left(s: str, width: i64, char: str) -> str`

Pad a string on the left to reach the target width. Default pad character is a space.

**Example:**
```kryos
println(string_pad_left("42", 5))        // "   42"
println(string_pad_left("42", 5, "0"))   // "00042"
println(string_pad_left("hello", 3))     // "hello" (no truncation)
```

**Edge cases:**
- If the string is already longer than `width`, it is returned unchanged (no truncation).
- Only single-character pad strings are reliable.

**See also:** string_pad_right

---

### string_pad_right

`string_pad_right(s: str, width: i64) -> str`
`string_pad_right(s: str, width: i64, char: str) -> str`

Pad a string on the right to reach the target width. Default pad character is a space.

**Example:**
```kryos
println(string_pad_right("Name", 20, "."))  // "Name................"
println(string_pad_right("hi", 10))         // "hi        "
```

**Edge cases:**
- If the string is already longer than `width`, it is returned unchanged.

**See also:** string_pad_left

---

### string_lines

`string_lines(s: str) -> [str]`

Split a string into an array of lines (split on `\n`).

**Example:**
```kryos
let text = "line one\nline two\nline three"
let lines = string_lines(text)
println(len(lines))   // 3
println(lines[0])     // line one
```

**Edge cases:**
- A trailing newline produces an empty string as the last element.

---

### string_index

`string_index(s: str, sub: str) -> i64`

Find the first occurrence of a substring. Returns the zero-based index, or `-1` if not found.

**Example:**
```kryos
println(string_index("hello world", "world"))  // 6
println(string_index("hello world", "xyz"))    // -1
```

**See also:** string_count, contains

---

### string_count

`string_count(s: str, sub: str) -> i64`

Count non-overlapping occurrences of a substring.

**Example:**
```kryos
println(string_count("banana", "an"))  // 2
println(string_count("hello", "z"))    // 0
```

**See also:** string_index

---

### to_int

`to_int(s: str) -> i64`

Parse a string as an integer. Alias for the core builtin `parse_int`.

**Example:**
```kryos
let port = to_int("8080")
println(port + 1)  // 8081
```

**Edge cases:**
- Raises a runtime error if the string cannot be parsed as an integer.

**See also:** to_float, parse_int

---

### to_float

`to_float(s: str) -> f64`

Parse a string as a floating-point number. Alias for the core builtin `parse_float`.

**Example:**
```kryos
let ratio = to_float("3.14")
println(ratio * 2)  // 6.28
```

**Edge cases:**
- Raises a runtime error if the string cannot be parsed as a float.

**See also:** to_int, parse_float
