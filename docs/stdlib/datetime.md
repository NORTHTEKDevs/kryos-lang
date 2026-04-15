# std::datetime

Date, time, duration, and instant measurement. Built around three types: `Duration` (a span of time), `Instant` (a point in time), and `DateTime` (a calendar-aware timestamp).

```kryos
use std::datetime
```

---

## Duration

A span of time. Use `Duration` to express delays, timeouts, and measured intervals.

### Duration.from_millis

`Duration.from_millis(ms: i64) -> Duration`

Create a `Duration` from a number of milliseconds.

**Example:**
```kryos
use std::datetime

let timeout = Duration.from_millis(500)
```

---

### Duration.from_secs

`Duration.from_secs(secs: i64) -> Duration`

Create a `Duration` from a number of seconds.

**Example:**
```kryos
use std::datetime

let one_second = Duration.from_secs(1)
let five_minutes = Duration.from_secs(300)
```

---

### Duration.from_mins

`Duration.from_mins(mins: i64) -> Duration`

Create a `Duration` from a number of minutes.

**Example:**
```kryos
use std::datetime

let half_hour = Duration.from_mins(30)
```

---

### Duration.from_hours

`Duration.from_hours(hours: i64) -> Duration`

Create a `Duration` from a number of hours.

**Example:**
```kryos
use std::datetime

let one_day = Duration.from_hours(24)
```

---

### Duration.zero

`Duration.zero() -> Duration`

Create a zero-length `Duration`.

**Example:**
```kryos
use std::datetime

let d = Duration.zero()
```

---

## Instant

A point in time. Use `Instant` for timestamps, sleeping, and benchmarking code.

### Instant.now

`Instant.now() -> Instant`

Capture the current moment as an `Instant`.

**Example:**
```kryos
use std::datetime

let start = Instant.now()
```

---

### Instant.timestamp

`Instant.timestamp() -> i64`

Return the Unix timestamp (seconds since 1970-01-01 00:00:00 UTC) for this instant.

**Example:**
```kryos
use std::datetime

let now = Instant.now()
println(now.timestamp())   // e.g. 1713110400
```

---

### Instant.timestamp_millis

`Instant.timestamp_millis() -> i64`

Return the Unix timestamp in milliseconds for this instant.

**Example:**
```kryos
use std::datetime

let now = Instant.now()
println(now.timestamp_millis())   // e.g. 1713110400000
```

---

### Instant.measure

`Instant.measure(f: fn) -> Duration`

Call `f` with no arguments and return the `Duration` it took to execute.

**Example:**
```kryos
use std::datetime

let elapsed = Instant.measure(fn() {
    let sum = 0
    let i = 0
    while i < 1000000 {
        sum = sum + i
        i = i + 1
    }
})
println(elapsed.from_millis(0))   // Duration of the loop
```

**Note:** `f` must take no arguments and may return any value; the return value is discarded.

---

### Instant.sleep

`Instant.sleep(d: Duration)`

Pause execution for the duration `d`.

**Example:**
```kryos
use std::datetime

Instant.sleep(Duration.from_millis(500))   // sleep 500ms
Instant.sleep(Duration.from_secs(2))       // sleep 2 seconds
```

---

### Instant.sleep_millis

`Instant.sleep_millis(ms: i64)`

Pause execution for `ms` milliseconds. Convenience wrapper over `Instant.sleep`.

**Example:**
```kryos
use std::datetime

Instant.sleep_millis(100)
```

---

### Instant.sleep_secs

`Instant.sleep_secs(secs: i64)`

Pause execution for `secs` seconds. Convenience wrapper over `Instant.sleep`.

**Example:**
```kryos
use std::datetime

Instant.sleep_secs(5)
```

---

## DateTime

A UTC-anchored calendar date and time. Use `DateTime` when you need human-readable timestamps, ISO 8601 formatting, or working from Unix epoch values.

### DateTime.now_utc

`DateTime.now_utc() -> DateTime`

Return a `DateTime` representing the current moment in UTC.

**Example:**
```kryos
use std::datetime

let now = DateTime.now_utc()
```

---

### DateTime.from_timestamp

`DateTime.from_timestamp(ts: i64) -> DateTime`

Construct a `DateTime` from a Unix timestamp (seconds since epoch).

**Example:**
```kryos
use std::datetime

let dt = DateTime.from_timestamp(1713110400)
```

---

### DateTime.now_iso

`DateTime.now_iso() -> str`

Return the current UTC time as an ISO 8601 string.

**Example:**
```kryos
use std::datetime

let iso = DateTime.now_iso()
println(iso)   // e.g. "2024-04-14T16:00:00Z"
```

---

### DateTime.format_timestamp

`DateTime.format_timestamp(ts: i64, fmt: str) -> str`

Format a Unix timestamp using a format string. Format tokens follow `strftime` conventions.

**Common tokens:**

| Token | Meaning                    | Example   |
|-------|----------------------------|-----------|
| `%Y`  | 4-digit year               | `2024`    |
| `%m`  | 2-digit month (01-12)      | `04`      |
| `%d`  | 2-digit day (01-31)        | `14`      |
| `%H`  | Hour, 24-hour (00-23)      | `16`      |
| `%M`  | Minute (00-59)             | `30`      |
| `%S`  | Second (00-59)             | `05`      |
| `%A`  | Full weekday name          | `Sunday`  |
| `%B`  | Full month name            | `April`   |

**Example:**
```kryos
use std::datetime

let ts = Instant.now().timestamp()
let date_str = DateTime.format_timestamp(ts, "%Y-%m-%d")
println(date_str)   // e.g. "2024-04-14"

let full = DateTime.format_timestamp(ts, "%A, %B %d %Y at %H:%M")
println(full)   // e.g. "Sunday, April 14 2024 at 16:30"
```

---

## Complete Example

```kryos
use std::datetime

// Benchmark a block of code
let elapsed = Instant.measure(fn() {
    Instant.sleep_millis(50)
})

// Get current timestamps
let now = Instant.now()
println(now.timestamp())          // Unix seconds
println(now.timestamp_millis())   // Unix milliseconds

// Format a date
let formatted = DateTime.format_timestamp(now.timestamp(), "%Y-%m-%d %H:%M:%S")
println(formatted)   // e.g. "2024-04-14 16:30:00"

// ISO string
println(DateTime.now_iso())   // "2024-04-14T16:30:00Z"

// Sleep
println("starting...")
Instant.sleep(Duration.from_secs(1))
println("done after 1 second")
```
