# std::db

Database connectivity for SQLite (built-in) and PostgreSQL (requires `psycopg2-binary`). Uses connection handles for resource management.

```kryos
import std::db
```

---

### db_connect

`db_connect(url: String) -> Int`

Open a database connection. Returns an integer handle used by all other `db_*` functions.

**Supported URL schemes:**
| Scheme | Description |
|--------|-------------|
| `sqlite://:memory:` | In-memory SQLite database |
| `sqlite:///path/to/file.db` | File-backed SQLite database |
| `postgres://user:pass@host/dbname` | PostgreSQL connection |
| `postgresql://user:pass@host/dbname` | PostgreSQL (alias) |

**Example:**
```kryos
// In-memory SQLite
let db = db_connect("sqlite://:memory:")

// File-backed SQLite
let db = db_connect("sqlite:///data/app.db")

// PostgreSQL
let db = db_connect(env_require("DATABASE_URL"))
```

**Edge cases:**
- SQLite connections automatically enable WAL mode and foreign keys.
- PostgreSQL connections have autocommit disabled (use `db_execute` which commits after each statement).
- Raises if `psycopg2` is not installed when connecting to PostgreSQL.
- Raises on unrecognized URL schemes.

**See also:** db_close

---

### db_query

`db_query(handle: Int, sql: String) -> Array`
`db_query(handle: Int, sql: String, params: Array) -> Array`

Execute a SELECT query and return all rows as an array of maps. Each map has column names as keys.

**Example:**
```kryos
let db = db_connect("sqlite://:memory:")
db_execute(db, "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INT)")
db_execute(db, "INSERT INTO users (name, age) VALUES ($1, $2)", ["Alice", 30])
db_execute(db, "INSERT INTO users (name, age) VALUES ($1, $2)", ["Bob", 25])

let users = db_query(db, "SELECT * FROM users WHERE age > $1", [20])
for user in users {
    print(user.name + " is " + to_string(user.age))
}
// Alice is 30
// Bob is 25
```

**Edge cases:**
- Parameters use `$1`, `$2`, ... placeholder syntax (automatically rewritten to `?` for SQLite).
- Returns an empty array if no rows match.
- Column names come from the query (use `AS` aliases to control them).

**See also:** db_query_one, db_execute

---

### db_query_one

`db_query_one(handle: Int, sql: String) -> Map | Nil`
`db_query_one(handle: Int, sql: String, params: Array) -> Map | Nil`

Execute a SELECT query and return the first row as a map, or `nil` if no rows match.

**Example:**
```kryos
let user = db_query_one(db, "SELECT * FROM users WHERE id = $1", [42])
if user == nil {
    print("User not found")
} else {
    print("Found: " + user.name)
}
```

**Edge cases:**
- Fetches all rows internally, then returns the first. For large result sets, add `LIMIT 1` to the query.

**See also:** db_query

---

### db_execute

`db_execute(handle: Int, sql: String) -> Int`
`db_execute(handle: Int, sql: String, params: Array) -> Int`

Execute a non-SELECT statement (INSERT, UPDATE, DELETE, CREATE, etc.). Returns the number of rows affected. Commits automatically.

**Example:**
```kryos
db_execute(db, "CREATE TABLE posts (id INTEGER PRIMARY KEY, title TEXT, body TEXT)")

let affected = db_execute(db, "UPDATE users SET active = $1 WHERE last_login < $2", [false, cutoff])
print(to_string(affected) + " users deactivated")
```

```kryos
// Parameterized insert
db_execute(db, "INSERT INTO posts (title, body) VALUES ($1, $2)", ["Hello", "World"])
```

**Edge cases:**
- Automatically commits after execution (no explicit transaction management needed for single statements).
- Use `$1`, `$2`, ... for parameters. Never concatenate user input into SQL strings.

**See also:** db_query

---

### db_close

`db_close(handle: Int) -> Nil`

Close a database connection and release the handle.

**Example:**
```kryos
let db = db_connect("sqlite://:memory:")
// ... use the database ...
db_close(db)
```

**Edge cases:**
- Safe to call on an already-closed handle (silently ignored).
- After closing, any further operations on the handle will raise an error.

**See also:** db_connect
