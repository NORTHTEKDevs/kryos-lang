# std::auth

Authentication utilities: JWT creation/verification and password hashing. Uses HMAC-SHA256 for JWT and PBKDF2-SHA256 for passwords. No external dependencies.

```kryos
import std::auth
```

---

### jwt_sign

`jwt_sign(payload: Map, secret: String) -> String`
`jwt_sign(payload: Map, secret: String, expires_in: Int) -> String`

Create a signed JWT token using HMAC-SHA256. Automatically adds an `iat` (issued-at) claim. Optional third argument sets expiration in seconds from now.

**Example:**
```kryos
let token = jwt_sign(map_from("user_id", 42, "role", "admin"), "my-secret")
print(token)  // eyJhbGciOi...
```

```kryos
// Token that expires in 1 hour
let token = jwt_sign(
    map_from("user_id", 42),
    env_require("JWT_SECRET"),
    3600
)
```

**Edge cases:**
- The payload must be a map. Raises otherwise.
- The `iat` claim is always added (overwriting any existing one).
- The `exp` claim is only added when `expires_in` is provided.

**See also:** jwt_verify, jwt_decode

---

### jwt_verify

`jwt_verify(token: String, secret: String) -> Map | Nil`

Verify a JWT token's signature and expiration. Returns the payload map if valid, `nil` if invalid or expired.

**Example:**
```kryos
let payload = jwt_verify(token, "my-secret")
if payload == nil {
    print("Invalid or expired token")
    exit(1)
}
print("User ID: " + to_string(payload.user_id))
```

**Edge cases:**
- Returns `nil` for malformed tokens (wrong number of segments).
- Returns `nil` if the signature does not match.
- Returns `nil` if the token has an `exp` claim that is in the past.
- Uses constant-time comparison to prevent timing attacks.

**See also:** jwt_sign, jwt_decode

---

### jwt_decode

`jwt_decode(token: String) -> Map | Nil`

Decode a JWT token **without** verifying the signature. For inspection only -- never trust the output for authorization.

**Example:**
```kryos
let payload = jwt_decode(token)
print(payload.exp)  // 1714000000
print(payload.iat)  // 1713996400
```

**Edge cases:**
- Returns `nil` for malformed tokens.
- Does not check expiration or signature.

**See also:** jwt_verify

---

### hash_password

`hash_password(password: String) -> String`
`hash_password(password: String, iterations: Int) -> String`

Hash a password using PBKDF2-SHA256 with a random 32-byte salt. Default iterations: 600,000.

Returns an encoded string in the format: `pbkdf2:<iterations>:<salt_hex>:<hash_hex>`

**Example:**
```kryos
let hashed = hash_password("hunter2")
print(hashed)  // pbkdf2:600000:a1b2c3...:d4e5f6...

// Store hashed in your database
db_execute(db, "INSERT INTO users (email, password) VALUES ($1, $2)", [email, hashed])
```

**Edge cases:**
- Each call produces a different result (random salt).
- Higher iteration counts are more secure but slower.

**See also:** verify_password

---

### verify_password

`verify_password(password: String, hashed: String) -> Bool`

Verify a password against a hash produced by `hash_password`. Returns `true` if the password matches.

**Example:**
```kryos
let row = db_query_one(db, "SELECT password FROM users WHERE email = $1", [email])
if row == nil || !verify_password(input_password, row.password) {
    print("Invalid credentials")
    exit(1)
}
print("Login successful")
```

**Edge cases:**
- Returns `false` (not an error) if the hash format is unrecognized.
- Uses constant-time comparison to prevent timing attacks.

**See also:** hash_password

---

### generate_token

`generate_token() -> String`
`generate_token(length: Int) -> String`

Generate a cryptographically secure random token as a hex string. Default length is 32 bytes (64 hex characters).

**Example:**
```kryos
let session_id = generate_token()
print(len(session_id))  // 64

let short_code = generate_token(8)
print(len(short_code))  // 16
```

**Edge cases:**
- The `length` parameter is the number of random bytes; the returned hex string is twice as long.

**See also:** jwt_sign
