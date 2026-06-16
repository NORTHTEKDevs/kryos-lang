# Cookbook 05 · Fetch many concurrently

Fetch a list of URLs in parallel, one OS thread per URL, and collect the
results over a channel. This is the concurrency model that actually works in
Kryos today: `spawn` + typed channels (see
[Concurrency](../../09-concurrency.md)).

> **Heads up on `async`/`await`:** Kryos parses and type-checks `async fn` and
> `await`, but `await expr` currently lowers to a **direct synchronous call** —
> there is no non-blocking executor and no single-worker suspension behind it.
> For real concurrency today, use `spawn` + channels (below) or actors. Do not
> reach for `async`/`await` expecting cooperative scheduling; it is grammar-only.

## The program

Save as `fetch_many.kry`:

```kryos
use std::net::{http_get}

fn main() {
    let urls = [
        "https://example.com",
        "https://example.org",
        "https://example.net",
    ]

    let ch = chan()

    // One OS thread per URL. Each fetch runs independently and sends its
    // byte count back on the shared channel.
    for url in urls {
        spawn {
            let resp = http_get(url)
            send(ch, len(resp.body))
        }
    }

    // Collect one result per spawned fetch. `recv` blocks until a value is
    // ready, so the loop naturally waits for all fetches to finish.
    let mut total = 0
    let mut i = 0
    while i < len(urls) {
        let bytes = recv(ch)
        println("fetched " + to_string(bytes) + " bytes")
        total = total + bytes
        i = i + 1
    }
    println("total: " + to_string(total) + " bytes")
}
```

## Run it

```bash
kryos run fetch_many.kry
# → fetched 1256 bytes
# → fetched 1256 bytes
# → fetched 1256 bytes
# → total: 3768 bytes
```

## What this teaches

- **`spawn { ... }`** starts a real OS thread and returns immediately. The
  fetches run in parallel, not one after another.
- **Channels (`chan()` / `send` / `recv`)** are the safe way to get results
  back out of a spawned thread. `recv` blocks until a value is available, so
  recv-ing once per spawned task is a simple "wait for all" barrier.
- **Channels carry `i64`.** We send each response's byte count (an `i64`).
  Results arrive in completion order, not URL order — if you need to correlate
  a result with its URL, send a tagged id and look it up, or use an actor that
  holds a result map.
- **`@capabilities(net)` is inferred** from `http_get`; you don't have to write
  it explicitly on the spawned block.

## Why not async/await?

The obvious-looking version —

<!-- docs-example: skip -->
```kryos
// Does NOT run concurrently: `await` lowers to a synchronous call today.
async fn fetch_size(url: str) -> i64 {
    let resp = await http_get(url)
    return len(resp.body)
}
```

— compiles, but every `await` runs to completion before the next line, so the
fetches happen one at a time. The `spawn` + channel version above is what gives
you actual parallelism. A non-blocking I/O runtime is planned, not shipped.

## Variations to try

- Send a `(id, bytes)` pair over the channel (id first) so you can map each
  result back to its URL.
- Bound concurrency: spawn in batches of N, draining the channel between
  batches, instead of one thread per URL.
- Use an actor to accumulate results into a map keyed by URL.

When you're ready for more, see [06 · Build a small library](./06-library.md).
