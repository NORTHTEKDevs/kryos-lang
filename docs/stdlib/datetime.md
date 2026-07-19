# std::datetime

Date, time, duration, and instant measurement. Built around three types: `Duration` (a span of time), `Instant` (a point in time), and `DateTime` (a calendar-aware UTC timestamp).

All constructors and clock functions are **free functions** (`from_secs(90)`, `now()`, `from_timestamp(ts)`), not static methods -- there is no `Duration.from_secs(...)` / `Instant.now()` call form. Methods exist only on values (`d.as_secs()`, `t.elapsed()`).

```kryos
use std::datetime
```

---

## Duration

A span of time, stored with millisecond precision. Use `Duration` to express delays, timeouts, and measured intervals.

### Constructors (free functions)

```
from_millis(ms: i64) -> Duration
from_secs(secs: i64) -> Duration
from_mins(mins: i64) -> Duration
from_hours(hours: i64) -> Duration
zero() -> Duration
```

**Example:**
```kryos
use std::datetime

fn main() {
    let timeout = from_millis(500)
    let half_hour = from_mins(30)
    print("{timeout.as_millis()} {half_hour.as_secs()}")  // 500 1800
}
```

---

### Duration methods

`d.as_secs() -> i64` -- whole seconds in the span (`from_secs(90).as_secs()` is `90`).

`d.as_millis() -> i64` -- total milliseconds (`from_secs(90).as_millis()` is `90000`).

`d.subsec_millis() -> i64` -- milliseconds beyond the whole seconds (`from_millis(1500).subsec_millis()` is `500`).

`d.add(other: Duration) -> Duration`, `d.sub(other: Duration) -> Duration`, `d.mul(factor: i64) -> Duration` -- arithmetic (`from_secs(60).add(from_secs(30)).as_secs()` is `90`).

`d.is_zero() -> bool` -- true when the span is exactly zero (`zero().is_zero()` is `true`).

`d.format() -> str` -- human-readable rendering (`from_secs(90).format()` is `"1m30s"`).

---

## Instant

An opaque point in time captured from the system clock. Compare or subtract two instants to measure elapsed time; an `Instant` is not calendar-aware (use `DateTime` for that).

### now

`now() -> Instant`

Capture the current moment.

```kryos
use std::datetime

fn main() {
    let t0 = now()
    // ... work ...
    let took = t0.elapsed()
    print("took {took.as_millis()}ms")
}
```

---

### Instant methods

`t.elapsed() -> Duration` -- time since `t` was captured.

`t.duration_since(earlier: Instant) -> Duration` -- span between two instants.

`t.is_after(other: Instant) -> bool`, `t.is_before(other: Instant) -> bool` -- ordering.

`t.add(dur: Duration) -> Instant`, `t.sub(dur: Duration) -> Instant` -- shift an instant.

`t.as_millis() -> i64`, `t.as_secs() -> i64` -- the instant's raw clock value. (There are no `.timestamp()` / `.timestamp_millis()` methods; for wall-clock epoch time use the free functions below.)

---

## Clocks and measurement (free functions)

`timestamp() -> i64` -- current Unix epoch time in seconds.

`timestamp_millis() -> i64` -- current Unix epoch time in milliseconds.

`measure(callback: fn() -> str) -> Duration` -- run `callback` and return how long it took. The callback takes no arguments and returns a `str` (return `""` if you have nothing to say):

```kryos
use std::datetime

fn main() {
    let took = measure(|| {
        let mut i = 0
        while i < 1000000 { i = i + 1 }
        return "done"
    })
    print("loop took {took.as_millis()}ms")
}
```

---

## Sleeping

`sleep_millis(ms: i64) -> bool` -- sleep for `ms` milliseconds (real native sleep, not a busy-wait). Non-positive values return immediately.

`sleep_secs(secs: i64) -> bool` -- sleep for whole seconds.

`sleep(dur: Duration) -> bool` -- sleep for a duration. **Prefer `sleep_millis` / `sleep_secs`**: the bare name `sleep` shadows a global builtin and resolves to this module function reliably only on the AOT backend.

---

## DateTime

A calendar-aware UTC timestamp with public fields `year`, `month`, `day`, `hour`, `minute`, `second` (all `i32`-valued components derived from an epoch timestamp). Handles leap years (including the div-100/div-400 rules) and pre-1970 (negative) timestamps.

### from_timestamp

`from_timestamp(epoch_secs: i64) -> DateTime`

Convert a Unix timestamp to calendar components.

```kryos
use std::datetime

fn main() {
    let dt = from_timestamp(1713110400)
    print("{dt.year}-{dt.month}-{dt.day}")   // 2024-4-14
    print(dt.to_iso())                        // 2024-04-14T16:00:00Z
}
```

---

### now_utc

`now_utc() -> DateTime`

The current moment as a UTC `DateTime` (equivalent to `from_timestamp(timestamp())`).

---

### DateTime methods

`dt.to_iso() -> str` -- ISO-8601: `"2024-04-14T16:00:00Z"`.

`dt.to_date_string() -> str` -- date only: `"2024-04-14"`.

`dt.to_time_string() -> str` -- time only: `"16:00:00"`.

`dt.to_human() -> str` -- readable: `"Apr 14, 2024 16:00"`.

---

## Formatting helpers (free functions)

`format_timestamp(epoch_secs: i64) -> str` -- ISO-8601 rendering of an epoch timestamp: `format_timestamp(1713110400)` is `"2024-04-14T16:00:00Z"`. Takes only the timestamp -- there is **no** format-string parameter and no strftime-style `%Y`/`%m` tokens; for other layouts, build the string from `DateTime` fields or the `to_*` methods above.

`now_iso() -> str` -- `format_timestamp(timestamp())`.

---

## Notes

- All calendar math is UTC; there is no timezone handling in this module.
- `DateTime` fields are plain struct fields -- read them directly for custom formatting.
- Verified behaviors: `from_timestamp(0).to_iso()` is `"1970-01-01T00:00:00Z"`; negative timestamps resolve to pre-epoch dates (e.g. `-86400` is `1969-12-31`); the year-2038 (`2^31`) boundary is handled.
