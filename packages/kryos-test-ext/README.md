# kryos-test-ext

Extra assertion helpers for `kryos test`. Wraps `throw` with helpful messages so failed tests report what they expected vs what they got.

## Install

```bash
kryos pkg add kryos-test-ext
```

## Use

```kryos
use kryos_test_ext::{assert_eq_i64, assert_eq_str, assert_lt, assert_contains_str, assert_msg}

@test
fn arithmetic_works() {
    assert_eq_i64(2 + 2, 4)
}

@test
fn string_match() {
    assert_eq_str("hello".to_uppercase(), "HELLO")
}
```

## API

| Function | Behavior |
| --- | --- |
| `assert_eq_i64(actual, expected)` | Panics with `"expected X, got Y"` on mismatch |
| `assert_eq_str(actual, expected)` | Panics with `"expected \`X\`, got \`Y\`"` on mismatch |
| `assert_lt(a, b)` | Panics if `!(a < b)` |
| `assert_contains_str(haystack, needle)` | Panics if `haystack` doesn't contain `needle` |
| `assert_msg(cond, msg)` | Panics with `msg` if `cond` is false |
