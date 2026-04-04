# std::io

File system, directory, path, environment, and temporary file operations.

All functions in this module are available after `use std::io`. File operations use UTF-8 encoding by default. Paths use forward slashes on all platforms.

---

## File Operations

### file_read

```
file_read(path: str) -> str
```

Read the entire contents of a file as a UTF-8 string.

**Example:**

```kryos
let content = file_read("config.toml")
println(content)
```

**Edge cases:**

- Throws a runtime error if the file does not exist or is not readable.
- Binary files will be read but may produce garbled output since decoding is UTF-8.

**See also:** `file_lines`, `file_exists`

---

### file_write

```
file_write(path: str, content: str) -> bool
```

Write content to a file, replacing any existing content. Creates parent directories automatically if they do not exist. Returns `true` on success.

**Example:**

```kryos
file_write("output/report.txt", "Total: 42")
```

**Edge cases:**

- Creates the file if it does not exist.
- Overwrites the file completely if it already exists.
- Throws a runtime error if the path is not writable (permissions, invalid path).

**See also:** `file_append`, `dir_create`

---

### file_append

```
file_append(path: str, content: str) -> bool
```

Append content to the end of a file. The file must already exist. Returns `true` on success.

**Example:**

```kryos
file_append("log.txt", "Event occurred at 12:00\n")
```

**Edge cases:**

- Throws a runtime error if the file does not exist or is not writable.
- Does not add a newline automatically. Include `\n` in your content if needed.

**See also:** `file_write`

---

### file_exists

```
file_exists(path: str) -> bool
```

Check whether a file or directory exists at the given path.

**Example:**

```kryos
if file_exists("config.toml") {
    let cfg = file_read("config.toml")
    println(cfg)
} else {
    println("No config found")
}
```

**Edge cases:**

- Returns `true` for both files and directories.
- Does not throw on invalid paths -- returns `false`.

**See also:** `file_is_file`, `file_is_dir`

---

### file_delete

```
file_delete(path: str) -> bool
```

Delete a file. Returns `true` on success.

**Example:**

```kryos
file_delete("temp_output.txt")
```

**Edge cases:**

- Throws a runtime error if the file does not exist or cannot be deleted.
- Does not work on directories. Use `dir_remove` for directories.

**See also:** `dir_remove`, `file_exists`

---

### file_lines

```
file_lines(path: str) -> [str]
```

Read a file and return its non-empty lines as an array of strings. Trailing newlines and carriage returns are stripped from each line. Empty lines (whitespace-only) are excluded.

**Example:**

```kryos
let lines = file_lines("data.csv")
for line in lines {
    println(line)
}
```

**Edge cases:**

- Throws a runtime error if the file does not exist.
- Lines that contain only whitespace are skipped entirely.

**See also:** `file_read`, `split`

---

### file_copy

```
file_copy(src: str, dst: str) -> bool
```

Copy a file from `src` to `dst`, preserving metadata (timestamps, permissions). Returns `true` on success.

**Example:**

```kryos
file_copy("template.txt", "output.txt")
```

**Edge cases:**

- Overwrites `dst` if it already exists.
- Throws a runtime error if `src` does not exist or `dst` is not writable.

**See also:** `file_move`

---

### file_move

```
file_move(src: str, dst: str) -> bool
```

Move (rename) a file from `src` to `dst`. Returns `true` on success.

**Example:**

```kryos
file_move("draft.txt", "final.txt")
```

**Edge cases:**

- The source file no longer exists after a successful move.
- Can move across directories.
- Throws a runtime error if `src` does not exist.

**See also:** `file_copy`, `file_delete`

---

### file_size

```
file_size(path: str) -> i32
```

Return the size of a file in bytes.

**Example:**

```kryos
let size = file_size("data.bin")
println("File is " + to_string(size) + " bytes")
```

**Edge cases:**

- Throws a runtime error if the file does not exist.

**See also:** `file_exists`, `file_modified`

---

### file_modified

```
file_modified(path: str) -> i32
```

Return the last modification time of a file as a Unix timestamp (seconds since epoch).

**Example:**

```kryos
let ts = file_modified("config.toml")
println("Last modified: " + to_string(ts))
```

**Edge cases:**

- Throws a runtime error if the file does not exist.

**See also:** `file_size`

---

### file_is_dir

```
file_is_dir(path: str) -> bool
```

Check whether the path points to a directory.

**Example:**

```kryos
if file_is_dir("src") {
    let entries = dir_list("src")
    println("Source has " + to_string(len(entries)) + " entries")
}
```

**See also:** `file_is_file`, `file_exists`

---

### file_is_file

```
file_is_file(path: str) -> bool
```

Check whether the path points to a regular file (not a directory).

**Example:**

```kryos
if file_is_file("main.kry") {
    println("Found main source file")
}
```

**See also:** `file_is_dir`, `file_exists`

---

## Directory Operations

### dir_list

```
dir_list(path: str) -> [str]
```

List the names of all entries (files and subdirectories) in a directory. Returns a sorted array of names (not full paths).

**Example:**

```kryos
let entries = dir_list(".")
for entry in entries {
    println(entry)
}
```

**Edge cases:**

- Throws a runtime error if the path does not exist or is not a directory.
- Does not include `.` or `..`.
- Returns names only, not full paths. Use `path_join` to build full paths.

**See also:** `glob`, `path_join`

---

### dir_create

```
dir_create(path: str) -> bool
```

Create a directory, including any missing parent directories. Returns `true` on success.

**Example:**

```kryos
dir_create("output/reports/2026")
```

**Edge cases:**

- Does not throw if the directory already exists.
- Creates all intermediate directories.

**See also:** `dir_remove`, `file_is_dir`

---

### dir_remove

```
dir_remove(path: str) -> bool
```

Remove a directory and all of its contents recursively. Returns `true` on success.

**Example:**

```kryos
dir_remove("build")
```

**Edge cases:**

- Deletes everything inside the directory. Use with caution.
- Throws a runtime error if the path does not exist.

**See also:** `dir_create`, `file_delete`

---

## Glob

### glob

```
glob(pattern: str) -> [str]
```

Find all file paths matching a glob pattern. Supports `*`, `**` (recursive), and `?` wildcards. Returns a sorted array of matching paths.

**Example:**

```kryos
let sources = glob("src/**/*.kry")
for f in sources {
    println(f)
}
```

```kryos
let configs = glob("*.toml")
println("Found " + to_string(len(configs)) + " config files")
```

**Edge cases:**

- Returns an empty array if no files match.
- `**` matches across directory boundaries (recursive).
- Throws a runtime error only if the pattern itself is invalid.

**See also:** `dir_list`

---

## Path Utilities

### path_join

```
path_join(parts: ...str) -> str
```

Join path segments with the platform path separator. Always returns forward slashes.

**Example:**

```kryos
let full = path_join("src", "lib", "main.kry")
println(full)  // "src/lib/main.kry"
```

**See also:** `path_dirname`, `path_basename`

---

### path_dirname

```
path_dirname(path: str) -> str
```

Return the directory portion of a path.

**Example:**

```kryos
let dir = path_dirname("src/lib/main.kry")
println(dir)  // "src/lib"
```

**See also:** `path_basename`, `path_join`

---

### path_basename

```
path_basename(path: str) -> str
```

Return the final component of a path (the file name).

**Example:**

```kryos
let name = path_basename("src/lib/main.kry")
println(name)  // "main.kry"
```

**See also:** `path_dirname`, `path_extension`

---

### path_extension

```
path_extension(path: str) -> str
```

Return the file extension without the leading dot.

**Example:**

```kryos
let ext = path_extension("report.pdf")
println(ext)  // "pdf"
```

**Edge cases:**

- Returns an empty string if the file has no extension.
- If the file has multiple dots (e.g., `archive.tar.gz`), returns only the last extension (`gz`).

**See also:** `path_basename`

---

### path_resolve

```
path_resolve(path: str) -> str
```

Return the absolute path, resolving relative segments. Always returns forward slashes.

**Example:**

```kryos
let abs = path_resolve("../config.toml")
println(abs)  // "/home/user/config.toml"
```

**See also:** `cwd`

---

## Environment and Temp

### stdin_read

```
stdin_read() -> str
```

Read one line of input from stdin. Blocks until the user presses Enter.

**Example:**

```kryos
println("What is your name?")
let name = stdin_read()
println("Hello, " + name)
```

**Edge cases:**

- Returns an empty string on EOF.

---

### env_get

```
env_get(name: str) -> str
```

Get the value of an environment variable. Returns an empty string if the variable is not set.

**Example:**

```kryos
let home = env_get("HOME")
println("Home directory: " + home)
```

**Edge cases:**

- Returns `""` (empty string) if the variable does not exist. Does not throw.

**See also:** `env_set`

---

### env_set

```
env_set(name: str, value: str) -> none
```

Set an environment variable for the current process.

**Example:**

```kryos
env_set("APP_MODE", "production")
let mode = env_get("APP_MODE")
println(mode)  // "production"
```

**Edge cases:**

- Only affects the current process. Child processes inherit the variable, but the parent shell does not see it.

**See also:** `env_get`

---

### cwd

```
cwd() -> str
```

Return the current working directory as an absolute path with forward slashes.

**Example:**

```kryos
let dir = cwd()
println("Working in: " + dir)
```

---

### temp_file

```
temp_file(prefix: str?) -> str
```

Create a temporary file and return its path. The file is created empty on disk. The optional `prefix` argument sets a name prefix (defaults to `"kryos_"`).

**Example:**

```kryos
let tmp = temp_file()
file_write(tmp, "scratch data")
println("Temp file at: " + tmp)
```

```kryos
let tmp = temp_file("report_")
file_write(tmp, "data")
```

**Edge cases:**

- The file is created immediately. You are responsible for deleting it when done.
- Path always uses forward slashes.

**See also:** `temp_dir`, `file_delete`

---

### temp_dir

```
temp_dir(prefix: str?) -> str
```

Create a temporary directory and return its path. The optional `prefix` argument sets a name prefix (defaults to `"kryos_"`).

**Example:**

```kryos
let dir = temp_dir("build_")
file_write(path_join(dir, "output.txt"), "result")
```

**Edge cases:**

- The directory is created immediately. You are responsible for removing it when done.

**See also:** `temp_file`, `dir_remove`
