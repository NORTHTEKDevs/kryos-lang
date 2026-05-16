# Cookbook 01 · CLI tool

Build a command-line word counter that reads a file and prints line, word, and character counts. Like `wc`, but yours.

## The program

Save as `wc.kry`:

```kryos
fn count_words(line: str) -> i64 {
    let mut count = 0
    let mut in_word = false
    for c in line {
        let is_space = c == ' ' or c == '\t' or c == '\n'
        if !is_space and !in_word {
            count = count + 1
            in_word = true
        } else if is_space {
            in_word = false
        }
    }
    count
}

@capabilities(io)
fn main() {
    let args = process_args()
    if len(args) < 2 {
        println("usage: wc <file>")
        return
    }

    let path = args[1]
    let body = file_read(path)

    let mut lines = 0
    let mut words = 0
    let chars = len(body)

    for line in str_split(body, "\n") {
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

- **`process_args()`** returns the argv array (program name at `args[0]`)
- **`file_read(path)`** reads a file into a `str`. Requires the `io` capability — note `@capabilities(io)` on `main`.
- **`str_split(s, delim)`** splits a string into an array of substrings.
- **Iterating a `str`** yields each character.

## Variations to try

- Add a `-l` flag that only prints lines.
- Accept multiple files: loop over `args[1..]`, print totals.
- Print results as JSON: build a string with `{ \"lines\": ... }`.

When you're ready for more, see [02 · HTTP server](./02-http-server.md).
