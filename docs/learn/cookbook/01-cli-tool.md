# Cookbook 01 · CLI tool

Build a command-line word counter that reads a file and prints line, word, and character counts. Like `wc`, but yours.

## The program

Save as `wc.kry`:

```kryos
use std::string::{replace}

fn count_words(line: str) -> i64 {
    // Strings can't be iterated char-by-char (`for c in line` is a compile
    // error) -- split into an array of tokens first.
    let normalized = replace(line, "\t", " ")
    let mut count = 0
    for w in split(normalized, " ") {
        if len(w) > 0 {
            count = count + 1
        }
    }
    return count
}

@capabilities(io)
fn main() {
    let argv = args()
    if len(argv) < 2 {
        println("usage: wc <file>")
        return
    }

    let path = argv[1]
    let body = file_read(path)

    let mut lines = 0
    let mut words = 0
    let chars = len(body)

    for line in split(body, "\n") {
        lines = lines + 1
        words = words + count_words(line)
    }

    println(to_string(lines) + " lines  "
          + to_string(words) + " words  "
          + to_string(chars) + " chars  "
          + path)
}
```

## Run it

```bash
kryos run wc.kry README.md
# → 323 lines  1842 words  12362 chars  README.md
```

## What this teaches

- **`args()`** returns the argv array (program name at `args[0]`)
- **`file_read(path)`** reads a file into a `str`. Requires the `io` capability — note `@capabilities(io)` on `main`.
- **`split(s, delim)`** splits a string into an array of substrings — this is the global builtin form (`std::path` and `std::string` also each have their own `split` with a different signature; the flat namespace resolves the unqualified call to the builtin unless you import one of those).
- **You cannot iterate a `str` directly.** `for c in line` is a compile error (`E0110`) — `for` only accepts an array or a range. Split the string into an array first (`split(s, " ")`, `std::string::split_lines`) or index characters with `substr(s, i, i + 1)`.

## Variations to try

- Add a `-l` flag that only prints lines.
- Accept multiple files: loop over `args[1..]`, print totals.
- Print results as JSON: build a string with `{ \"lines\": ... }`.

When you're ready for more, see [02 · HTTP server](./02-http-server.md).
