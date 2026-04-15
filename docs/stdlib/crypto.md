# std::crypto

Cryptographic hashing, HMAC, random generation, and timing-safe comparison utilities.

```kryos
use std::crypto
```

---

## Hashing

### sha256

`sha256(input: str) -> str`

Return the SHA-256 hash of `input` as a lowercase hex string (64 characters).

**Example:**
```kryos
use std::crypto

let digest = sha256("hello")
println(digest)
// 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
```

---

### sha512

`sha512(input: str) -> str`

Return the SHA-512 hash of `input` as a lowercase hex string (128 characters).

**Example:**
```kryos
use std::crypto

let digest = sha512("hello")
println(digest)
// 9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043
```

---

## HMAC

### hmac_sha256

`hmac_sha256(key: str, message: str) -> str`

Return the HMAC-SHA256 of `message` using `key` as a lowercase hex string.

**Example:**
```kryos
use std::crypto

let mac = hmac_sha256("secret-key", "data to authenticate")
println(mac)
// lowercase hex string, 64 characters
```

**Use cases:** API request signing, webhook verification, message authentication.

---

## Random Generation

### random_bytes

`random_bytes(n: i64) -> ptr`

Return `n` cryptographically random bytes as a raw pointer. Sourced from the OS entropy pool (`/dev/urandom` on Unix, `BCryptGenRandom` on Windows).

**Note:** The return type is `ptr`. For most use cases, prefer `random_hex` (returns a hex string) or `random_int` (returns a typed integer).

---

### random_hex

`random_hex(n: i64) -> str`

Return `n` cryptographically random bytes encoded as a lowercase hex string. The resulting string is `2 * n` characters long.

**Example:**
```kryos
use std::crypto

let token = random_hex(16)
println(token)   // e.g. "a3f2b19c8d07e654f1c4b2a9e3d08f17"

let key = random_hex(32)
println(len(key))   // 64
```

**Use cases:** Session tokens, API keys, nonces, CSRF tokens.

---

### random_int

`random_int(lo: i64, hi: i64) -> i64`

Return a cryptographically random integer in the range `[lo, hi)` (inclusive lo, exclusive hi).

**Example:**
```kryos
use std::crypto

let roll = random_int(1, 7)   // 1 through 6 inclusive
println(roll)

let pin = random_int(1000, 10000)   // 4-digit PIN
println(pin)
```

**Edge cases:**
- If `lo >= hi`, behavior is undefined. Always pass `lo < hi`.

---

### random_bool

`random_bool() -> bool`

Return a cryptographically random boolean value.

**Example:**
```kryos
use std::crypto

let coin_flip = random_bool()
println(coin_flip)   // true or false with equal probability
```

---

### random_choice

`random_choice(options: [str]) -> str`

Return a cryptographically random element from `options`.

**Example:**
```kryos
use std::crypto

let words = ["alpha", "beta", "gamma", "delta"]
let chosen = random_choice(words)
println(chosen)   // one of the four words, selected at random
```

**Edge cases:**
- Throws if `options` is empty.

---

### shuffle

`shuffle(items: [str]) -> [str]`

Return a new array with the elements of `items` in a cryptographically random order. The original array is not modified.

**Example:**
```kryos
use std::crypto

let deck = ["A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K"]
let shuffled = shuffle(deck)
println(shuffled)   // same 13 elements, random order
```

---

### uuid_v4

`uuid_v4() -> str`

Generate a random UUID (version 4) and return it as a standard formatted string.

**Example:**
```kryos
use std::crypto

let id = uuid_v4()
println(id)   // e.g. "f47ac10b-58cc-4372-a567-0e02b2c3d479"
println(len(id))   // 36
```

**Format:** `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx` where `y` is `8`, `9`, `a`, or `b`.

---

## Timing-Safe Comparison

### constant_time_eq

`constant_time_eq(a: str, b: str) -> bool`

Compare two strings in constant time. Returns `true` if `a` and `b` are identical.

**Example:**
```kryos
use std::crypto

let token_a = "super-secret-token"
let token_b = request_token   // value from user input

if constant_time_eq(token_a, token_b) {
    println("authenticated")
} else {
    println("rejected")
}
```

**Why this matters:** Standard string equality (`==`) short-circuits on the first differing byte, creating a timing side-channel that an attacker can exploit to guess a secret one byte at a time. `constant_time_eq` always takes the same amount of time regardless of where the strings differ, eliminating this leak.

**Use cases:** Token comparison, HMAC verification, password digest comparison.

---

## Complete Example

```kryos
use std::crypto

// Hash a password (use a proper KDF in production -- bcrypt, argon2, etc.)
let password = "hunter2"
let digest = sha256(password)
println(digest)

// Generate a secure session token
let token = random_hex(32)
println(token)   // 64-character hex string

// Verify a webhook signature
fn verify_webhook(payload: str, signature: str, secret: str) -> bool {
    let expected = hmac_sha256(secret, payload)
    return constant_time_eq(expected, signature)
}

// UUID for a new record
let record_id = uuid_v4()
println(record_id)

// Random sampling
let choices = ["red", "green", "blue"]
let color = random_choice(choices)
println(color)
```
