# std::db

SQLite database access. `std::db` wraps the `kryos-stdlib-native` SQLite FFI
layer in a high-level connection / cursor API. Every function that touches the
database requires the `db` capability.

```kryos
use std::db
```

> **Scope:** this module is a SQLite driver only — open a file (or in-memory)
> database, run SQL, and iterate result rows. There is no query builder, ORM,
> connection pool, or other database backend. Statements are passed as raw SQL
> strings; bind values by composing the SQL string (no parameter binding API is
> exposed yet, so escape untrusted input yourself).

---

## Types

```kryos
struct Connection { handle: i64 }
struct Cursor { handle: i64, done: bool }
```

`Connection` is an open database handle. `Cursor` is an in-progress result set
returned by `query`.

---

## Connection

### open

`open(path: str) -> Connection` · `@capabilities(db)`

Open the SQLite database file at `path`, creating it if it does not exist.
Throws on failure.

### open_memory

`open_memory() -> Connection` · `@capabilities(db)`

Open a temporary in-memory database. Useful for tests.

### close

`close(conn: Connection)` · `@capabilities(db)`

Close a connection.

---

## Executing statements

### execute

`execute(conn: Connection, sql: str) -> i64` · `@capabilities(db)`

Run a DDL/DML statement (`CREATE`, `INSERT`, `UPDATE`, `DELETE`, ...). Returns
the number of rows changed. Throws on SQL error.

### exec_multi

`exec_multi(conn: Connection, sql: str) -> i64` · `@capabilities(db)`

Run multiple `;`-separated statements (e.g. a migration script) by splitting on
`;` and calling `execute` for each. Returns the total rows affected. This is a
simple textual split — it does not understand semicolons inside string
literals.

---

## Querying

### query

`query(conn: Connection, sql: str) -> Cursor` · `@capabilities(db)`

Run a `SELECT` and return a `Cursor` over the result rows. Advance with `step`,
read columns with `col_int` / `col_text`, and call `finalize` when done.

### step

`step(cursor: Cursor) -> bool` · `@capabilities(db)`

Advance to the next row. Returns `true` if a row is available, `false` when the
result set is exhausted.

### col_count

`col_count(cursor: Cursor) -> i32`

Number of columns in the result set.

### col_int

`col_int(cursor: Cursor, col: i32) -> i64`

Read an integer (or real) column from the current row (0-indexed).

### col_text

`col_text(cursor: Cursor, col: i32) -> str`

Read a text column from the current row (0-indexed). Returns `""` for empty or
null values.

### finalize

`finalize(cursor: Cursor)`

Free a cursor and release its memory. Always call this when done iterating.

### query_flat

`query_flat(conn: Connection, sql: str, sep: str) -> [str]` · `@capabilities(db)`

Convenience wrapper: run a `SELECT` and return all rows as a flat `[str]`, with
each row's columns joined by `sep`. Handles `step`/`finalize` for you. Good for
quick inspection.

---

## Example

```kryos
use std::db

fn main() {
    let conn = db::open_memory()
    db::execute(conn, "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
    db::execute(conn, "INSERT INTO users (name) VALUES ('Alice')")
    db::execute(conn, "INSERT INTO users (name) VALUES ('Bob')")

    let cursor = db::query(conn, "SELECT id, name FROM users")
    while db::step(cursor) {
        let id   = db::col_int(cursor, 0)
        let name = db::col_text(cursor, 1)
        println(to_string(id) + " " + name)
    }
    db::finalize(cursor)
    db::close(conn)
}
```
