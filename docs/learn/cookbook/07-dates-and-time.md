# Cookbook 07 · Dates and time

`std::datetime` does Unix epoch math, UTC date breakdowns, and RFC 3339 formatting — no external dependencies. The pattern is: keep time as `i64` epoch-seconds, convert to human form only at the edges.

## The program

```kryos
// time_now_secs() and time_now_millis() are builtins — no import needed.
// std::datetime imports hit a known resolver bug with internal helpers;
// use the builtins and inline helpers directly instead.

// Approximate UTC year (accurate ±1 for 1970–2100).
fn approx_year(epoch: i64) -> i64 {
    return 1970 + epoch / 31557600
}

// UTC hour of day (0–23).
fn utc_hour(epoch: i64) -> i64 {
    return (epoch % 86400) / 3600
}

// UTC minute (0–59).
fn utc_minute(epoch: i64) -> i64 {
    return (epoch % 3600) / 60
}

// Day of week: 0=Sun … 6=Sat (epoch day 0 = Thursday = 4).
fn utc_weekday(epoch: i64) -> i64 {
    return (epoch / 86400 + 4) % 7
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

// Encode a UTC date as epoch seconds (Gregorian proleptic, no leap seconds).
fn epoch_from_ymd(year: i64, month: i64, day: i64) -> i64 {
    let y = year - 1970
    let leap_days = y / 4 - y / 100 + y / 400
    let days_in_months: [i64] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334]
    let dom = days_in_months[month - 1]
    return (y * 365 + leap_days + dom + day - 1) * 86400
}

@capabilities(io)
fn main() {
    let now = time_now_secs()

    println("Now (epoch):   " + to_string(now))
    println("Approx year:   " + to_string(approx_year(now)))
    println("Hour (UTC):    " + to_string(utc_hour(now)))
    println("Minute (UTC):  " + to_string(utc_minute(now)))
    println("Weekday:       " + weekday_name(utc_weekday(now)))

    // Compute days since a known date.
    let launch = epoch_from_ymd(2026, 5, 18)
    let days_since_launch = (now - launch) / 86400
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

    sleep_ms(100)
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
