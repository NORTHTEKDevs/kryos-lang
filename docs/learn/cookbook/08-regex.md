# Cookbook 08 · Regex everywhere

`std::re` wraps Rust's `regex` crate (PCRE-like, no backtracking, linear-time). It supports the usual: literal text, character classes, quantifiers, groups, anchors. The pattern is: compile once at startup, reuse the handle.

## The program

```kryos
use std::re::{compile, is_match, find, find_all, replace_all}

@capabilities(io)
fn main() {
    let log = "2026-05-18 12:34:56 INFO Booted\n" +
              "2026-05-18 12:34:57 WARN Disk usage 91%\n" +
              "2026-05-18 12:35:00 ERROR Out of space"

    // 1. Is there at least one ERROR line?
    if is_match("\\bERROR\\b", log) {
        println("found at least one ERROR")
    }

    // 2. Replace all whitespace runs with a single space.
    let normalized = replace_all("\\s+", log, " ")
    println("normalized: " + normalized)

    // 3. Extract every log-level keyword from the log.
    let matches = find_all("INFO|WARN|ERROR", log)
    for m in matches {
        println("level: " + m.text + " at byte " + to_string(m.start))
    }

    // 4. Compile-and-reuse for repeated matching.
    // Braces are doubled ({{4}} / {{2}}) because every Kryos string
    // interpolates -- a bare {4} would be parsed as an interpolation and
    // silently strip the quantifier, leaving a pattern that matches nothing.
    let re = compile("\\d{{4}}-\\d{{2}}-\\d{{2}}")
    let dates = re.find_all(log)
    for d in dates {
        println("date: " + d.text)
    }
    let _ = re.drop()
}
```

## Run it

```bash
kryos run regex.kry
```

Output:

```
found at least one ERROR
normalized: 2026-05-18 12:34:56 INFO Booted 2026-05-18 12:34:57 WARN Disk usage 91% 2026-05-18 12:35:00 ERROR Out of space
level: INFO at byte 20
level: WARN at byte 52
level: ERROR at byte 92
date: 2026-05-18
date: 2026-05-18
date: 2026-05-18
```

## Things to know

- The `regex` crate has no lookbehind / lookahead — patterns must be expressible as a linear-time DFA. If you find yourself wanting `(?<=...)`, restructure with a capture group instead.
- Backslashes need escaping in Kryos string literals: `"\\d"` becomes `\d` in the compiled regex.
- A literal `{` in a pattern needs doubling too, same as any other Kryos string: `\d{{4}}` compiles to the regex `\d{4}`. Forgetting this doesn't error — the bare `{4}` is silently consumed as a (no-op) string interpolation, leaving a pattern that quietly matches something else (or nothing).
- Always call `.drop()` on a compiled `Regex` when you're done — `re.drop()` (method), not a free function. Compiled regex objects are heap-allocated on the native side.
- The module-level `is_match(pattern, text)` is much cheaper than `find`/`find_all` — use it when you only care whether a match exists. Once you have a compiled `Regex` (from `compile(pattern)`), the equivalent is the `re.is_match(text)` method.
- `captures(pattern, text)` (or `re.captures(text)`) returns a `Captures { groups: [str], count: i32, found: bool }`. `groups[0]` is the whole match, `groups[1..]` are the explicit parenthesized groups in order, and `count` is the number of explicit groups (excluding the whole match). A group that didn't participate in the match comes back as `""`, not a sentinel start offset — there is no separate span-only capture API.
