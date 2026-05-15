# regex

Regex matching for Kryos via POSIX `regcomp`/`regexec` (from libc), with a pure-Kryos wildcard fallback.

## Installation

```
kryos pkg add regex
```

## Usage

```kryos
import "regex"

fn main() {
    println(regex_match("[0-9]+", "abc123"))          // true
    println(regex_match("^hello", "hello world"))     // true
    println(regex_match("^hello", "say hello"))       // false
    println(regex_find("world", "hello world"))       // 6
    println(regex_find("missing", "hello world"))     // -1
}
```

## API

- `fn regex_match(pattern: str, text: str) -> bool` — true if pattern matches anywhere in text
- `fn regex_find(pattern: str, text: str) -> i64` — byte offset of first match, or -1

## Pattern syntax

When libc is available (Linux/macOS), full POSIX Extended Regular Expressions (ERE) are supported:

```
.  *  +  ?  []  ()  |  {}  ^  $  [^...]  \d  \w  etc.
```

When libc is unavailable, the fallback uses simple glob patterns:
- `*` matches any sequence of characters
- All other characters match literally (substring match)

## License

MIT — see https://github.com/NORTHTEKDevs/kryos-registry
