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
    let re = compile("\\d{4}-\\d{2}-\\d{2}")
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
2026-05-18 12:34:56 — INFO
2026-05-18 12:34:57 — WARN
2026-05-18 12:35:00 — ERROR
```

## Things to know

- The `regex` crate has no lookbehind / lookahead — patterns must be expressible as a linear-time DFA. If you find yourself wanting `(?<=...)`, restructure with a capture group instead.
- Backslashes need escaping in Kryos string literals: `"\\d"` becomes `\d` in the compiled regex.
- Always `regex_drop` the handle when you're done. Compiled regex objects are heap-allocated.
- `regex_is_match` is much cheaper than `regex_find` — use it when you only care whether a match exists.
- `regex_capture(re, text, 0)` returns the whole-match span; `1..N` return explicit groups. A group that didn't participate in the match returns `start = -1`.
