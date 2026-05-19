# Cookbook 21 · Path manipulation

`std::pathext` adds normalize, component-counting, and absolute-path
detection on top of the core `std::path` join/dirname/basename helpers.
All ops are pure string manipulation — no syscalls.

## Examples

```kryos
use std::pathext::{path_is_absolute, path_normalize, path_component_count}

@capabilities(io)
fn main() {
    // Detect absolute paths
    println(to_string(path_is_absolute("/etc/hosts")))    // 1
    println(to_string(path_is_absolute("relative/path"))) // 0

    // Normalize
    println(path_normalize("a/b/../c"))     // → a/c
    println(path_normalize("a//b/./c"))     // → a/b/c
    println(path_normalize("/a/b/../../c")) // → /c
    println(path_normalize("./foo/bar"))    // → foo/bar
    println(path_normalize("a\\b\\c"))      // → a/b/c (Windows ⇒ POSIX)

    // Component count
    println(to_string(path_component_count("a/b/c")))     // 3
    println(to_string(path_component_count("/a/b")))      // 2
    println(to_string(path_component_count("")))          // 0
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
