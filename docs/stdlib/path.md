# std::path

Pure string path manipulation. No I/O -- all functions operate on path strings only and never touch the filesystem.

```kryos
use std::path
```

---

## Construction

### join

`join(parts: [str]) -> str`

Join an array of path segments into a single path, inserting the platform separator between each part and normalizing the result.

**Example:**
```kryos
use std::path

let p = join(["/home", "alice", "projects", "main.kry"])
println(p)   // "/home/alice/projects/main.kry"
```

---

### child

`child(p: str, name: str) -> str`

Append a single child segment to a path. Equivalent to `join([p, name])`.

**Example:**
```kryos
use std::path

let p = child("/home/alice", "config.json")
println(p)   // "/home/alice/config.json"
```

---

### with_extension

`with_extension(p: str, ext: str) -> str`

Return `p` with the file extension replaced by `ext`. The leading dot in `ext` is optional -- both `"txt"` and `".txt"` produce the same result.

**Example:**
```kryos
use std::path

println(with_extension("archive.tar.gz", "bz2"))   // "archive.tar.bz2"
println(with_extension("script.kry", "js"))         // "script.js"
println(with_extension("noext", ".md"))             // "noext.md"
```

---

## Decomposition

### dirname

`dirname(p: str) -> str`

Return the directory portion of `p` -- everything up to and including the last separator.

**Example:**
```kryos
use std::path

println(dirname("/home/alice/file.txt"))   // "/home/alice"
println(dirname("src/main.kry"))           // "src"
println(dirname("file.txt"))              // "."
```

---

### basename

`basename(p: str) -> str`

Return the final component of `p`, including the extension.

**Example:**
```kryos
use std::path

println(basename("/home/alice/file.txt"))   // "file.txt"
println(basename("/home/alice/"))           // "alice"
```

---

### parent

`parent(p: str) -> str`

Alias for `dirname`. Return the parent directory of `p`.

---

### extname

`extname(p: str) -> str`

Return the file extension including the leading dot, e.g. `".txt"`. Returns an empty string if there is no extension.

**Example:**
```kryos
use std::path

println(extname("report.pdf"))      // ".pdf"
println(extname("archive.tar.gz"))  // ".gz"
println(extname("Makefile"))        // ""
```

---

### stem

`stem(p: str) -> str`

Return the filename without its extension.

**Example:**
```kryos
use std::path

println(stem("report.pdf"))      // "report"
println(stem("archive.tar.gz"))  // "archive.tar"
println(stem("Makefile"))        // "Makefile"
```

---

### split

`split(p: str) -> [str]`

Split a path into its individual components. The root separator is returned as the first element for absolute paths.

**Example:**
```kryos
use std::path

let parts = split("/home/alice/file.txt")
println(parts)   // ["/", "home", "alice", "file.txt"]

let rel = split("src/lib/mod.kry")
println(rel)     // ["src", "lib", "mod.kry"]
```

---

## Normalization

### normalize

`normalize(p: str) -> str`

Resolve `.` and `..` components, collapse duplicate separators, and return a clean path.

**Example:**
```kryos
use std::path

println(normalize("/home/alice/../bob/./file.txt"))   // "/home/bob/file.txt"
println(normalize("src//lib/../main.kry"))            // "src/main.kry"
```

---

### relative

`relative(from: str, to: str) -> str`

Compute the relative path from `from` to `to`.

**Example:**
```kryos
use std::path

println(relative("/home/alice", "/home/alice/projects/app"))   // "projects/app"
println(relative("/home/alice/src", "/home/alice/docs"))       // "../docs"
```

---

## Predicates

### is_absolute

`is_absolute(p: str) -> bool`

Return `true` if `p` is an absolute path.

---

### is_relative

`is_relative(p: str) -> bool`

Return `true` if `p` is a relative path.

---

### has_extension

`has_extension(p: str) -> bool`

Return `true` if the filename has an extension.

---

### starts_with

`starts_with(p: str, prefix: str) -> bool`

Return `true` if `p` starts with the path component `prefix`.

**Example:**
```kryos
use std::path

println(starts_with("/home/alice/file.txt", "/home/alice"))   // true
println(starts_with("/home/alice/file.txt", "/home/bob"))     // false
```

---

### equals

`equals(a: str, b: str) -> bool`

Return `true` if `a` and `b` refer to the same path after normalization. On case-insensitive file systems the comparison is case-insensitive.

**Example:**
```kryos
use std::path

println(equals("/home/alice", "/home/alice/"))         // true
println(equals("./src/../main.kry", "main.kry"))       // true
```

---

## Complete Example

```kryos
use std::path

// Build a path from parts
let base = join(["/projects", "kryos", "src"])
let file = child(base, "main.kry")
println(file)   // "/projects/kryos/src/main.kry"

// Decompose it
println(dirname(file))    // "/projects/kryos/src"
println(basename(file))   // "main.kry"
println(stem(file))       // "main"
println(extname(file))    // ".kry"

// Swap the extension
let js = with_extension(file, ".js")
println(js)   // "/projects/kryos/src/main.js"

// Normalize a messy path
let messy = "/projects/kryos/src/../lib/./utils.kry"
println(normalize(messy))   // "/projects/kryos/lib/utils.kry"

// Compute relative path
println(relative("/projects/kryos/src", "/projects/kryos/docs"))   // "../docs"

// Predicates
println(is_absolute(file))         // true
println(has_extension(file))       // true
println(starts_with(file, base))   // true
```
