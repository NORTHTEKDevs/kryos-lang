# Cookbook 24 · Resilience patterns

Three patterns from `std::ratelimit`, `std::circuit`, and `std::backoff`
that you'll combine for any production-grade external call.

## Recipe

```kryos
use std::ratelimit::{ratelimit_init, ratelimit_try_acquire}
use std::circuit::{cb_init, cb_allow, cb_record_success, cb_record_failure, STATE_CLOSED}
use std::backoff::{backoff_next}
use std::datetime::{time_now_nanos, time_sleep_millis}

@capabilities(io, net)
fn call_remote(url: str) -> bool {
    // 1. Throttle to 10 RPS.
    let mut bucket: [i64] = [0, 0, 0, 0]
    ratelimit_init(bucket, 10, 10, time_now_nanos())

    // 2. Open circuit after 5 consecutive failures, retry after 30s.
    let mut breaker: [i64] = [0, 0, 0, 0, 0, 0]
    cb_init(breaker, 5, 30_000_000_000)

    let mut attempt: i64 = 0
    let mut delay_ms: i64 = 0

    loop {
        // Wait for permit + breaker closed.
        if ratelimit_try_acquire(bucket, time_now_nanos()) == 0 {
            time_sleep_millis(100)
            continue
        }
        if cb_allow(breaker, time_now_nanos()) == 0 {
            println("circuit is open; failing fast")
            return false
        }

        let ok = do_request(url)
        if ok {
            cb_record_success(breaker)
            return true
        }
        cb_record_failure(breaker)
        attempt = attempt + 1
        if attempt >= 5 { return false }

        // Exponential backoff with 20% jitter.
        delay_ms = backoff_next(delay_ms, 100, 5000, attempt, 200)
        println("retry " + to_string(attempt) + " after " + to_string(delay_ms) + "ms")
        time_sleep_millis(delay_ms)
    }
}

fn do_request(url: str) -> bool {
    // Replace with your actual HTTP / TCP / etc. call.
    let _ = url
    return false  // simulated failure for demo
}
```

## Pattern checklist

- **Rate limit first.** Bound how often you even *try* — prevents thundering herd.
- **Circuit breaker second.** Once a downstream is clearly broken, fail fast.
  Don't waste retries on a dead service.
- **Exponential backoff with jitter last.** When you *do* retry, do it with
  growing delays and jitter to avoid synchronized retries.
- **Combine with timeouts.** Always set per-request timeouts; backoff doesn't
  help if a single call hangs forever.
