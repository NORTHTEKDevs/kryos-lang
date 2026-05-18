# Cookbook 07 · Dates and time

`std::datetime` does Unix epoch math, UTC date breakdowns, and RFC 3339 formatting — no external dependencies. The pattern is: keep time as `i64` epoch-seconds, convert to human form only at the edges.

## The program

```kryos
use std::datetime::{
    time_now_secs,
    time_now_millis,
    time_sleep_millis,
    time_year_utc,
    time_month_utc,
    time_day_utc,
    time_hour_utc,
    time_minute_utc,
    time_weekday_utc,
    time_from_ymdhms_utc,
    time_format_rfc3339_utc,
}

@capabilities(io)
fn main() {
    let now = time_now_secs()

    println("Now (epoch):   " + to_string(now))
    println("Year:          " + to_string(time_year_utc(now)))
    println("Month:         " + to_string(time_month_utc(now)))
    println("Day:           " + to_string(time_day_utc(now)))
    println("Hour (UTC):    " + to_string(time_hour_utc(now)))
    println("Minute (UTC):  " + to_string(time_minute_utc(now)))
    println("Weekday:       " + weekday_name(time_weekday_utc(now)))
    println("RFC 3339:      " + time_format_rfc3339_utc(now))

    // Build an epoch from a known date.
    let launch_day = time_from_ymdhms_utc(2026, 5, 18, 0, 0, 0)
    let seconds_since_launch = now - launch_day
    let days_since_launch = seconds_since_launch / 86400
    println("Days since 2026-05-18: " + to_string(days_since_launch))

    // Tiny benchmark: how long does this loop take?
    let t0 = time_now_millis()
    let mut sum: i64 = 0
    let mut i: i64 = 0
    while i < 1000000 {
        sum = sum + i
        i = i + 1
    }
    let elapsed = time_now_millis() - t0
    println("Sum: " + to_string(sum) + " in " + to_string(elapsed) + "ms")

    // Pause 100ms then exit.
    time_sleep_millis(100)
}

fn weekday_name(n: i64) -> str {
    if n == 0 { return "Sunday" }
    if n == 1 { return "Monday" }
    if n == 2 { return "Tuesday" }
    if n == 3 { return "Wednesday" }
    if n == 4 { return "Thursday" }
    if n == 5 { return "Friday" }
    return "Saturday"
}
```

## Run it

```bash
kryos run dates.kry
```

## Things to know

- All breakdowns are UTC. There is no built-in timezone library yet; if you need local time, compute the offset yourself with `env_get("TZ")` and add it to the epoch value.
- `time_format_rfc3339_utc` always emits the `Z` suffix.
- `time_from_ymdhms_utc(...)` returns -1 on out-of-range input (month outside 1..12, day outside 1..31). Always check.
- `time_sleep_millis` parks the thread — the same caveat as Rust: don't call inside a hot loop unless you mean it.
