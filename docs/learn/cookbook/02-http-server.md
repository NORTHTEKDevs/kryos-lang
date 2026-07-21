# Cookbook 02 · HTTP server

Build a tiny JSON API. Two endpoints: `GET /health` and `POST /echo`. Handles concurrent requests with `spawn`.

## The program

Save as `server.kry`:

```kryos
use std::http::{new_router, route_get, route_post, http_serve, json_response, text_response}

@capabilities(net, io)
fn main() {
    let router = new_router()
    let router = route_get(router, "/health", |req| json_response(200, "{{\"ok\":true}}"))
    let router = route_post(router, "/echo", |req|
        json_response(200, "{{\"echoed\":\"" + req.body + "\"}}"))
    println("listening on http://127.0.0.1:8080")
    http_serve(8080, router)
}
```

## Run it

```bash
kryos run server.kry &

curl http://localhost:8080/health
# → {"ok":true}

curl -X POST http://localhost:8080/echo -d "hello"
# → {"echoed":"hello"}
```

## What this teaches

- **`tcp_listen` / `tcp_accept`** are the low-level networking primitives `http_serve` is built on.
- **`parse_request` / `serialize_response`** (internal to `std::http`) parse and emit HTTP/1.1 messages.
- **`spawn { ... }`** runs the handler on a fresh task; the main loop returns to `accept` immediately. The TCP stack does not serialize through a global mutex, so workers actually run concurrently.
- **`@capabilities(net, io)`** is declared explicitly. The compiler will reject any helper this calls that doesn't carry these capabilities.

## Variations to try

- Add a `GET /time` endpoint that returns the current Unix time.
- Track request counts with an atomic counter (`atomic_i64_new`).
- Switch to HTTPS — see `examples/https_server.kry` for the TLS variant.

When you're ready for more, see [03 · JSON pipeline](./03-json-pipeline.md).
