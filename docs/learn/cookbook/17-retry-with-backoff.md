# Cookbook 17 · Retry with exponential backoff

Network calls fail. The pattern is: try once, on failure wait Nms, retry, double N, up to a cap.

## The program

```kryos
// sleep_ms(ms) is a builtin — no import needed.

@capabilities(io, net)
fn main() {
    let result = with_retry(5, 100, attempt_request)
    if result {
        println("success")
    } else {
        println("failed after 5 attempts")
    }
}

/// Run `op` up to `max_attempts` times, sleeping with exponential backoff
/// between failures. Returns true on first success.
fn with_retry(max_attempts: i64, base_ms: i64, op: fn() -> bool) -> bool {
    let mut attempt: i64 = 0
    let mut delay: i64 = base_ms
    while attempt < max_attempts {
        if op() {
            return true
        }
        attempt = attempt + 1
        if attempt >= max_attempts { break }
        println("attempt " + to_string(attempt) + " failed, waiting " + to_string(delay) + "ms")
        sleep_ms(delay)
        delay = delay * 2
        // Cap at 30 seconds
        if delay > 30000 { delay = 30000 }
    }
    return false
}

fn attempt_request() -> bool {
    // Replace with your actual operation. For demo, succeed on the 3rd try.
    let count = env_get("ATTEMPTS")
    let n = parse_int(count)
    if n >= 2 { return true }
    return false
}
```

## Things to know

- `sleep_ms(N)` is a builtin. It blocks the current thread.
- Cap the maximum delay (30s in this recipe). Unbounded growth = bad UX.
- Add jitter for thundering-herd scenarios: `delay + (time_now_millis() % 50)`.
- For async contexts, use `await sleep_ms(...)` — it yields to
  the scheduler instead of blocking the OS thread.
- Distinguish retriable errors (5xx, timeouts, connection refused) from
  non-retriable (4xx) — don't burn retries on permanent failures.
