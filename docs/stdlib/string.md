# std::string

Extended string manipulation functions. These complement the core string builtins (`len`, `split`, `join`, `trim`, `starts_with`, `ends_with`, `contains`, `replace`, `substr`, `to_upper`, `to_lower`) with operations for text formatting, padding, parsing, and searching.

```kryos
import std::string
```

---

### string_repeat

`string_repeat(s: String, n: Int) -> String`

Repeat a string `n` times.

**Example:**
```kryos
print(string_repeat("ha", 3))   // hahaha
print(string_repeat("-", 40))   // ----------------------------------------
```

**Edge cases:**
- `n = 0` returns an empty string.
- The input is coerced to a string.

---

### string_pad_left

`string_pad_left(s: String, width: Int) -> String`
`string_pad_left(s: String, width: Int, char: String) -> String`

Pad a string on the left to reach the target width. Default pad character is a space.

**Example:**
```kryos
print(string_pad_left("42", 5))        // "   42"
print(string_pad_left("42", 5, "0"))   // "00042"
print(string_pad_left("hello", 3))     // "hello" (no truncation)
```

**Edge cases:**
- If the string is already longer than `width`, it is returned unchanged (no truncation).
- Only single-character pad strings are reliable.

**See also:** string_pad_right

---

### string_pad_right

`string_pad_right(s: String, width: Int) -> String`
`string_pad_right(s: String, width: Int, char: String) -> String`

Pad a string on the right to reach the target width. Default pad character is a space.

**Example:**
```kryos
print(string_pad_right("Name", 20, "."))  // "Name................"
print(string_pad_right("hi", 10))         // "hi        "
```

**Edge cases:**
- If the string is already longer than `width`, it is returned unchanged.

**See also:** string_pad_left

---

### string_lines

`string_lines(s: String) -> Array`

Split a string into an array of lines (split on `\n`).

**Example:**
```kryos
let text = "line one\nline two\nline three"
let lines = string_lines(text)
print(len(lines))   // 3
print(lines[0])     // line one
```

**Edge cases:**
- A trailing newline produces an empty string as the last element.

---

### string_index

`string_index(s: String, sub: String) -> Int`

Find the first occurrence of a substring. Returns the zero-based index, or `-1` if not found.

**Example:**
```kryos
print(string_index("hello world", "world"))  // 6
print(string_index("hello world", "xyz"))    // -1
```

**See also:** string_count, contains

---

### string_count

`string_count(s: String, sub: String) -> Int`

Count non-overlapping occurrences of a substring.

**Example:**
```kryos
print(string_count("banana", "an"))  // 2
print(string_count("hello", "z"))    // 0
```

**See also:** string_index

---

### to_int

`to_int(s: String) -> Int`

Parse a string as an integer.

**Example:**
```kryos
let port = to_int("8080")
print(port + 1)  // 8081
```

**Edge cases:**
- Raises a runtime error if the string cannot be parsed as an integer.

**See also:** to_float

---

### to_float

`to_float(s: String) -> Float`

Parse a string as a floating-point number.

**Example:**
```kryos
let ratio = to_float("3.14")
print(ratio * 2)  // 6.28
```

**Edge cases:**
- Raises a runtime error if the string cannot be parsed as a float.

**See also:** to_int
