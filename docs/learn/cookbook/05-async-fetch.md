# Cookbook 05 · Async fetch many

Fetch a list of URLs concurrently using async/await, without spawning a thread per URL.

## The program

Save as `fetch_many.kry`:

```kryos
async fn fetch_size(url: str) -> i64 {
    let body = await http_get(url)
    len(body)
}

async fn main() {
    let urls = [
        "https://example.com",
        "https://example.org",
        "https://example.net",
    ]

    let mut total = 0
    for url in urls {
        let bytes = await fetch_size(url)
        println(url + " → " + to_string(bytes) + " bytes")
        total = total + bytes
    }
    println("total: " + to_string(total) + " bytes")
}
```

## Run it

```bash
kryos run fetch_many.kry
# → https://example.com → 1256 bytes
# → https://example.org → 1256 bytes
# → https://example.net → 1256 bytes
# → total: 3768 bytes
```

## What this teaches

- **`async fn`** functions return a future. Calling them does *not* execute their body; `await` does.
- **The compiler lowers async functions** to state machines. There's no thread per await — they suspend and resume on a single worker.
- **`@capabilities(net)` is inferred** from `http_get`; you don't need to write it on `async` boundaries explicitly.

## Sequential vs concurrent

The example above is *sequential* — each `await` blocks the next. To fetch concurrently, spawn each one and await the join:

```kryos
async fn main() {
    let urls = ["https://example.com", "https://example.org"]
    let mut handles = []
    for url in urls {
        handles = array_push(handles, async_spawn(fetch_size(url)))
    }
    let mut total = 0
    for h in handles {
        total = total + await async_join(h)
    }
    println("total: " + to_string(total))
}
```

Now both fetches happen in parallel; the loop just collects their results.

## Variations to try

- Add a timeout: wrap `await` in `await_with_timeout(fut, 5000)` (returns an `Option`).
- Parse JSON from each response and merge them.
- Add a retry-with-backoff helper around `http_get`.

When you're ready for more, see [06 · Build a small library](./06-library.md).
