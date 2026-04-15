# std::os

Operating system detection and platform-specific directory paths.

```kryos
use std::os
```

---

## Platform Detection

### name

`name() -> str`

Return the current operating system name. Returns one of: `"windows"`, `"linux"`, `"macos"`, or `"unknown"`.

**Example:**
```kryos
use std::os

println(name())   // "linux", "windows", or "macos"
```

---

### arch

`arch() -> str`

Return the CPU architecture. Returns `"x86_64"`, `"aarch64"`, `"x86"`, or the raw architecture string from the environment.

**Example:**
```kryos
use std::os

println(arch())   // "x86_64"
```

---

### family

`family() -> str`

Return the OS family: `"unix"` or `"windows"`.

**Example:**
```kryos
use std::os

println(family())   // "unix"
```

---

### is_windows

`is_windows() -> bool`

Return `true` if running on Windows.

---

### is_linux

`is_linux() -> bool`

Return `true` if running on Linux.

---

### is_macos

`is_macos() -> bool`

Return `true` if running on macOS.

---

### is_unix

`is_unix() -> bool`

Return `true` if the OS family is Unix (Linux or macOS).

**Example:**
```kryos
use std::os

if is_windows() {
    println("windows path handling")
} else if is_unix() {
    println("unix path handling")
}
```

---

## Platform Strings

### path_sep

`path_sep() -> str`

Return the platform path separator: `"/"` on Unix, `"\\"` on Windows.

---

### path_list_sep

`path_list_sep() -> str`

Return the path list separator used in environment variables like `PATH`: `":"` on Unix, `";"` on Windows.

---

### line_ending

`line_ending() -> str`

Return the platform line ending: `"\n"` on Unix, `"\r\n"` on Windows.

**Example:**
```kryos
use std::os

let sep = path_sep()
let eol = line_ending()
```

---

## Platform Directories

All directory functions throw a runtime error if the directory cannot be determined.

### home_dir

`home_dir() -> str`

Return the current user's home directory. Uses `HOME` on Unix, `USERPROFILE` on Windows.

**Example:**
```kryos
use std::os

let home = home_dir()
println(home)   // e.g. "/home/alice" or "C:\Users\Alice"
```

---

### temp_dir

`temp_dir() -> str`

Return the system temporary directory.

**Example:**
```kryos
use std::os

let tmp = temp_dir()
println(tmp)   // e.g. "/tmp"
```

---

### current_dir

`current_dir() -> str`

Return the current working directory. Throws if the directory cannot be determined.

**Example:**
```kryos
use std::os

let cwd = current_dir()
println(cwd)
```

---

### config_dir

`config_dir() -> str`

Return the platform config directory. On Linux: `~/.config`. On macOS: `~/Library/Application Support`. On Windows: `%APPDATA%`.

---

### data_dir

`data_dir() -> str`

Return the platform data directory. On Linux: `~/.local/share`. On macOS: `~/Library/Application Support`. On Windows: `%APPDATA%`.

---

### cache_dir

`cache_dir() -> str`

Return the platform cache directory. On Linux: `~/.cache`. On macOS: `~/Library/Caches`. On Windows: `%LOCALAPPDATA%`.

**Example:**
```kryos
use std::os

let cfg = config_dir()
let data = data_dir()
let cache = cache_dir()
```

---

## User and Host

### username

`username() -> str`

Return the current user's login name.

**Example:**
```kryos
use std::os

println(username())   // e.g. "alice"
```

---

### hostname

`hostname() -> str`

Return the machine hostname.

**Example:**
```kryos
use std::os

println(hostname())   // e.g. "dev-box"
```

---

## Complete Example

```kryos
use std::os

// Print platform info
println(name())      // "linux"
println(arch())      // "x86_64"
println(family())    // "unix"

// Platform-conditional logic
if is_windows() {
    println("running on Windows")
} else {
    println("running on a Unix system")
}

// Build an app-specific config path
let cfg = config_dir() + path_sep() + "myapp" + path_sep() + "config.json"
println(cfg)   // e.g. "/home/alice/.config/myapp/config.json"

// Print user info
println(username())   // "alice"
println(hostname())   // "dev-box"
```
