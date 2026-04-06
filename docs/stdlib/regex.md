# std::regex

Regular expression support. Pattern syntax follows Perl-compatible regular expressions (PCRE).

```kryos
import std::regex
```

---

### regex_match

`regex_match(pattern: String, text: String) -> Map | Nil`

Match a pattern at the **start** of the text. Returns a match map or `nil` if no match.

The match map contains:
- `matched` -- the matched substring
- `groups` -- array of capture group values
- `start` -- start index of the match
- `end` -- end index of the match

**Example:**
```kryos
let m = regex_match("([0-9]+)-([0-9]+)", "123-456 rest")
print(m.matched)   // 123-456
print(m.groups)    // ["123", "456"]
print(m.start)     // 0
print(m.end)       // 7
```

```kryos
let m = regex_match("^hello", "goodbye")
print(m)  // nil
```

**Edge cases:**
- Only matches at the start of the string. Use `regex_search` for anywhere-in-string matching.

**See also:** regex_search, regex_test

---

### regex_search

`regex_search(pattern: String, text: String) -> Map | Nil`

Search for the first occurrence of a pattern anywhere in the text. Returns the same match map as `regex_match`, or `nil`.

**Example:**
```kryos
let m = regex_search("[0-9]+", "abc 42 def")
print(m.matched)  // 42
print(m.start)    // 4
```

**See also:** regex_match, regex_find_all

---

### regex_find_all

`regex_find_all(pattern: String, text: String) -> Array`

Find all non-overlapping matches. Returns an array of matched strings.

**Example:**
```kryos
let nums = regex_find_all("[0-9]+", "port 8080, timeout 30, retries 3")
print(nums)  // ["8080", "30", "3"]
```

```kryos
let words = regex_find_all("[A-Z][a-z]+", "HelloWorld FooBar")
print(words)  // ["Hello", "World", "Foo", "Bar"]
```

**Edge cases:**
- Returns an empty array if no matches are found.
- If the pattern contains capture groups, returns the group contents instead of the full match.

**See also:** regex_search

---

### regex_replace

`regex_replace(pattern: String, replacement: String, text: String) -> String`
`regex_replace(pattern: String, replacement: String, text: String, max: Int) -> String`

Replace all matches of a pattern. Optional fourth argument limits the number of replacements.

**Example:**
```kryos
let result = regex_replace("[0-9]+", "X", "abc 1 def 2 ghi 3")
print(result)  // abc X def X ghi X
```

```kryos
// Replace only the first match
let result = regex_replace("[0-9]+", "X", "abc 1 def 2 ghi 3", 1)
print(result)  // abc X def 2 ghi 3
```

**Edge cases:**
- `max = 0` means replace all (the default).

**See also:** regex_split

---

### regex_split

`regex_split(pattern: String, text: String) -> Array`
`regex_split(pattern: String, text: String, max: Int) -> Array`

Split text by a regex pattern. Optional third argument limits the number of splits.

**Example:**
```kryos
let parts = regex_split("[,;\\s]+", "a, b; c  d")
print(parts)  // ["a", "b", "c", "d"]
```

```kryos
let parts = regex_split(":", "a:b:c:d", 2)
print(parts)  // ["a", "b", "c:d"]
```

**Edge cases:**
- `max = 0` means split all (the default).

**See also:** regex_replace

---

### regex_test

`regex_test(pattern: String, text: String) -> Bool`

Test whether a pattern matches anywhere in the text. Returns a boolean.

**Example:**
```kryos
if regex_test("[0-9]", user_input) {
    print("Contains a digit")
}
```

```kryos
let valid = regex_test("^[a-zA-Z0-9_]+$", username)
```

**See also:** regex_match, regex_search

---

### regex_escape

`regex_escape(text: String) -> String`

Escape all special regex characters in a string so it can be used as a literal pattern.

**Example:**
```kryos
let safe = regex_escape("price: $9.99 (USD)")
print(safe)  // price:\ \$9\.99\ \(USD\)
let found = regex_test(safe, "the price: $9.99 (USD) is final")
print(found)  // true
```

**See also:** regex_replace
