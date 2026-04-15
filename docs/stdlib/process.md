# std::process

Environment variables, command-line arguments, process exit, and subprocess execution.

```kryos
use std::process
```

---

## Types

### Args

The return type of `args()` and `parse_args()`. Provides structured access to parsed command-line arguments.

### Command

The return type of `command()`. Represents a subprocess to be configured and run.

---

## Environment Variables

### env_get

`env_get(name: str) -> str`

Return the value of the environment variable `name`. Throws if the variable is not set.

**Example:**
```kryos
use std::process

let home = env_get("HOME")
println(home)   // e.g. /home/alice
```

**See also:** `env_get_or`, `env_has`

---

### env_get_or

`env_get_or(name: str, default: str) -> str`

Return the value of environment variable `name`, or `default` if it is not set.

**Example:**
```kryos
use std::process

let port = env_get_or("PORT", "3000")
println(port)   // uses PORT if set, otherwise "3000"

let level = env_get_or("LOG_LEVEL", "info")
```

---

### env_has

`env_has(name: str) -> bool`

Return `true` if the environment variable `name` is set.

**Example:**
```kryos
use std::process

if env_has("DEBUG") {
    println("debug mode enabled")
}
```

---

## Process Exit

### exit

`exit(code: i32) -> !`

Terminate the process with the given exit code. Never returns.

**Example:**
```kryos
use std::process

if !env_has("API_KEY") {
    println("error: API_KEY is required")
    exit(1)
}
```

---

### exit_ok

`exit_ok() -> !`

Terminate the process with exit code `0` (success). Equivalent to `exit(0)`.

**Example:**
```kryos
use std::process

println("done")
exit_ok()
```

---

### exit_error

`exit_error(message: str) -> !`

Print `message` to stderr and terminate with exit code `1`.

**Example:**
```kryos
use std::process

exit_error("fatal: configuration file not found")
// prints to stderr and exits with code 1
```

---

## Command-Line Arguments

### argc

`argc() -> i64`

Return the number of command-line arguments, including the program name at index 0.

**Example:**
```kryos
use std::process

println(argc())   // e.g. 3 for: kryos run main.kry --verbose
```

---

### argv

`argv(i: i64) -> str`

Return the command-line argument at index `i`. Index `0` is the program name. Throws if `i` is out of bounds.

**Example:**
```kryos
use std::process

let program = argv(0)
let first_arg = argv(1)
println(program)    // e.g. "main"
println(first_arg)  // e.g. "--verbose"
```

---

### args

`args() -> Args`

Return all command-line arguments as an `Args` value for structured access.

**Example:**
```kryos
use std::process

let a = args()
```

---

### parse_args

`parse_args(definition: str) -> Args`

Parse command-line arguments according to a definition string and return structured `Args`.

**Example:**
```kryos
use std::process

let a = parse_args("--verbose --output=<file>")
```

---

## Subprocess Execution

### command

`command(cmd: str) -> Command`

Create a `Command` to run a shell command. Returns a `Command` value that can be configured and executed.

**Example:**
```kryos
use std::process

let cmd = command("ls -la /tmp")
```

**Note:** The `Command` type is returned for further configuration and execution. See the `Command` struct documentation for available methods such as `.run()`, `.output()`, and `.env()`.

---

## Complete Example

```kryos
use std::process

// Read required config from environment
if !env_has("DATABASE_URL") {
    exit_error("DATABASE_URL is not set")
}
let db_url = env_get("DATABASE_URL")
let log_level = env_get_or("LOG_LEVEL", "info")

println(log_level)
println(db_url)

// Check arguments
if argc() < 2 {
    println("usage: myapp <command>")
    exit(1)
}

let subcommand = argv(1)

if subcommand == "version" {
    println("v1.0.0")
    exit_ok()
}

println(subcommand)
```
