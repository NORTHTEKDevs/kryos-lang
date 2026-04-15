# std::io

File I/O, buffered readers/writers, and console I/O.

All functions in this module are available after `use std::io`. File operations use UTF-8 encoding by default. Paths use forward slashes on all platforms.

```kryos
use std::io
```

---

## File Struct

The `File` struct is the central type for all file I/O in `std::io`. Open a file with `open`, `create`, or `open_append`, then call methods on the returned handle.

```kryos
struct File {
    fd:     i64,
    path:   str,
    mode:   i32,
    closed: bool
}
```

### open

`open(path: str) -> File`

Open a file for reading. Throws a runtime error if the file does not exist or cannot be opened.

**Example:**
```kryos
use std::io

let f = open("config.toml")
let content = f.read_all()
f.close()
println(content)
```

---

### create

`create(path: str) -> File`

Open a file for writing. Creates the file if it does not exist; truncates it if it does. Throws a runtime error if the path is not writable.

**Example:**
```kryos
use std::io

let f = create("output.txt")
f.write("Hello, world!\n")
f.close()
```

---

### open_append

`open_append(path: str) -> File`

Open a file for appending. Creates the file if it does not exist. Throws a runtime error if the path is not writable.

**Example:**
```kryos
use std::io

let f = open_append("log.txt")
f.write("Event occurred\n")
f.close()
```

---

### open_with_mode

`open_with_mode(path: str, mode: i32) -> File`

Open a file with an explicit mode integer. Use the module constants `MODE_READ` (0), `MODE_WRITE` (1), or `MODE_APPEND` (2).

**Example:**
```kryos
use std::io

let f = open_with_mode("data.bin", MODE_WRITE)
f.write_all(binary_data)
f.close()
```

---

## File Methods

### File.read

`read(self: File, n: i64) -> str`

Read up to `n` bytes from the file. Returns the bytes read as a string. Returns an empty string at EOF.

**Edge cases:**
- Throws a runtime error if the file is already closed.
- May return fewer bytes than requested at EOF.

---

### File.read_all

`read_all(self: File) -> str`

Read the entire remaining contents of the file as a string.

**Example:**
```kryos
let f = open("notes.txt")
let text = f.read_all()
f.close()
println(text)
```

**Edge cases:**
- Throws a runtime error if the file is already closed.
- Reads in 8192-byte chunks internally.

---

### File.write

`write(self: File, data: str) -> i64`

Write data to the file. Returns the number of bytes written.

**Edge cases:**
- Throws a runtime error if the file is already closed or if the write fails.
- May write fewer bytes than the length of `data` in rare cases. Use `write_all` for guaranteed full writes.

**See also:** `File.write_all`

---

### File.write_all

`write_all(self: File, data: str) -> i64`

Write all of `data` to the file, retrying until every byte is written. Returns the total number of bytes written.

**Example:**
```kryos
let f = create("report.txt")
f.write_all("Line 1\nLine 2\nLine 3\n")
f.close()
```

**Edge cases:**
- Throws a runtime error if the file is already closed or if any write attempt fails.

---

### File.close

`close(self: File) -> File`

Close the file handle. Returns a new `File` value with `closed: true`. Safe to call on an already-closed file (no-op).

**Example:**
```kryos
let f = open("data.txt")
let text = f.read_all()
let f = f.close()
```

**Edge cases:**
- Always close files when done. Unclosed files leak OS file descriptors.

---

### File.is_open

`is_open(self: File) -> bool`

Return `true` if the file handle is still open.

**Example:**
```kryos
let f = open("data.txt")
println(f.is_open())   // true
let f = f.close()
println(f.is_open())   // false
```

---

## FileError

The `FileError` enum represents error conditions for file operations. Thrown as strings in the current implementation -- structured error handling is planned.

```kryos
enum FileError {
    NotFound(str),
    PermissionDenied(str),
    AlreadyClosed(str),
    WriteFailed(str),
    ReadFailed(str),
    Unknown(str)
}
```

---

## Buffered I/O

For large files or line-by-line reading, use `BufReader` and `BufWriter`. These wrap a `File` and buffer reads/writes internally to reduce syscall overhead.

### buf_reader

`buf_reader(file: File) -> BufReader`

Create a `BufReader` with the default buffer size (8192 bytes).

**Example:**
```kryos
use std::io

let f = open("large.log")
let mut reader = buf_reader(f)

while not reader.is_eof() {
    let line = reader.read_line()
    println(line)
}
reader.close()
```

---

### buf_reader_sized

`buf_reader_sized(file: File, size: i64) -> BufReader`

Create a `BufReader` with a custom buffer size in bytes.

**Example:**
```kryos
let f = open("data.bin")
let reader = buf_reader_sized(f, 65536)
```

---

### BufReader Methods

#### BufReader.read_line

`read_line(self: BufReader) -> str`

Read the next line from the file. Strips the trailing newline (and carriage return on Windows). Returns an empty string after EOF.

**Example:**
```kryos
let f = open("names.txt")
let mut r = buf_reader(f)
while not r.is_eof() {
    let name = r.read_line()
    println("Name: " + name)
}
r.close()
```

---

#### BufReader.is_eof

`is_eof(self: BufReader) -> bool`

Return `true` if the reader has reached end of file and the internal buffer is exhausted.

---

#### BufReader.fill

`fill(self: BufReader) -> BufReader`

Manually fill the internal buffer from the underlying file. Called automatically by `read_line` as needed.

---

#### BufReader.close

`close(self: BufReader) -> BufReader`

Close the underlying file handle.

---

### buf_writer

`buf_writer(file: File) -> BufWriter`

Create a `BufWriter` with the default buffer size (8192 bytes). Writes are accumulated in memory and flushed to the file automatically when the buffer is full, or manually via `flush`.

**Example:**
```kryos
use std::io

let f = create("output.csv")
let mut w = buf_writer(f)
w = w.write("name,score\n")
w = w.write("alice,98\n")
w = w.write("bob,87\n")
w.close()
```

---

### buf_writer_sized

`buf_writer_sized(file: File, size: i64) -> BufWriter`

Create a `BufWriter` with a custom buffer size in bytes.

---

### BufWriter Methods

#### BufWriter.write

`write(self: BufWriter, data: str) -> BufWriter`

Write data to the internal buffer. Flushes automatically to the underlying file when the buffer reaches capacity. Returns an updated `BufWriter`.

---

#### BufWriter.flush

`flush(self: BufWriter) -> BufWriter`

Flush any buffered data to the underlying file immediately. Returns an updated `BufWriter` with an empty buffer.

**Edge cases:**
- Always call `flush` or `close` before the program exits, or buffered data may be lost.

---

#### BufWriter.close

`close(self: BufWriter) -> BufWriter`

Flush remaining data and close the underlying file handle.

---

## Console Output

These functions write to stdout or stderr. Prefer the core builtins `print` and `println` for ordinary output. Use `eprint`/`eprintln` to write diagnostic or error output to stderr without mixing with stdout.

### print

`print(msg: str) -> i64`

Write `msg` to stdout without a trailing newline. Returns the number of bytes written.

---

### println

`println(msg: str) -> i64`

Write `msg` to stdout followed by a newline. Returns the number of bytes written.

---

### eprint

`eprint(msg: str) -> i64`

Write `msg` to stderr without a trailing newline.

**Example:**
```kryos
use std::io

eprint("warning: retrying connection\n")
```

---

### eprintln

`eprintln(msg: str) -> i64`

Write `msg` to stderr followed by a newline.

**Example:**
```kryos
use std::io

eprintln("error: config file not found")
```

---

## Console Input

### read_line

`read_line() -> str`

Read a single line from stdin. Blocks until the user presses Enter. Strips the trailing newline (and `\r\n` on Windows).

**Example:**
```kryos
use std::io

print("Enter your name: ")
let name = read_line()
println("Hello, " + name)
```

**Edge cases:**
- Returns an empty string on EOF (e.g., when stdin is piped and exhausted).
- Buffer size is 4096 bytes. Lines longer than 4096 bytes are truncated.

**See also:** `stdin_read` (core builtin)

---

### read_stdin

`read_stdin(n: i64) -> str`

Read up to `n` bytes from stdin. Does not strip newlines.

**Example:**
```kryos
use std::io

let raw = read_stdin(1024)
println("Read " + to_string(len(raw)) + " bytes")
```

---

## Planned Functions

> **Implementation Status:** The following functions are planned for `std::io`. They are not yet available in the runtime. Use the `File` struct API above as the current alternative.

| Function | Planned signature | Alternative today |
|----------|-------------------|-------------------|
| `file_read` | `file_read(path: str) -> str` | `open(path).read_all()` |
| `file_write` | `file_write(path: str, content: str)` | `create(path).write_all(content)` |
| `file_append` | `file_append(path: str, content: str)` | `open_append(path).write_all(content)` |
| `file_exists` | `file_exists(path: str) -> bool` | planned |
| `file_delete` | `file_delete(path: str)` | planned |
| `file_lines` | `file_lines(path: str) -> [str]` | `open(path).read_all()` + `split(text, "\n")` |
| `file_copy` | `file_copy(src: str, dst: str)` | planned |
| `file_move` | `file_move(src: str, dst: str)` | planned |
| `file_size` | `file_size(path: str) -> i64` | planned |
| `file_modified` | `file_modified(path: str) -> i64` | planned |
| `file_is_dir` | `file_is_dir(path: str) -> bool` | planned |
| `file_is_file` | `file_is_file(path: str) -> bool` | planned |
| `dir_list` | `dir_list(path: str) -> [str]` | planned |
| `dir_create` | `dir_create(path: str)` | planned |
| `dir_remove` | `dir_remove(path: str)` | planned |
| `glob` | `glob(pattern: str) -> [str]` | planned |
| `path_join` | `path_join(parts: ...str) -> str` | planned |
| `path_dirname` | `path_dirname(path: str) -> str` | planned |
| `path_basename` | `path_basename(path: str) -> str` | planned |
| `path_extension` | `path_extension(path: str) -> str` | planned |
| `path_resolve` | `path_resolve(path: str) -> str` | planned |
| `env_get` | `env_get(name: str) -> str` | planned |
| `env_set` | `env_set(name: str, value: str)` | planned |
| `cwd` | `cwd() -> str` | planned |
| `temp_file` | `temp_file(prefix: str?) -> str` | planned |
| `temp_dir` | `temp_dir(prefix: str?) -> str` | planned |
