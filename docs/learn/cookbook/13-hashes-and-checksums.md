# Cookbook 13 · Hashes and checksums

`std::hash` ships three non-cryptographic hashes: **FNV-1a 64** (fast general-purpose), **DJB2** (string), and **CRC32 IEEE** (zip/png/ethernet). For cryptographic hashes (SHA-256, blake3), use `std::crypto`.

## The program

```kryos
use std::hash::{fnv1a64, djb2, crc32}

@capabilities(io)
fn main() {
    let input = "the quick brown fox"

    println("FNV-1a 64: " + to_string(fnv1a64(input)))
    println("DJB2:      " + to_string(djb2(input)))
    println("CRC32:     " + to_hex(crc32(input)))

    // Content-id pattern: derive a short stable ID from a value.
    let id = "doc-" + to_hex(fnv1a64(input))
    println("doc id:    " + id)
}

fn to_hex(n: i64) -> str {
    // Convert i64 to a 16-char hex string (all 64 bits; CRC32's upper
    // 32 bits are always zero, so its hex form is zero-padded on the left).
    let digits: [str] = ["0","1","2","3","4","5","6","7","8","9","a","b","c","d","e","f"]
    let mut out: str = ""
    let mut shift: i64 = 60
    while shift >= 0 {
        let nibble = (n >> shift) & 15
        out = out + digits[nibble]
        shift = shift - 4
    }
    return out
}
```

## When to pick which

- **FNV-1a 64** — default for hash maps, bloom filters, content addresses.
- **DJB2** — legacy interop only. Reasonable, but FNV-1a usually wins.
- **CRC32** — only when you specifically need IEEE checksum compatibility
  (zip/png/ethernet frames). Slow vs FNV-1a; not collision-resistant
  enough for general hashing.
- **None of these are cryptographic.** For password hashing or signing,
  use `std::crypto` with argon2/sha256/blake3.
