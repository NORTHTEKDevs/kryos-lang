# Cookbook 24 · Resilience patterns

Three patterns from `std::ratelimit`, `std::circuit`, and `std::backoff`
that you'll combine for any production-grade external call.

## Recipe

```kryos
use std::ratelimit::{new_bucket, try_acquire}
use std::circuit::{new_breaker, allow, record_success, record_failure}
use std::backoff::{next_delay}

// std::ratelimit / std::circuit key their state on nanosecond timestamps,
// but the only clock builtin exposed is millisecond-resolution. Upscale it —
// the modules only care that the unit is consistent between calls.
fn now_nanos() -> i64 {
    return time_now_millis() * 1000000
}

fn call_remote(url: str) -> bool {
    // 1. Throttle to 10 RPS. new_bucket / new_breaker return an [i64] state
    // handle; because arrays are shared (ARC) handles, passing `bucket` into
    // try_acquire mutates the same backing storage — no `mut` needed here.
    let bucket: [i64] = new_bucket(10, 10, now_nanos())

    // 2. Open circuit after 5 consecutive failures, retry after 30s
    // (reset_nanos uses the same synthetic-nanosecond unit as now_nanos()).
    let breaker: [i64] = new_breaker(5, 30_000_000_000)

    let mut attempt: i64 = 0
    let mut delay_ms: i64 = 0

    loop {
        // Wait for permit + breaker closed.
        if try_acquire(bucket, now_nanos()) == false {
            sleep_ms(100)
            continue
        }
        if allow(breaker, now_nanos()) == false {
            println("circuit is open; failing fast")
            return false
        }

        let ok = do_request(url)
        if ok {
            record_success(breaker)
            return true
        }
        record_failure(breaker)
        attempt = attempt + 1
        if attempt >= 5 { return false }

        // Exponential backoff, doubling and capped at 5s. std::backoff has
        // no built-in jitter helper — add your own if you need it, e.g.
        // `delay_ms + (time_now_millis() % 200)`.
        delay_ms = next_delay(delay_ms, 100, 5000)
        println("retry " + to_string(attempt) + " after " + to_string(delay_ms) + "ms")
        sleep_ms(delay_ms)
    }
    return false  // unreachable — the checker requires an explicit fallthrough
}

fn do_request(url: str) -> bool {
    // Replace with your actual HTTP / TCP / etc. call.
    let _ = url
    return false  // simulated failure for demo
}

@capabilities(net)
fn main() {
    let ok = call_remote("https://example.com")
    println("result: " + to_string(ok))
}
```

Run it (`kryos run resilience.kry`) and you'll see four retries with growing
delays, then `result: false` once the simulated `do_request` has failed 5
times in a row.

## Pattern checklist

- **Rate limit first.** Bound how often you even *try* — prevents thundering herd.
- **Circuit breaker second.** Once a downstream is clearly broken, fail fast.
  Don't waste retries on a dead service.
- **Exponential backoff last.** When you *do* retry, use growing delays
  (`std::backoff::next_delay`) so you don't hammer a recovering service.
  `std::backoff` has no jitter helper — add jitter yourself
  (`delay_ms + (time_now_millis() % 200)`) to avoid synchronized retries
  across many clients.
- **Combine with timeouts.** Always set per-request timeouts; backoff doesn't
  help if a single call hangs forever.
