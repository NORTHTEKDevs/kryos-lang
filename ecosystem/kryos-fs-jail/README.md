# kryos-fs-jail

A path-scoped filesystem facade for Kryos. A `Jail` is rooted at a declared
directory; all reads and writes are confined to that root, and any path
containing a `..` segment or starting with an absolute path prefix is rejected
before any IO reaches the filesystem.

The distinctive part: path validation is **pure compute** (`@capabilities()`).
Only the four leaf functions that actually touch the disk carry
`@capabilities(io)`. The security-sensitive logic -- segment scanning, absolute
detection -- is testable without a filesystem and cannot silently acquire IO
capability.

## MVP scope

- `jail(root)` constructs a `Jail`; normalizes root to forward slashes and
  strips trailing slashes.
- `jail_resolve(j, relpath)` validates a relative path and returns the
  absolute path string if safe. Pure compute, no IO.
- `jail_read(j, relpath)`, `jail_write(j, relpath, data)`,
  `jail_exists(j, relpath)`, `jail_mkdir(j, relpath)` -- the IO surface.
  Each calls `jail_resolve` first; a rejected path never reaches a file
  syscall.
- `jail_list(j, relpath)` validates the path (escape attempts rejected) and
  returns `Ok([])`. Full listing is deferred -- see Known Limitations.

Cross-platform: root paths may use `\` or `/`; both are normalized to `/`
internally. Windows drive-letter absolute paths (`C:/...`) are detected and
rejected in `relpath`.

## Out of scope

- Symlink resolution (needs OS-level realpath -- see Known Limitations).
- Read-only vs read-write split (awaiting Kryos sub-capabilities).
- Quota enforcement.
- Full directory listing (`list_dir` has no builtin; `std::fs::list_dir`
  is available on release builds).

## Layout

```
kryos.toml              package manifest, [capabilities] allowed = ["io", "process"]
src/jail.kry            Jail struct, validation helpers, IO operations
tests/test_jail.kry     13 @test functions (pure compute) + IO round-trip in main()
demo_jail.kry           live demo: writes, reads, escape attempts
```

## Using the jail

```kryos
use std::result::{Result, Ok, Err}
use jail

fn main() {
    let j = jail("/srv/user-uploads")

    // Write a file inside the jail.
    match jail_write(j, "profile.txt", "data") {
        Result::Ok(_) => println("written"),
        Result::Err(e) => println("blocked: " + e),
    }

    // Escape attempt -- always Err.
    match jail_read(j, "../../etc/passwd") {
        Result::Ok(_) => println("BUG"),
        Result::Err(e) => println(e),  // jail: path traversal (..) not allowed
    }
}
```

`jail_resolve` is the pure gateway. Call it directly when you need the
absolute path for a downstream operation (e.g., passing to `std::fs::list_dir`
on a release build):

```kryos
match jail_resolve(j, relpath) {
    Result::Ok(abs) => { /* safe to use abs */ },
    Result::Err(e)  => { /* blocked */ },
}
```

## Running

From the repo root:

```
kryos test --path ecosystem/kryos-fs-jail
kryos run ecosystem/kryos-fs-jail/tests/test_jail.kry   # also runs IO round-trip
kryos run ecosystem/kryos-fs-jail/demo_jail.kry
```

## Known Limitations

**Symlink escapes.** This library enforces path-string rules, not OS-level
containment. A symlink inside the jail root pointing outside it would bypass
the check. Without OS-level `realpath(3)`, symlink attacks cannot be
prevented at the string level. Document this to callers and never claim a
security boundary against a hostile filesystem administrator.

**Directory listing stub.** `jail_list` validates the path but returns an
empty array. There is no `list_dir` builtin. On a release build
(`kryos build --release`), call `std::fs::list_dir(abs)` after validating
with `jail_resolve`.

## Notes

- All path forms are long flags (no short forms). Absolute paths starting
  with `/`, `\`, or a Windows drive letter (`C:`) are rejected.
- Error messages are fixed prefix strings so callers can pattern-match on
  `"jail: path traversal"` and `"jail: absolute path"`.
- The library is licensed Apache-2.0 (see `LICENSE`).
