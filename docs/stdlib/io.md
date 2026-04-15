# std::io

Low-level file I/O, console I/O, and buffered readers and writers. For simple file reads and writes, prefer `std::fs`. Use `std::io` when you need streaming access, buffering, or explicit file handle control.

```kryos
use std::io
```

---

## Constants

| Constant      | Value | Description     |
|---------------|-------|-----------------|
| `MODE_READ`   | `0`   | Open for reading |
| `MODE_WRITE`  | `1`   | Open for writing (truncate) |
| `MODE_APPEND` | `2`   | Open for appending |

---

## Types

### File

An open file handle.

| Field    | Type   | Description                            |
|----------|--------|----------------------------------------|
| `fd`     | `i64`  | Native file descriptor                 |
| `path`   | `str`  | Path the file was opened from          |
| `mode`   | `i32`  | Mode constant (`MODE_READ`, etc.)      |
| `closed` | `bool` | Whether the file has been closed       |

### FileError

An error value produced by file operations.

| Variant                 | Payload | Description                     |
|-------------------------|---------|---------------------------------|
| `NotFound(str)`         | path    | File does not exist             |
| `PermissionDenied(str)` | path    | Insufficient permissions        |
| `AlreadyClosed(str)`    | path    | Operation on a closed file      |
| `WriteFailed(str)`      | message | Write operation failed          |
| `ReadFailed(str)`       | message | Read operation failed           |
| `Unknown(str)`          | message | Unclassified error              |

---

## Opening Files

### open

`open(path: str) -> File`

Open `path` for reading (`MODE_READ`). Throws a `FileError` if the file does not exist.

---

### create

`create(path: str) -> File`

Open `path` for writing (`MODE_WRITE`), creating it if it does not exist and truncating it if it does.

---

### open_append

`open_append(path: str) -> File`

Open `path` for appending (`MODE_APPEND`). Creates the file if it does not exist.

---

### open_with_mode

`open_with_mode(path: str, mode: i32) -> File`

Open `path` with an explicit mode constant.

**Example:**
```kryos
use std::io

let f = open("data.txt")
let w = create("output.txt")
let a = open_append("log.txt")
let m = open_with_mode("data.txt", MODE_READ)
```

---

## File Methods

### read

`read(n: i64) -> str`

Read up to `n` bytes from the file and return them as a string. Returns fewer bytes at end of file.

---

### read_all

`read_all() -> str`

Read the entire remaining contents of the file.

---

### write

`write(data: str) -> i64`

Write `data` to the file. Returns the number of bytes written.

---

### write_all

`write_all(data: str) -> i64`

Write all of `data`, retrying on partial writes. Returns the total number of bytes written.

---

### close

`close() -> File`

Flush and close the file. Returns `self`. Subsequent operations on the handle will throw `FileError.AlreadyClosed`.

---

### is_open

`is_open() -> bool`

Return `true` if the file has not been closed.

**Example:**
```kryos
use std::io

let f = open("readme.md")
let text = f.read_all()
println(text)
f.close()
println(f.is_open())   // false
```

---

## Console I/O

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

---

### eprintln

`eprintln(msg: str) -> i64`

Write `msg` to stderr followed by a newline.

---

### read_line

`read_line() -> str`

Read one line from stdin, blocking until the user presses Enter. The returned string includes the newline.

---

### read_stdin

`read_stdin(n: i64) -> str`

Read up to `n` bytes from stdin.

**Example:**
```kryos
use std::io

print("Enter your name: ")
let name = read_line()
println("Hello, " + name)
```

---

## BufReader

A buffered reader that reduces system-call overhead for line-by-line reading.

### buf_reader

`buf_reader(file: File) -> BufReader`

Create a `BufReader` with the default buffer size.

---

### buf_reader_sized

`buf_reader_sized(file: File, size: i64) -> BufReader`

Create a `BufReader` with an explicit buffer size in bytes.

---

### BufReader Methods

#### fill

`fill()`

Refill the internal buffer from the underlying file.

---

#### read_line

`read_line() -> str`

Return the next line including the trailing newline, or an empty string at end of file.

---

#### is_eof

`is_eof() -> bool`

Return `true` if the end of file has been reached.

---

#### close

`close()`

Close the underlying file.

**Example:**
```kryos
use std::io

let f = open("large.log")
let reader = buf_reader(f)

while !reader.is_eof() {
    let line = reader.read_line()
    if len(line) > 0 {
        print(line)
    }
}

reader.close()
```

---

## BufWriter

A buffered writer that coalesces small writes into larger system calls.

### buf_writer

`buf_writer(file: File) -> BufWriter`

Create a `BufWriter` with the default buffer size.

---

### buf_writer_sized

`buf_writer_sized(file: File, size: i64) -> BufWriter`

Create a `BufWriter` with an explicit buffer size in bytes.

---

### BufWriter Methods

#### write

`write(data: str) -> BufWriter`

Buffer `data` for writing. Returns `self` for chaining.

---

#### flush

`flush() -> BufWriter`

Write all buffered data to the underlying file. Returns `self` for chaining.

---

#### close

`close() -> BufWriter`

Flush the buffer and close the underlying file.

**Example:**
```kryos
use std::io

let f = create("report.txt")
let writer = buf_writer(f)

writer.write("line 1\n").write("line 2\n").write("line 3\n")
writer.flush()
writer.close()
```

---

## Complete Example

```kryos
use std::io

// Read a file line by line with a buffered reader
let f = open("input.csv")
let reader = buf_reader(f)
let row_count = 0

while !reader.is_eof() {
    let line = reader.read_line()
    if len(line) > 0 {
        row_count = row_count + 1
    }
}
reader.close()
println("rows: " + row_count)

// Write output with a buffered writer
let out = create("output.txt")
let writer = buf_writer(out)
writer.write("processed " + row_count + " rows\n")
writer.close()

// Console interaction
print("Continue? [y/n]: ")
let answer = read_line()
if answer == "y\n" {
    println("continuing...")
}
```
