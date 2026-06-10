# Cookbook 01 · CLI tool

Build a command-line word counter that reads a file and prints line, word, and character counts. Like `wc`, but yours.

## The program

Save as `wc.kry`:

```kryos
fn count_words(line: str) -> i64 {
    let mut count = 0
    let mut in_word = false
    let space = char_code(" ")
    let tab = char_code("\t")
    let newline = char_code("\n")
    for c in line {
        let is_space = c == space or c == tab or c == newline
        if !is_space and !in_word {
            count = count + 1
            in_word = true
        } elif is_space {
            in_word = false
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
- **`split(s, delim)`** splits a string into an array of substrings.
- **Iterating a `str`** yields each character as an `i64` char code; compare with `char_code(" ")` etc.

## Variations to try

- Add a `-l` flag that only prints lines.
- Accept multiple files: loop over `args[1..]`, print totals.
- Print results as JSON: build a string with `{ \"lines\": ... }`.

When you're ready for more, see [02 · HTTP server](./02-http-server.md).
