# Cookbook 10 · Read structured logs

You're staring at a million-line `app.log` of JSONL records. You want to group by `level` and print counts. Kryos's stdlib has everything you need: `file_read`, `split_lines`, `std::json`.

## The input

`app.log` (one JSON object per line):

```jsonl
{"ts": 1716057600, "level": "info",  "msg": "boot"}
{"ts": 1716057601, "level": "warn",  "msg": "slow disk"}
{"ts": 1716057602, "level": "error", "msg": "out of memory"}
{"ts": 1716057603, "level": "info",  "msg": "recovered"}
{"ts": 1716057604, "level": "warn",  "msg": "slow disk"}
```

## The program

```kryos
use std::json::{parse, get, to_str, to_int}
use std::string::{split_lines}

@capabilities(io)
fn main() {
    let body = file_read("app.log")
    let lines = split_lines(body)

    let mut info_count: i64 = 0
    let mut warn_count: i64 = 0
    let mut error_count: i64 = 0
    let mut other_count: i64 = 0

    let mut earliest: i64 = 0
    let mut latest: i64 = 0
    let mut first = true

    for line in lines {
        if len(line) == 0 { continue }
        let rec = parse(line)
        let level = to_str(get(rec, "level"))
        let ts = to_int(get(rec, "ts"))

        if first {
            earliest = ts
            latest = ts
            first = false
        } else {
            if ts < earliest { earliest = ts }
            if ts > latest { latest = ts }
        }

        if level == "info" {
            info_count = info_count + 1
        } elif level == "warn" {
            warn_count = warn_count + 1
        } elif level == "error" {
            error_count = error_count + 1
        } else {
            other_count = other_count + 1
        }
    }

    println("=== Log summary ===")
    println("info:   " + to_string(info_count))
    println("warn:   " + to_string(warn_count))
    println("error:  " + to_string(error_count))
    if other_count > 0 {
        println("other:  " + to_string(other_count))
    }
    println("span:   " + to_string(latest - earliest) + " seconds")
}
```

## Run it

```bash
kryos run app.log.kry
# === Log summary ===
# info:   2
# warn:   2
# error:  1
# span:   4 seconds
```

## Things to know

- `split_lines` handles both `\n` and `\r\n` — safe across platforms.
- `parse` throws on a malformed line; wrap the call in `try`/`catch` if your input may contain bad lines you want to skip rather than abort on.
- Counts are kept in `let mut` locals — no need for a `map<str, i64>` for the four well-known levels. For arbitrary user-defined levels, switch to `map<str, i64>` and use `m["new_level"] = (m["new_level"] + 1)`.
- For 100M-line files, `file_read` + `split_lines` loads the whole file into memory at once; switch to `std::io::open` + `buf_reader(file)` and loop `reader.read_line()` until `reader.is_eof()` if you hit memory pressure.
