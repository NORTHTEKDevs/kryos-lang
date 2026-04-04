# std::crypto

Cryptographic hashing, HMAC, encoding, and random generation utilities.

All functions in this module are available after `use std::crypto`. All hash and encode functions operate on the UTF-8 byte representation of their input strings.

---

## Hashing

### sha256

```
sha256(data: str) -> str
```

Compute the SHA-256 hash of a string. Returns the hash as a 64-character lowercase hex string.

**Example:**

```kryos
let hash = sha256("hello")
println(hash)  // "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
```

**Edge cases:**

- Input is converted to a string with `to_string()` before hashing if it is not already a string.
- Empty string produces the SHA-256 of zero bytes: `"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"`.

**See also:** `sha512`, `hmac_sha256`

---

### sha512

```
sha512(data: str) -> str
```

Compute the SHA-512 hash of a string. Returns the hash as a 128-character lowercase hex string.

**Example:**

```kryos
let hash = sha512("hello")
println(hash)
```

**Edge cases:**

- Same conversion behavior as `sha256`.

**See also:** `sha256`

---

### md5

```
md5(data: str) -> str
```

Compute the MD5 hash of a string. Returns the hash as a 32-character lowercase hex string.

**Example:**

```kryos
let hash = md5("hello")
println(hash)  // "5d41402abc4b2a76b9719d911017c592"
```

**Edge cases:**

- MD5 is cryptographically broken. Do not use for security purposes. Use `sha256` or `sha512` instead.
- Acceptable for checksums, cache keys, and non-security fingerprinting.

**See also:** `sha256`

---

### hmac_sha256

```
hmac_sha256(key: str, data: str) -> str
```

Compute an HMAC-SHA256 message authentication code. Returns the HMAC as a 64-character lowercase hex string.

**Example:**

```kryos
let signature = hmac_sha256("my_secret_key", "message to sign")
println(signature)
```

```kryos
// Verify a webhook signature
let expected = hmac_sha256(env_get("WEBHOOK_SECRET"), request_body)
if expected == received_signature {
    println("Signature valid")
}
```

**Edge cases:**

- Both `key` and `data` are converted to strings and encoded as UTF-8.

**See also:** `sha256`

---

## Encoding

### base64_encode

```
base64_encode(data: str) -> str
```

Encode a string as Base64.

**Example:**

```kryos
let encoded = base64_encode("Hello, World!")
println(encoded)  // "SGVsbG8sIFdvcmxkIQ=="
```

**See also:** `base64_decode`

---

### base64_decode

```
base64_decode(data: str) -> str
```

Decode a Base64-encoded string back to the original UTF-8 string.

**Example:**

```kryos
let decoded = base64_decode("SGVsbG8sIFdvcmxkIQ==")
println(decoded)  // "Hello, World!"
```

**Edge cases:**

- Throws a runtime error if the input is not valid Base64.
- The decoded bytes are interpreted as UTF-8. Throws if they are not valid UTF-8.

**See also:** `base64_encode`

---

### hex_encode

```
hex_encode(data: str) -> str
```

Encode a string as a hexadecimal string. Each byte becomes two hex characters.

**Example:**

```kryos
let hex = hex_encode("AB")
println(hex)  // "4142"
```

**See also:** `hex_decode`

---

### hex_decode

```
hex_decode(data: str) -> str
```

Decode a hexadecimal string back to the original UTF-8 string.

**Example:**

```kryos
let original = hex_decode("4142")
println(original)  // "AB"
```

**Edge cases:**

- Throws a runtime error if the input contains non-hex characters or has an odd length.
- Throws if the decoded bytes are not valid UTF-8.

**See also:** `hex_encode`

---

## Random

### random_bytes

```
random_bytes(n: i32) -> str
```

Generate `n` cryptographically secure random bytes, returned as a hex string (2 characters per byte, so the result is `2 * n` characters long).

**Example:**

```kryos
let token = random_bytes(16)
println(token)  // e.g. "a3f2b8c91d4e7f0612ab34cd56ef7890"
println(len(token))  // 32
```

```kryos
// Generate a 256-bit secret key
let key = random_bytes(32)
```

**Edge cases:**

- Uses a cryptographically secure random number generator.
- `n` must be a positive integer.

**See also:** `uuid`

---

### uuid

```
uuid() -> str
```

Generate a random UUID v4 string in the standard `8-4-4-4-12` format.

**Example:**

```kryos
let id = uuid()
println(id)  // e.g. "550e8400-e29b-41d4-a716-446655440000"
```

```kryos
// Use as a unique identifier
let session_id = uuid()
file_write("sessions/" + session_id + ".json", json_stringify({"user": "alice"}))
```

**Edge cases:**

- Each call returns a new unique identifier.
- The output follows RFC 4122 UUID v4 format.

**See also:** `random_bytes`
