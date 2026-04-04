# std::datetime

Date, time, and timestamp functions. All timestamps are Unix seconds (float) unless otherwise noted. Internal times are UTC.

```kryos
import std::datetime
```

---

### now

`now() -> Float`

Current Unix timestamp in seconds (with fractional milliseconds).

**Example:**
```kryos
let start = now()
sleep(1)
let elapsed = now() - start
print("Took " + to_string(elapsed) + " seconds")
```

**See also:** now_ms, now_iso

---

### now_ms

`now_ms() -> Int`

Current Unix timestamp in milliseconds.

**Example:**
```kryos
let id = "evt_" + to_string(now_ms())
print(id)  // evt_1714000000000
```

**See also:** now

---

### now_iso

`now_iso() -> String`

Current time as an ISO 8601 string in UTC.

**Example:**
```kryos
print(now_iso())  // 2026-04-01T12:00:00.000000+00:00
```

**See also:** now_local

---

### now_local

`now_local() -> String`

Current local time as an ISO 8601 string (uses the system timezone).

**Example:**
```kryos
print(now_local())  // 2026-04-01T04:00:00.000000
```

**See also:** now_iso

---

### timestamp_to_iso

`timestamp_to_iso(timestamp: Float) -> String`

Convert a Unix timestamp to an ISO 8601 string (UTC).

**Example:**
```kryos
let iso = timestamp_to_iso(1714000000)
print(iso)  // 2024-04-25T02:26:40+00:00
```

**See also:** iso_to_timestamp

---

### iso_to_timestamp

`iso_to_timestamp(iso_string: String) -> Float`

Convert an ISO 8601 string to a Unix timestamp.

**Example:**
```kryos
let ts = iso_to_timestamp("2024-04-25T02:26:40+00:00")
print(ts)  // 1714000000.0
```

**Edge cases:**
- Handles the `Z` suffix by converting it to `+00:00`.
- Raises on unparseable date strings.

**See also:** timestamp_to_iso

---

### format_date

`format_date(timestamp: Float, format: String) -> String`

Format a Unix timestamp using strftime format codes. Output is in UTC.

**Common format codes:**
| Code | Meaning | Example |
|------|---------|---------|
| `%Y` | 4-digit year | 2026 |
| `%m` | Month (01-12) | 04 |
| `%d` | Day (01-31) | 01 |
| `%H` | Hour (00-23) | 14 |
| `%M` | Minute (00-59) | 30 |
| `%S` | Second (00-59) | 00 |
| `%A` | Weekday name | Tuesday |
| `%B` | Month name | April |

**Example:**
```kryos
let ts = now()
print(format_date(ts, "%Y-%m-%d"))           // 2026-04-01
print(format_date(ts, "%B %d, %Y"))          // April 01, 2026
print(format_date(ts, "%Y-%m-%d %H:%M:%S"))  // 2026-04-01 14:30:00
```

**See also:** parse_date

---

### parse_date

`parse_date(date_string: String, format: String) -> Float`

Parse a date string using strftime format codes. Returns a Unix timestamp. The parsed datetime is treated as local time (no timezone).

**Example:**
```kryos
let ts = parse_date("2026-04-01", "%Y-%m-%d")
print(format_date(ts, "%A"))  // day of week
```

```kryos
let ts = parse_date("April 01, 2026 14:30", "%B %d, %Y %H:%M")
```

**Edge cases:**
- Raises on format mismatches.
- The result is a local-time timestamp (not UTC-adjusted).

**See also:** format_date

---

### date_add

`date_add(timestamp: Float, seconds: Float) -> Float`

Add seconds to a timestamp. Use negative values for subtraction.

**Example:**
```kryos
let tomorrow = date_add(now(), duration(1, "d"))
let yesterday = date_add(now(), -duration(1, "d"))
print(format_date(tomorrow, "%Y-%m-%d"))
```

**See also:** date_diff, duration

---

### date_diff

`date_diff(timestamp1: Float, timestamp2: Float) -> Float`

Difference between two timestamps in seconds. Result is `timestamp1 - timestamp2`.

**Example:**
```kryos
let start = now()
// ... do work ...
let elapsed = date_diff(now(), start)
print("Took " + to_string(elapsed) + " seconds")
```

```kryos
let a = parse_date("2026-04-01", "%Y-%m-%d")
let b = parse_date("2026-03-01", "%Y-%m-%d")
let days = date_diff(a, b) / duration(1, "d")
print(to_string(days) + " days apart")  // 31.0 days apart
```

**See also:** date_add

---

### date_parts

`date_parts(timestamp: Float) -> Map`

Break a timestamp into its component parts (UTC).

**Returned map fields:**
| Field | Type | Description |
|-------|------|-------------|
| `year` | Int | 4-digit year |
| `month` | Int | 1-12 |
| `day` | Int | 1-31 |
| `hour` | Int | 0-23 |
| `minute` | Int | 0-59 |
| `second` | Int | 0-59 |
| `weekday` | Int | 0 (Monday) - 6 (Sunday) |
| `day_of_year` | Int | 1-366 |

**Example:**
```kryos
let parts = date_parts(now())
print("Year: " + to_string(parts.year))
print("Month: " + to_string(parts.month))

if parts.weekday >= 5 {
    print("It's the weekend!")
}
```

---

### duration

`duration(value: Float, unit: String) -> Float`

Convert a duration to seconds. Useful with `date_add` and `sleep`.

**Supported units:**
| Unit | Meaning |
|------|---------|
| `"s"` | Seconds |
| `"m"` | Minutes |
| `"h"` | Hours |
| `"d"` | Days |
| `"w"` | Weeks |

**Example:**
```kryos
print(duration(1, "h"))   // 3600.0
print(duration(7, "d"))   // 604800.0
print(duration(2, "w"))   // 1209600.0

sleep(duration(500, "s") / 1000)  // sleep 0.5 seconds (use math)

let expires = date_add(now(), duration(30, "d"))
```

**Edge cases:**
- Raises on unknown unit strings.

**See also:** date_add, sleep
