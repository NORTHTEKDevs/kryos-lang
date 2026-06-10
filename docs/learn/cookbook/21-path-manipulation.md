# Cookbook 21 · Path manipulation

`std::pathext` adds normalize, component-counting, and absolute-path
detection on top of the core `std::path` join/dirname/basename helpers.
All ops are pure string manipulation — no syscalls.

## Examples

```kryos
use std::pathext::{is_absolute, normalize, component_count}

@capabilities(io)
fn main() {
    // Detect absolute paths
    println(to_string(is_absolute("/etc/hosts")))    // true
    println(to_string(is_absolute("relative/path"))) // false

    // Normalize
    println(normalize("a/b/../c"))     // → a/c
    println(normalize("a//b/./c"))     // → a/b/c
    println(normalize("/a/b/../../c")) // → /c
    println(normalize("./foo/bar"))    // → foo/bar

    // Component count
    println(to_string(component_count("a/b/c")))     // 3
    println(to_string(component_count("/a/b")))      // 2
    println(to_string(component_count("")))          // 0
}
```

## Things to know

- Normalization is purely lexical. `path_normalize("a/../b")` returns
  `"b"` even if `a` doesn't exist or is a file. For filesystem-aware
  resolution use `std::fs::canonicalize`.
- `..` past the root is silently absorbed (`"/.." == "/"`). For relative
  paths it accumulates (`"../../foo"`).
- Backslashes are always converted to forward slashes — Kryos paths are
  POSIX-style internally, with the runtime translating on Windows
  filesystem calls.
