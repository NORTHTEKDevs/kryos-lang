# sqlite

FFI wrapper for SQLite via `libsqlite3` for Kryos.

## Installation

```
kryos pkg add sqlite
```

Requires `libsqlite3.so.0` to be installed on the system (standard on most Linux distributions). Install with:

```sh
apt-get install libsqlite3-dev   # Debian/Ubuntu
dnf install sqlite-devel         # Fedora/RHEL
```

## Usage

```kryos
import "sqlite"

fn main() {
    // Open an in-memory database (pass a file path for persistent storage)
    let db = sqlite_open(":memory:")
    if db == 0 {
        println("failed to open sqlite database")
        return
    }

    // Create a table
    let rc = sqlite_exec(db, "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
    if rc != 0 { println("CREATE TABLE failed") }

    // Insert rows
    sqlite_exec(db, "INSERT INTO users (name) VALUES ('alice')")
    sqlite_exec(db, "INSERT INTO users (name) VALUES ('bob')")

    // Close the database
    sqlite_close(db)
    println("done")
}
```

## API

- `fn sqlite_open(path: str) -> i64` — open (or create) a database; returns handle or 0 on failure
- `fn sqlite_exec(h: i64, sql: str) -> i64` — execute SQL statements; returns 0 on success, -1 on error
- `fn sqlite_close(h: i64)` — close the database

## Roadmap

- v0.2.0: `sqlite_query` with row-by-row reading via `sqlite3_prepare_v2` / `sqlite3_step` / `sqlite3_column_text`

## License

MIT — see https://github.com/NORTHTEKDevs/kryos-registry
