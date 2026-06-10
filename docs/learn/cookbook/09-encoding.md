# Cookbook 09 · Base64 + UUID

`std::base64` and `std::uuid` are the two utilities every web app eventually needs: encode binary for JSON / data URIs, mint unique identifiers for new records.

## The program

```kryos
use std::crypto::{uuid_v4}

// base64_encode and base64_decode are builtins — no import needed.

@capabilities(io, crypto)
fn main() {
    // 1. UUID v4 — fresh ID per record.
    let id1 = uuid_v4()
    let id2 = uuid_v4()
    println("id1: " + id1)
    println("id2: " + id2)
    if id1 == id2 {
        throw "UUIDs collided — RNG is broken"
    }

    // 2. Base64 — encode a binary blob for inline transport.
    let payload = "the quick brown fox jumps over 13 lazy dogs!"
    let encoded = base64_encode(payload)
    println("encoded:   " + encoded)

    let decoded = base64_decode(encoded)
    println("decoded:   " + decoded)

    if decoded != payload {
        throw "base64 round-trip mismatch"
    }

    // 3. Tag every UUID with a base64 prefix for compact loggable IDs.
    let mut i = 0
    while i < 3 {
        let id = uuid_v4()
        let tag = "u:" + base64_encode(id)
        println(tag)
        i = i + 1
    }
}
```

## Run it

```bash
kryos run encoding.kry
```

Output is non-deterministic per run (UUIDs are random):

```
id1: 8c5f4d2e-...
id2: bf8d1a06-...
parsed bytes length: 16
encoded:   dGhlIHF1aWNrIGJyb3duIGZveCBqdW1wcyBvdmVyIDEzIGxhenkgZG9ncyE=
decoded:   the quick brown fox jumps over 13 lazy dogs!
u:OGM1ZjRkMmUtN2Q4MS00ZjBkLWE5YjUtNzE5OWJjOWVmNDAx
u:...
u:...
```

## Things to know

- The UUID v4 random source is splitmix64 over `(nanos XOR counter)` — fine for IDs, not for crypto. If you need cryptographically secure randomness, use `std::crypto::rand_bytes`.
- `uuid_v4()` is in `std::crypto`. There is no `uuid_parse` in the standard library; validate UUID format with a regex if needed.
- `base64_encode` and `base64_decode` are builtins (no import). They use the standard RFC 4648 alphabet (`+ /`). For URL-safe variants (`- _`) substitute the chars after encoding.
- Both functions move bytes through a caller-owned buffer at the FFI layer; you don't have to free anything — the runtime handles cleanup.
