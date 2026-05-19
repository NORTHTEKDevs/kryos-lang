# Cookbook 19 · Running subprocesses

`std::cmd::cmd_run` shells out to a command, captures stdout, stderr, and exit code, and returns the bundle as a single string for easy parsing.

## The program

```kryos
use std::cmd::cmd_run

@capabilities(io, process)
fn main() {
    // Quick git status check.
    let bundle = cmd_run("git status --short")
    let lines = split_lines(bundle)
    if len(lines) < 2 {
        println("(unexpected bundle format)")
        return
    }
    let exit_code = parse_int(lines[0])
    let stderr_len = parse_int(lines[1])

    if exit_code != 0 {
        println("git failed with exit " + to_string(exit_code))
        // stderr is in the next `stderr_len` bytes after the second \n.
        return
    }

    // stdout starts after the header + stderr_len bytes.
    let header_len = len(lines[0]) + 1 + len(lines[1]) + 1
    let body_start = header_len + stderr_len
    let stdout_str = substr(bundle, body_start, len(bundle))

    let changed = split_lines(stdout_str)
    let mut count: i64 = 0
    for line in changed {
        if len(line) > 0 { count = count + 1 }
    }
    println("changed files: " + to_string(count))
}
```

## Bundle format

`cmd_run` returns a single string:

```
<exit_code>\n<stderr_byte_count>\n<stderr_bytes><stdout_bytes>
```

- `exit_code` — decimal, `-1` if the process couldn't be waited on
- `stderr_byte_count` — decimal, the length of the stderr block
- stderr — the captured stderr (may be empty)
- stdout — the captured stdout (may be empty)

## Things to know

- Argument splitting is shellword-style: respects `"..."` and `'...'` but no escape sequences. For complex commands write your own `Command` via FFI.
- The subprocess inherits no stdin (closed via `Stdio::null`). For piping-in, use the lower-level `std::process` module.
- Captures are unbounded — don't `cmd_run` something that outputs gigabytes. For streaming, use `std::process::Command` directly.
- Capability required: `@capabilities(process)`.
