# std::config

Configuration management: `.env` file loading, environment variable access with defaults and requirements, and bulk config resolution.

Note: The core `env_get` and `env_set` functions are defined in `std::io`. This module adds `env_load`, `env_require`, `env_default`, `env_all`, and `config`.

```kryos
import std::config
```

---

### env_load

`env_load() -> Int`
`env_load(path: String) -> Int`

Load environment variables from a `.env` file. Returns the number of variables loaded.

**Supported `.env` format:**
```
KEY=value
KEY="quoted value"
KEY='single quoted'
# comments are ignored
export KEY=value
```

**Example:**
```kryos
let count = env_load()
print(to_string(count) + " vars loaded from .env")
```

```kryos
env_load("config/production.env")
let db_url = env_get("DATABASE_URL")
```

**Edge cases:**
- Default path is `.env` in the current directory.
- Returns `0` if the file does not exist (does not raise).
- Only relative paths are allowed -- absolute paths and paths containing `..` are rejected for security.
- The `export` prefix is stripped automatically.
- Quoted values have their outer quotes removed.

**See also:** env_require, env_default

---

### env_require

`env_require(key: String) -> String`

Get a required environment variable. Raises a runtime error if the variable is not set.

**Example:**
```kryos
let secret = env_require("JWT_SECRET")
let db_url = env_require("DATABASE_URL")
```

```kryos
// Common pattern: load .env then require critical vars
env_load()
let api_key = env_require("ANTHROPIC_API_KEY")
let stripe_key = env_require("STRIPE_SECRET_KEY")
```

**Edge cases:**
- Raises with a descriptive message naming the missing variable.

**See also:** env_default, env_load

---

### env_default

`env_default(key: String, default: String) -> String`

Get an environment variable with a fallback default value.

**Example:**
```kryos
let port = env_default("PORT", "3000")
let host = env_default("HOST", "0.0.0.0")
let log_level = env_default("LOG_LEVEL", "info")
```

**Edge cases:**
- Both the key and default are coerced to strings.
- Returns the default if the variable is not set (does not distinguish between unset and empty).

**See also:** env_require

---

### env_all

`env_all() -> Map`

Get all environment variables as a map.

**Example:**
```kryos
let all = env_all()
let keys = map_keys(all)
print("Environment has " + to_string(len(keys)) + " variables")
```

**Edge cases:**
- Returns the full process environment, not just variables loaded from `.env`.

---

### config

`config(defaults: Map) -> Map`

Create a configuration map by resolving environment variables against a defaults map. Each key in the defaults map is treated as an environment variable name. If the variable is set, its value is used; otherwise the default is used.

**Example:**
```kryos
env_load()

let cfg = config(map_from(
    "PORT", "8080",
    "DATABASE_URL", "sqlite://:memory:",
    "JWT_SECRET", "",
    "LOG_LEVEL", "info"
))

print(cfg.PORT)          // env var PORT or "8080"
print(cfg.DATABASE_URL)  // env var DATABASE_URL or "sqlite://:memory:"

if cfg.JWT_SECRET == "" {
    print("WARNING: JWT_SECRET not set, using insecure default")
}
```

**Edge cases:**
- The argument must be a map. Raises otherwise.
- All values are coerced to strings.
- This is a convenience wrapper -- equivalent to calling `env_default` for each key.

**See also:** env_load, env_require, env_default
