# 08 · Strings & Text

After this chapter you will know what a `str` actually counts when you call
`len` on it, how to put a literal `{` in an interpolated string without the
parser mistaking it for an interpolation, why two of the string builtins
quietly use a completely different encoding model than the rest of the
language, and how to build a string in a loop without an accidental O(n²).

## The UTF-8 model: `len` counts bytes, not characters

A `str` is owned, heap-allocated, UTF-8 text. `len(s)` returns the **byte**
count -- for plain ASCII that's the same number as the character count, but
the moment a string holds a multibyte character, it isn't:

```kryos
fn main() {
    let ascii: str = "hi"
    let accented: str = "café"
    println("ascii len: " + to_string(len(ascii)))
    println("accented len: " + to_string(len(accented)))
}
```

Output:

```
ascii len: 2
accented len: 5
```

`"café"` is four *characters* but five *bytes* -- `é` alone takes two bytes
in UTF-8. If you need the character count instead, `std::utf8::codepoint_count`
walks the string decoding each multibyte sequence rather than just
returning the byte length:

```kryos
use std::utf8::{codepoint_count}

fn main() {
    let s: str = "café"
    println("len (bytes): " + to_string(len(s)))
    println("codepoint_count: " + to_string(codepoint_count(s)))
}
```

Output:

```
len (bytes): 5
codepoint_count: 4
```

Reach for `len` when you're about to do byte-level work (`substr`,
network/file I/O sizes); reach for `codepoint_count` when you're counting
what a human would call "characters."

## Interpolation and literal braces

Every string literal interpolates -- there is no separate `f"..."` or
`format!` form the way Python or Rust has. `{name}` inside *any* string
literal splices in that variable's value:

```kryos
fn main() {
    let name: str = "Kryos"
    let version: i64 = 9
    println("hello, {name}! version 0.{version}.0")
}
```

Output:

```
hello, Kryos! version 0.9.0
```

Because every string interpolates, a **literal** brace needs escaping --
doubling it (`{{`, `}}`) is the idiom:

```kryos
fn main() {
    println("literal braces: {{not interpolated}}")
}
```

Output:

```
literal braces: {not interpolated}
```

This bites JSON and set-notation strings the hardest, since both use `{`/`}`
constantly: `"{\"a\":1}"` fails to parse (the first `{` opens an
interpolation the lexer then can't close sensibly) -- write
`"{{\"a\":1}}"`, or skip the escaping entirely and build the string with
`+` (`"{" + "\"a\":1" + "}"`).

## The byte-buffer caveat: two builtins that aren't UTF-8-aware

`base64_encode`, `chr`, and `byte_at` use a **latin-1 byte-buffer model** --
each codepoint is treated as exactly one byte (0-255) -- not the UTF-8
model the rest of the language uses. This is fine for text that never
leaves the ASCII/Latin-1 range, and silently wrong the moment it doesn't.
Watch what happens encoding a string containing `€` (codepoint 8364, far
outside one byte) and decoding it back:

```kryos
fn main() {
    let euro: str = "caf€"
    let encoded: str = base64_encode(euro)
    let decoded: str = base64_decode(encoded)
    println("decoded: " + decoded)
    println("decoded len: " + to_string(len(decoded)))
}
```

Output:

```
decoded: caf¬
decoded len: 5
```

`€` got truncated to its low byte (`0xAC`) before encoding -- there is no
error, no panic, just a different character on the way out (`¬`, `U+00AC`)
than the one that went in. And `decoded`'s `len` reports `5`, not `4`,
even though it's "the same four characters" as the input: the truncated
`0xAC` byte gets reinterpreted as the *codepoint* `U+00AC` on the way back
into a normal `str`, and `U+00AC` itself needs two bytes once it's stored
as legitimate UTF-8 -- so `len` genuinely overcounts a byte-buffer string
that holds anything at or above `0x80`. If you're using `base64_encode`/
`chr`/`byte_at` on genuine binary data (0-255 byte values, not human text),
this is exactly the model you want; if you're using it on arbitrary UTF-8
text, encode a `str` you already know is ASCII-only, or reach for
`std::bytes` (a real codepoint-indexed byte type) instead.

`byte_at` compounds the confusion by name alone: despite the name, it does
**not** index raw bytes -- it returns the Unicode **codepoint** of the
*i*-th **character** (not the *i*-th byte):

```kryos
fn main() {
    let s: str = "café"
    println("byte_at(s, 3): " + to_string(byte_at(s, 3)))
    println("char_code(\"é\"): " + to_string(char_code("é")))
}
```

Output:

```
byte_at(s, 3): 233
char_code("é"): 233
```

Index `3` is the fourth *character* (`é`), and `byte_at` returns its
codepoint (`233`) -- the same value `char_code` gives you for `"é"` on its
own. For a genuine latin-1 byte buffer (every codepoint already in `0-255`
by construction), codepoint and byte coincide and `byte_at` behaves like
its name suggests; for ordinary multibyte text, it doesn't, and there is
no raw-byte accessor for a multibyte string at all.

## Building a string efficiently: `string_builder`

`s = s + chunk` in a loop works, but each `+` allocates a brand-new string
and copies everything accumulated so far into it -- O(n²) total work for n
appends:

```kryos
fn main() {
    let mut s: str = ""
    let mut i: i64 = 0
    while i < 5 {
        s = s + "[" + to_string(i) + "]"
        i = i + 1
    }
    println(s)
}
```

Output:

```
[0][1][2][3][4]
```

Fine at five iterations, expensive at five hundred thousand.
`std::string::string_builder()` accumulates into one growable buffer and
materializes the result once, at the end, with `build()` -- O(n) total:

```kryos
use std::string::{string_builder, StringBuilder}

fn main() {
    let sb: StringBuilder = string_builder()
    let mut i: i64 = 0
    while i < 5 {
        sb.append("[" + to_string(i) + "]")
        i = i + 1
    }
    let result: str = sb.build()
    println(result)
}
```

Output:

```
[0][1][2][3][4]
```

`append` mutates the builder's internal buffer and returns `self`, so calls
chain (`sb.append("a").append("b")`); call `build()` exactly once -- the
builder frees its buffer when it materializes the final string, so a
second `build()` on the same builder is a safe no-op that returns `""`,
not a use-after-free. Reach for `string_builder` the moment "loop that
concatenates" describes what you're writing, not after you've measured it
being slow.

## `substr` can split a codepoint -- and later operations panic on it

`substr(s, start, end)` is **byte-indexed**, not codepoint-indexed. Ordinary
arithmetic on a multibyte string can land you in the middle of a
multi-byte character, producing a `str` that holds genuinely invalid UTF-8:

```kryos
fn main() {
    let s: str = "café"
    let bad: str = substr(s, 0, 4)
    println(to_string(contains(bad, "x")))
}
```

Output:

```
kryos panic: string operation requires valid UTF-8, but the string contains invalid byte sequences (a substr()/byte_at() call likely split a multibyte character mid-codepoint) -- use std::utf8::is_valid(s) to check first
stack trace (most recent call last):
  0: main() at mistake.kry:1
```

`"café"` is 5 bytes (`c`, `a`, `f`, then `é`'s two bytes); byte offset `4`
lands on the *second* byte of `é`, so `substr(s, 0, 4)` returns 4 bytes
that are not valid UTF-8 on their own. The string itself is constructed
without complaint -- the panic happens the moment a text-aware operation
(`contains`, `trim`, `to_upper`, `to_lower`, `replace`, `split`, `join`, and
their `trim_start`/`trim_end` cousins) tries to actually decode it, with
exit code `98`. Check `std::utf8::is_valid(s)` before slicing at an
arithmetic byte offset you haven't proven lands on a character boundary,
or slice by codepoint count instead of raw byte math when the source might
contain multibyte characters.

## Common mistakes

**Assuming `contains`/`byte_at`/`base64_encode` all share one model.**
They don't: `contains` and `substr` operate on UTF-8 byte offsets, while
`base64_encode`/`chr`/`byte_at` operate on a latin-1 byte-per-codepoint
model. Mixing text through both without knowing which is which is exactly
how the `€` example above silently corrupts.

**Reaching for `+` in a hot loop instead of `string_builder`.** Both
compile and run identically for small inputs -- the difference only shows
up as the iteration count grows, which is exactly what makes it easy to
ship. If a loop's body concatenates onto an accumulator string, default to
`string_builder` from the start.

## Exercises

1. Write a function that takes a `[str]` of words and joins them with
   `", "` using `string_builder` (skip the separator before the first
   word). Confirm the output against `["a", "b", "c"]` -> `"a, b, c"`.
2. Take the `€` round-trip example above and replace `base64_encode`/
   `base64_decode` with plain `+` concatenation and printing -- confirm
   the character survives when you're not going through the byte-buffer
   functions at all.
3. Write a string containing at least one accented character, call
   `codepoint_count` and `len` on it, and predict which one is larger
   before running it.
4. Deliberately construct a `substr` call that lands mid-codepoint on a
   string of your choice, call `std::utf8::is_valid` on the result before
   doing anything else with it, and confirm it reports `false`.

## Summary

- `len(s)` counts UTF-8 **bytes**; `std::utf8::codepoint_count(s)` counts
  characters -- they diverge the moment a string holds anything outside
  ASCII.
- Every string literal interpolates (`"{name}"`); a literal brace needs
  doubling (`{{`, `}}`) or it's read as the start of an interpolation.
- `base64_encode`/`chr`/`byte_at` use a **latin-1 byte-per-codepoint**
  model, not UTF-8 -- encoding text with a codepoint above `0xFF` silently
  truncates it, and `len` overcounts the truncated result once it's back
  in a normal `str`.
- `byte_at(s, i)` returns the codepoint of the *i*-th **character**, not
  the *i*-th raw byte, despite the name.
- `string_builder()` / `.append()` / `.build()` accumulate in O(n) total
  work; a `s = s + chunk` loop is O(n²) and only looks fine at small sizes.
- `substr` is byte-indexed and can split a multibyte character; a
  downstream text operation on the result panics with exit code `98` --
  check `std::utf8::is_valid` first if the slice offset isn't provably on
  a character boundary.

Next: [Generics & traits](09-generics-and-traits.md)
