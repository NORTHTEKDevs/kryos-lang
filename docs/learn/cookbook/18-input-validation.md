# Cookbook 18 · Validating user input

Don't trust input. Whether it comes from CLI args, env vars, files, or HTTP, validate at the boundary and reject the rest with a clear message.

## The program

```kryos
use std::re::{regex_new, regex_is_match, regex_drop}

@capabilities(io)
fn main() {
    let argv = args()
    if len(argv) < 3 {
        println("usage: validate <email> <port>")
        return
    }
    let email = argv[1]
    let port_str = argv[2]

    match validate_email(email) {
        true => println("✓ email ok"),
        false => println("✗ email invalid: " + email),
    }

    let port = validate_port(port_str)
    if port < 0 {
        println("✗ port invalid: " + port_str)
    } else {
        println("✓ port ok: " + to_string(port))
    }
}

fn validate_email(s: str) -> bool {
    // Conservative regex — won't catch every weird-but-valid RFC 5322
    // address, but rejects obvious garbage.
    let re = regex_new("^[a-zA-Z0-9._+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$")
    let ok = regex_is_match(re, s) == 1
    regex_drop(re)
    return ok
}

fn validate_port(s: str) -> i64 {
    // Must be 1-65535. parse_int panics on non-numeric, so pre-check.
    let n = len(s)
    if n == 0 || n > 5 { return -1 }
    let mut i: i64 = 0
    while i < n {
        let c = char_code(substr(s, i, i + 1))
        if c < 48 || c > 57 { return -1 }
        i = i + 1
    }
    let v = parse_int(s)
    if v < 1 || v > 65535 { return -1 }
    return v
}
```

## Patterns

- **Reject early.** Validate as soon as the value enters your code, not
  three function calls deeper.
- **Return Option / Result, not -1.** The `validate_port` above uses
  `-1` as sentinel; in production, return `Option<i64>` and let the
  caller pattern-match.
- **Bound input size.** Reject any string > expected max length before
  parsing. `parse_int` on a giant string wastes cycles.
- **Don't roll your own crypto / URL / email regex** for security-critical
  paths. Use `std::crypto`, `std::http`, etc. The validator above is fine
  for "is this even close to an email", not for verifying ownership.
