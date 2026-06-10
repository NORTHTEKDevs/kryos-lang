# Cookbook 15 · CSV parsing

CSV is the most common interop format you'll touch. Kryos doesn't ship a CSV crate; you don't need one for ~95% of real CSVs.

## The program

```kryos
use std::string::{split_lines}

@capabilities(io)
fn main() {
    let raw = file_read("data.csv")
    let lines = split_lines(raw)
    if len(lines) == 0 { return }

    let header = parse_csv_line(lines[0])
    let mut i: i64 = 1
    while i < len(lines) {
        if len(lines[i]) == 0 {
            i = i + 1
            continue
        }
        let row = parse_csv_line(lines[i])
        print_row(header, row)
        i = i + 1
    }
}

fn parse_csv_line(line: str) -> [str] {
    let mut out: [str] = []
    let mut field: str = ""
    let mut in_quote: bool = false
    let n = len(line)
    let mut i: i64 = 0
    while i < n {
        let c = char_code(substr(line, i, i + 1))
        if c == 34 {  // " — quote
            in_quote = !in_quote
        } elif c == 44 and !in_quote {  // ,
            out = push(out, field)
            field = ""
        } else {
            field = field + substr(line, i, i + 1)
        }
        i = i + 1
    }
    out = push(out, field)
    return out
}

fn print_row(header: [str], row: [str]) {
    let n = len(row)
    let mut i: i64 = 0
    let mut line: str = "{{ "
    while i < n {
        let mut key: str = "field_" + to_string(i)
        if i < len(header) { key = header[i] }
        if i > 0 { line = line + ", " }
        line = line + key + ": " + row[i]
        i = i + 1
    }
    line = line + " }}"
    println(line)
}
```

## Things to know

- Handles double-quoted fields (commas inside quotes are not delimiters).
- Does **not** handle embedded newlines inside quoted fields. For that,
  buffer until quotes balance.
- Does **not** handle escaped `""` inside quoted fields — strip those in
  a follow-up pass if needed.
- For multi-MB files, switch to a streaming approach (`std::stream`) to
  avoid loading the whole file into a single `str`.
