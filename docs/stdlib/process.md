# std::process

Process execution, timing, and control. Provides shell command execution for tool integration, CLI argument access, and spawn synchronization.

```kryos
import std::process
```

---

### exec

`exec(command: String) -> Map`

Execute a shell command. Returns a map with `stdout`, `stderr`, and `exit_code` fields. Default timeout is 30 seconds.

**Example:**
```kryos
let result = exec("echo hello")
print(result.stdout)     // hello\n
print(result.exit_code)  // 0
```

```kryos
let result = exec("ls nonexistent")
if result.exit_code != 0 {
    print("Error: " + result.stderr)
}
```

**Edge cases:**
- If the command times out (30s), returns `exit_code: -1` and `stderr: "Command timed out"`.
- The command runs in a shell (`sh -c` on Unix, `cmd /c` on Windows).

**See also:** exec_capture, exec_timeout

---

### exec_capture

`exec_capture(command: String) -> String`

Execute a shell command and return its stdout as a string. Raises on non-zero exit or timeout.

**Example:**
```kryos
let version = exec_capture("python --version")
print(version)  // Python 3.12.0
```

**Edge cases:**
- Raises a runtime error if the command exits with a non-zero code (includes stderr in the message).
- Raises on timeout (30s default).
- Use `exec` instead if you need to handle errors without exceptions.

**See also:** exec

---

### exec_timeout

`exec_timeout(command: String, seconds: Float) -> Map`

Execute a shell command with a custom timeout. Returns the same map structure as `exec`.

**Example:**
```kryos
let result = exec_timeout("ping -c 3 localhost", 10)
print(result.stdout)
```

```kryos
// Kill long-running processes after 2 seconds
let result = exec_timeout("sleep 60", 2)
print(result.exit_code)  // -1 (timed out)
```

**See also:** exec

---

### sleep

`sleep(seconds: Float) -> Nil`

Pause execution for the given number of seconds.

**Example:**
```kryos
print("Starting...")
sleep(1.5)
print("Done.")
```

---

### args

`args() -> Array`

Return command-line arguments passed after the script filename.

**Example:**
```kryos
// Run: kryos script.kry hello world
let argv = args()
print(argv)  // ["hello", "world"]
```

**Edge cases:**
- Returns an empty array if no arguments were passed.

---

### exit

`exit() -> Nil`
`exit(code: Int) -> Nil`

Exit the program with an optional exit code. Default is 0.

**Example:**
```kryos
if !valid {
    print("Invalid input")
    exit(1)
}
```

---

### wait_all

`wait_all() -> Nil`
`wait_all(handle1: Thread, handle2: Thread, ...) -> Nil`

Wait for spawned threads to complete. With no arguments, waits for all threads tracked by the interpreter. With arguments, waits only for the specified handles.

**Example:**
```kryos
spawn { sleep(1); print("task 1 done") }
spawn { sleep(2); print("task 2 done") }
wait_all()
print("all tasks complete")
```
