# std::string

Extended string manipulation functions. These complement the core string builtins (`len`, `split`, `join`, `trim`, `starts_with`, `ends_with`, `contains`, `replace`, `substr`, `upper`, `lower`) with additional searching, padding, transformation, and formatting utilities.

Strings in Kryos are UTF-8 sequences. Indexing is byte-based.

```kryos
use std::string
```

---

## Query

### is_empty

`is_empty(s: str) -> bool`

Return `true` if the string has zero length.

**Example:**
```kryos
use std::string

println(is_empty(""))       // true
println(is_empty("hello"))  // false
```

**See also:** `len`

---

### contains

`contains(haystack: str, needle: str) -> bool`

Return `true` if `haystack` contains `needle`.

**Example:**
```kryos
println(contains("hello world", "world"))  // true
println(contains("hello world", "xyz"))    // false
```

**Note:** This shadows the core builtin `contains`. The behavior is identical.

**See also:** `find`

---

### starts_with

`starts_with(s: str, prefix: str) -> bool`

Return `true` if `s` begins with `prefix`.

**Example:**
```kryos
println(starts_with("hello", "he"))   // true
println(starts_with("hello", "lo"))   // false
```

---

### ends_with

`ends_with(s: str, suffix: str) -> bool`

Return `true` if `s` ends with `suffix`.

**Example:**
```kryos
println(ends_with("hello.kry", ".kry"))  // true
println(ends_with("hello", "he"))        // false
```

---

### find

`find(haystack: str, needle: str) -> i64`

Find the first occurrence of `needle` in `haystack`. Returns the zero-based byte index, or `-1` if not found.

**Example:**
```kryos
use std::string

println(find("hello world", "world"))  // 6
println(find("hello world", "xyz"))    // -1
println(find("abcabc", "bc"))          // 1
```

**Edge cases:**
- If `needle` is empty, returns `0`.

**See also:** `rfind`, `contains`

---

### rfind

`rfind(haystack: str, needle: str) -> i64`

Find the last occurrence of `needle` in `haystack`. Returns the zero-based byte index, or `-1` if not found.

**Example:**
```kryos
use std::string

println(rfind("abcabc", "bc"))   // 4
println(rfind("hello", "xyz"))   // -1
```

**Edge cases:**
- If `needle` is empty, returns `len(haystack)`.

**See also:** `find`

---

## Case Conversion

### to_upper

`to_upper(s: str) -> str`

Return a copy of `s` with all ASCII letters converted to uppercase.

**Example:**
```kryos
println(to_upper("hello"))   // HELLO
println(to_upper("Hello!"))  // HELLO!
```

**Note:** Only ASCII letters (a-z) are converted. Non-ASCII characters are passed through unchanged.

**See also:** `to_lower`

---

### to_lower

`to_lower(s: str) -> str`

Return a copy of `s` with all ASCII letters converted to lowercase.

**Example:**
```kryos
println(to_lower("HELLO"))   // hello
println(to_lower("Hello!"))  // hello!
```

**Note:** Only ASCII letters (A-Z) are converted. Non-ASCII characters are passed through unchanged.

**See also:** `to_upper`

---

## Whitespace

### trim

`trim(s: str) -> str`

Remove leading and trailing whitespace (spaces, tabs, newlines, carriage returns).

**Example:**
```kryos
println(trim("  hello  "))   // hello
println(trim("\thello\n"))   // hello
```

**Note:** This shadows the core builtin `trim`. The behavior is identical.

**See also:** `trim_start`, `trim_end`

---

### trim_start

`trim_start(s: str) -> str`

Remove leading whitespace only.

**Example:**
```kryos
use std::string

println(trim_start("  hello  "))  // "hello  "
println(trim_start("\thello"))    // "hello"
```

**See also:** `trim`, `trim_end`

---

### trim_end

`trim_end(s: str) -> str`

Remove trailing whitespace only.

**Example:**
```kryos
use std::string

println(trim_end("  hello  "))  // "  hello"
println(trim_end("hello\n"))    // "hello"
```

**See also:** `trim`, `trim_start`

---

## Split and Join

### split

`split(s: str, delimiter: str) -> [str]`

Split `s` into an array of substrings using `delimiter`. Returns all parts including empty strings between adjacent delimiters.

**Example:**
```kryos
use std::string

let parts = split("a,b,c", ",")
println(parts)                  // [a, b, c]

let words = split("one  two", " ")
println(words)                  // [one, , two]
```

**Edge cases:**
- If `delimiter` is empty, splits into individual characters.

**Note:** This shadows the core builtin `split`. Use the core builtin when splitting on spaces with default behavior.

**See also:** `join`

---

### join

`join(parts: [str], separator: str) -> str`

Join an array of strings with `separator` between each element.

**Example:**
```kryos
use std::string

println(join(["a", "b", "c"], ", "))  // a, b, c
println(join(["one", "two"], "-"))    // one-two
println(join([], ","))                // (empty string)
```

**Important:** The argument order is `join(parts, separator)` -- array first, separator second. This is reversed from the core builtin `join(separator, array)`.

**See also:** `split`

---

## Replacement

### replace

`replace(s: str, old_str: str, new_str: str) -> str`

Replace all occurrences of `old_str` with `new_str` in `s`.

**Example:**
```kryos
use std::string

println(replace("hello world", "world", "kryos"))  // hello kryos
println(replace("aaa", "a", "bb"))                 // bbbbbb
```

**Edge cases:**
- If `old_str` is not found, returns the original string unchanged.

**Note:** This shadows the core builtin `replace`. The behavior is identical.

---

## Repetition

### repeat

`repeat(s: str, n: i64) -> str`

Repeat `s` exactly `n` times.

**Example:**
```kryos
use std::string

println(repeat("ha", 3))   // hahaha
println(repeat("-", 40))   // ----------------------------------------
```

**Edge cases:**
- `n <= 0` returns an empty string.

---

## Reversal

### reverse

`reverse(s: str) -> str`

Reverse a string character by character.

**Example:**
```kryos
use std::string

println(reverse("hello"))   // olleh
println(reverse("abcde"))   // edcba
```

---

## Character Access

### chars

`chars(s: str) -> [str]`

Split a string into an array of single-character strings.

**Example:**
```kryos
use std::string

let cs = chars("hello")
println(cs)         // [h, e, l, l, o]
println(cs[0])      // h
```

---

### char_at

`char_at(s: str, i: i64) -> str`

Return the character at zero-based index `i`. Returns an empty string if `i` is out of bounds.

**Example:**
```kryos
use std::string

println(char_at("hello", 0))    // h
println(char_at("hello", 4))    // o
println(char_at("hello", 99))   // (empty string)
```

**Edge cases:**
- Out-of-bounds indices return `""` rather than throwing. This differs from the core builtin `char_at`, which throws on out-of-bounds.

**See also:** `chars`, `substring`

---

### substring

`substring(s: str, start: i64, end: i64) -> str`

Extract bytes from index `start` (inclusive) to `end` (exclusive). Indices are clamped to string bounds.

**Example:**
```kryos
use std::string

println(substring("hello world", 0, 5))   // hello
println(substring("hello world", 6, 11))  // world
println(substring("abcdef", 2, 4))        // cd
```

**Edge cases:**
- Indices below `0` are clamped to `0`.
- Indices beyond `len(s)` are clamped to `len(s)`.
- If `start >= end` after clamping, returns `""`.

**See also:** `char_at`, `find`

---

## Padding

### pad_left

`pad_left(s: str, width: i64, fill: str) -> str`

Pad `s` on the left with `fill` characters to reach `width`. Only the first character of `fill` is used. Defaults to space if `fill` is empty.

**Example:**
```kryos
use std::string

println(pad_left("42", 5, "0"))   // 00042
println(pad_left("hi", 6, " "))   // "    hi"
println(pad_left("hello", 3, " ")) // hello (no truncation)
```

**Edge cases:**
- If `s` is already `width` or longer, it is returned unchanged.

**See also:** `pad_right`

---

### pad_right

`pad_right(s: str, width: i64, fill: str) -> str`

Pad `s` on the right with `fill` characters to reach `width`. Only the first character of `fill` is used. Defaults to space if `fill` is empty.

**Example:**
```kryos
use std::string

println(pad_right("Name", 20, "."))  // "Name................"
println(pad_right("hi", 6, " "))    // "hi    "
```

**Edge cases:**
- If `s` is already `width` or longer, it is returned unchanged.

**See also:** `pad_left`

---

## Formatting

### format

`format(template: str, args: [str]) -> str`

Simple positional template formatting. Replaces `{0}`, `{1}`, `{2}`, ... with the corresponding element from `args`.

**Example:**
```kryos
use std::string

let msg = format("Hello, {0}! You have {1} messages.", ["Alice", "5"])
println(msg)  // Hello, Alice! You have 5 messages.

let path = format("{0}/{1}.{2}", ["src", "main", "kry"])
println(path)  // src/main.kry
```

**Edge cases:**
- Placeholders with no matching index are left in the output unchanged.
- Args are strings; convert numbers with `to_string` before passing.
- Replacement is applied left-to-right; earlier args cannot reference later ones.

---

## Core Builtin Overlap

Several `std::string` functions have the same name as core builtins. After `use std::string`, the module version takes precedence. The behavior is intentionally identical for `contains`, `starts_with`, `ends_with`, `trim`, `split`, and `replace`. Use the core builtins directly (without importing `std::string`) if you prefer the shorter call form.

One important difference: `std::string.join(parts, separator)` takes the array **first**, whereas the core builtin `join(separator, array)` takes the separator first.
