# std::fs

Filesystem operations: reading, writing, copying, moving, and inspecting files and directories. All path arguments accept both relative and absolute paths.

```kryos
use std::fs
```

---

## Types

### DirEntry

A single entry returned by directory listing functions.

| Field     | Type   | Description                       |
|-----------|--------|-----------------------------------|
| `name`    | `str`  | Filename only (no parent path)    |
| `path`    | `str`  | Full path to the entry            |
| `is_file` | `bool` | `true` if the entry is a file     |
| `is_dir`  | `bool` | `true` if the entry is a directory|

---

## Predicates

### exists

`exists(path: str) -> bool`

Return `true` if `path` exists (file or directory).

---

### is_file

`is_file(path: str) -> bool`

Return `true` if `path` exists and is a regular file.

---

### is_dir

`is_dir(path: str) -> bool`

Return `true` if `path` exists and is a directory.

**Example:**
```kryos
use std::fs

if exists("/tmp/data.json") {
    println(is_file("/tmp/data.json"))   // true
}
```

---

## Directory Operations

### create_dir

`create_dir(path: str) -> bool`

Create a single directory. Throws if the parent directory does not exist.

---

### create_dir_all

`create_dir_all(path: str) -> bool`

Create `path` and all missing parent directories. Safe to call if the directory already exists.

**Example:**
```kryos
use std::fs

create_dir_all("/tmp/app/logs/2025")
```

---

### remove_dir

`remove_dir(path: str) -> bool`

Remove an empty directory. Throws if the directory is not empty.

---

### list_dir

`list_dir(path: str) -> [str]`

Return the names of all entries directly inside `path`. Does not recurse into subdirectories.

**Example:**
```kryos
use std::fs

let entries = list_dir("/home/alice/projects")
let i = 0
while i < len(entries) {
    println(entries[i])
    i = i + 1
}
```

---

### walk_dir

`walk_dir(path: str) -> [str]`

Return the paths of all files and directories under `path`, recursing into all subdirectories.

**Example:**
```kryos
use std::fs

let all = walk_dir("/home/alice/src")
// Returns every file and directory path under /home/alice/src
```

---

## File Operations

### read_file

`read_file(path: str) -> str`

Read the entire contents of `path` as a string. Throws if the file cannot be
opened (does not exist, permissions) or if the underlying read syscall fails.
**Does NOT throw on invalid-UTF-8 content** -- the bytes are wrapped into a
`str` unchecked; a later operation on that string (`substr`, indexing, and
some stdlib string functions) is what will panic, with a message pointing
back at UTF-8 validity, not `read_file` itself. Check `std::utf8::is_valid(s)`
after reading a file whose contents are not guaranteed to be text. This
differs from the global `file_read(path)` builtin (no `use` needed, not a
`std::fs` function), which panics immediately on invalid UTF-8 rather than
deferring the failure -- pick `file_read` if you want the panic at the read
site, `read_file` if you want a catchable `throw` (only for open/read
failures, not content validity) plus deferred detection.

**Example:**
```kryos
use std::fs

let config = read_file("config.json")
println(config)
```

---

### write_file

`write_file(path: str, data: str) -> i64`

Write `data` to `path`, creating the file if it does not exist and truncating it if it does. Returns the number of bytes written.

**Example:**
```kryos
use std::fs

let n = write_file("/tmp/output.txt", "hello, world\n")
println(n)   // 13
```

---

### append_file

`append_file(path: str, data: str) -> i64`

Append `data` to `path`. Creates the file if it does not exist. Returns the number of bytes written.

**Example:**
```kryos
use std::fs

append_file("/var/log/app.log", "started\n")
```

---

### remove_file

`remove_file(path: str) -> bool`

Delete `path`. Throws if the file does not exist or cannot be removed.

---

### copy

`copy(src: str, dst: str) -> i64`

Copy the file at `src` to `dst`. Returns the number of bytes copied. Overwrites `dst` if it already exists.

**Example:**
```kryos
use std::fs

copy("config.json", "config.json.bak")
```

---

### rename

`rename(old_path: str, new_path: str) -> bool`

Move or rename `old_path` to `new_path`. Overwrites `new_path` if it already exists.

**Example:**
```kryos
use std::fs

rename("draft.md", "published.md")
```

---

### file_size

`file_size(path: str) -> i64`

Return the size of `path` in bytes. Throws if the path does not exist or is not a file.

**Example:**
```kryos
use std::fs

let size = file_size("video.mp4")
println(size)   // e.g. 10485760
```

---

## Path Helpers

These functions operate purely on strings -- they do not touch the filesystem.

### path_join

`path_join(base: str, child: str) -> str`

Append `child` to `base` with the platform separator.

---

### path_basename

`path_basename(path: str) -> str`

Return the final path component.

---

### path_dirname

`path_dirname(path: str) -> str`

Return all path components except the final one.

---

### path_extension

`path_extension(path: str) -> str`

Return the file extension including the leading dot (e.g. `".json"`), or an empty string if there is none.

---

### path_stem

`path_stem(path: str) -> str`

Return the filename without its extension.

---

### is_absolute

`is_absolute(path: str) -> bool`

Return `true` if `path` is absolute.

---

### normalize_path

`normalize_path(path: str) -> str`

Resolve `.` and `..` components and collapse duplicate separators.

**Example:**
```kryos
use std::fs

let p = path_join("/home/alice", "projects")
println(p)                                  // "/home/alice/projects"
println(path_basename("src/main.kry"))      // "main.kry"
println(path_extension("archive.tar.gz"))  // ".gz"
println(normalize_path("a/b/../c/./d"))    // "a/c/d"
```

---

## Complete Example

```kryos
use std::fs

// Ensure a directory structure exists
create_dir_all("/tmp/app/data")

// Write a file and read it back
write_file("/tmp/app/data/hello.txt", "Hello, Kryos!\n")
let content = read_file("/tmp/app/data/hello.txt")
println(content)   // "Hello, Kryos!"

// Append a log entry
append_file("/tmp/app/app.log", "started\n")

// Copy and rename
copy("/tmp/app/data/hello.txt", "/tmp/app/data/hello.bak")
rename("/tmp/app/data/hello.bak", "/tmp/app/data/backup.txt")

// Inspect
println(file_size("/tmp/app/data/hello.txt"))   // 14
println(is_file("/tmp/app/data/hello.txt"))     // true
println(is_dir("/tmp/app/data"))               // true

// List and walk
let entries = list_dir("/tmp/app/data")
let i = 0
while i < len(entries) {
    println(entries[i])   // "hello.txt", "backup.txt"
    i = i + 1
}

// Clean up
remove_file("/tmp/app/data/backup.txt")
```
