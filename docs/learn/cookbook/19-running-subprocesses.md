# Cookbook 19 · Running subprocesses

`std::process::command` shells out to a command, captures stdout, stderr, and exit code, and returns a `CommandResult` struct.

## The program

```kryos
use std::process::{command}
use std::string::{split_lines}

@capabilities(io, process)
fn main() {
    // Quick git status check.
    let result = command("git").arg("status").arg("--short").run()

    if !result.success {
        println("git failed with exit " + to_string(result.exit_code))
        if len(result.stderr) > 0 {
            println("stderr: " + result.stderr)
        }
        return
    }

    let changed = split_lines(result.stdout)
    let mut count: i64 = 0
    for line in changed {
        if len(line) > 0 { count = count + 1 }
    }
    println("changed files: " + to_string(count))
}
```

## CommandResult fields

- `exit_code: i64` — the process exit code; `0` on success
- `success: bool` — `true` when `exit_code == 0`
- `stdout: str` — captured stdout
- `stderr: str` — captured stderr

## Things to know

- Chain `.arg("...")` calls to add arguments; never join args with spaces into a single string (no shell expansion).
- The subprocess inherits no stdin. For piping-in, pass stdin via `.stdin_input("data")` before `.run()`.
- Captures are unbounded — don't run commands that output gigabytes.
- Capability required: `@capabilities(process)`.
