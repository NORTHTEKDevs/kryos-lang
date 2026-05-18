# Cookbook 08 · Regex everywhere

`std::re` wraps Rust's `regex` crate (PCRE-like, no backtracking, linear-time). It supports the usual: literal text, character classes, quantifiers, groups, anchors. The pattern is: compile once at startup, reuse the handle.

## The program

```kryos
use std::re::{
    regex_new,
    regex_is_match,
    regex_find,
    regex_replace_all,
    regex_capture,
    regex_capture_count,
    regex_drop,
}

@capabilities(io)
fn main() {
    let log = "2026-05-18 12:34:56 INFO Booted\n" +
              "2026-05-18 12:34:57 WARN Disk usage 91%\n" +
              "2026-05-18 12:35:00 ERROR Out of space"

    // 1. Is there at least one ERROR line?
    let err_re = regex_new("\\bERROR\\b")
    if regex_is_match(err_re, log) == 1 {
        println("found at least one ERROR")
    }

    // 2. Replace all whitespace runs with a single space.
    let ws = regex_new("\\s+")
    let normalized = regex_replace_all(ws, log, " ")
    println("normalized: " + normalized)

    // 3. Extract every (date, time, level) tuple.
    let entry = regex_new("(\\d{4}-\\d{2}-\\d{2}) (\\d{2}:\\d{2}:\\d{2}) (INFO|WARN|ERROR)")
    let mut cursor = 0
    while cursor < len(log) {
        let rest = substr(log, cursor, len(log))
        let date_span = regex_capture(entry, rest, 1)
        if date_span.start < 0 { break }
        let time_span = regex_capture(entry, rest, 2)
        let lvl_span  = regex_capture(entry, rest, 3)

        let date = substr(rest, date_span.start, date_span.start + date_span.len)
        let tm   = substr(rest, time_span.start, time_span.start + time_span.len)
        let lvl  = substr(rest, lvl_span.start, lvl_span.start + lvl_span.len)
        println(date + " " + tm + " — " + lvl)

        // Advance past the whole-match end.
        let whole = regex_find(entry, rest)
        cursor = cursor + whole.start + whole.len
    }

    regex_drop(err_re)
    regex_drop(ws)
    regex_drop(entry)
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
