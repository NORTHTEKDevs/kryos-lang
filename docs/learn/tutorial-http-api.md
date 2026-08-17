# Tutorial: Build an HTTP API server in Kryos

We'll build a working in-memory todo API: GET / list, POST / create, GET /:id, DELETE /:id. ~150 lines of Kryos, no dependencies beyond stdlib.

## Outcome

By the end you'll have:

- A TCP-accept loop on `127.0.0.1:8080`
- HTTP/1.1 request parsing (method + path + body)
- JSON request/response handling via `std::json`
- In-memory state with `std::mutex` for thread safety
- Structured logging via `std::log`
- Rate limiting via `std::ratelimit`
- Tests via `kryos test`

## Step 1 — Scaffold the project

```bash
kryos new todoapi --template http
cd todoapi
```

This creates `kryos.toml`, `src/main.kry` (with a basic TCP accept loop), `tests/smoke.kry`, and `README.md`.

## Step 2 — Parse HTTP requests

Replace `src/main.kry` with a request-handling skeleton:

```kryos
use std::net::{net_listen, http_get}
use std::string::{split_lines}
use std::log::{info, warn}

struct Request {
    method: str,
    path: str,
    body: str,
}

@capabilities(net, io)
fn main() {
    info("listening on 127.0.0.1:8080")
    let listener = net_listen("127.0.0.1", 8080)

    loop {
        let conn = listener.accept()
        // Read the request with a single bounded recv. Do NOT use read_all()
        // here: it blocks reading until the client half-closes its write side,
        // which a normal HTTP client never does on a keep-alive connection, so
        // the server would hang (or throw and crash) on the first request.
        let raw = conn.read(8192)
        let req = parse_request(raw)
        let resp = route(req)
        conn.write_all(resp)
        conn.close()
    }
}

fn parse_request(raw: str) -> Request {
    let lines = split_lines(raw)
    if len(lines) == 0 {
        return Request { method: "", path: "", body: "" }
    }
    let first = lines[0]
    let method_end = index_of_char(first, " ")
    let path_start = method_end + 1
    let path_end = path_start + index_of_char(substr(first, path_start, len(first)), " ")
    let method = substr(first, 0, method_end)
    let path = substr(first, path_start, path_end)

    let mut body: str = ""
    let mut in_body: bool = false
    let mut i: i64 = 1
    while i < len(lines) {
        if in_body { body = body + lines[i] }
        if len(lines[i]) == 0 and !in_body { in_body = true }
        i = i + 1
    }
    return Request { method: method, path: path, body: body }
}

fn route(req: Request) -> str {
    if req.path == "/" and req.method == "GET" {
        return http_ok("text/plain", "todos here")
    }
    return http_404()
}

fn http_ok(content_type: str, body: str) -> str {
    return "HTTP/1.1 200 OK\r\nContent-Type: " + content_type +
           "\r\nContent-Length: " + to_string(len(body)) +
           "\r\nConnection: close\r\n\r\n" + body
}

fn http_404() -> str {
    let body = "404 not found\n"
    return "HTTP/1.1 404 Not Found\r\nContent-Length: " +
           to_string(len(body)) + "\r\nConnection: close\r\n\r\n" + body
}

fn index_of_char(s: str, c: str) -> i64 {
    let n = len(s)
    let mut i: i64 = 0
    while i < n {
        if substr(s, i, i + 1) == c { return i }
        i = i + 1
    }
    return n
}
```

## Step 3 — Add JSON request handling

> Building a JSON string by hand with `+`? Every Kryos string literal interpolates (a bare `{` opens an interpolation), so a literal `{`/`}` in a hand-built JSON string must be doubled (`{{`/`}}`) as shown below -- see `docs/learn/common-errors.md`. Prefer `std::json::json_object`/`std::fmt` for anything beyond a one-off literal.

For `POST /todos` we'll accept `{"title": "..."}` and store it. We won't fully implement state mutation in this snippet (Kryos's mutex story is the next add) — for now show the JSON parse path:

```kryos
use std::json::{parse, get, to_str}

fn handle_create(req: Request) -> str {
    let obj = parse(req.body)
    let title = to_str(get(obj, "title"))
    if len(title) == 0 {
        return http_400("title required")
    }
    // ... store ...
    let payload = "{{\"id\": 1, \"title\": \"" + title + "\"}}"
    return http_ok("application/json", payload)
}

fn http_400(msg: str) -> str {
    let body = "{{\"error\": \"" + msg + "\"}}"
    return "HTTP/1.1 400 Bad Request\r\n" +
           "Content-Type: application/json\r\n" +
           "Content-Length: " + to_string(len(body)) + "\r\n" +
           "Connection: close\r\n\r\n" + body
}
```

## Step 4 — Rate limit per request

Add a token bucket via `std::ratelimit`. Refuse 429s when the bucket is empty.

<!-- docs-example: skip -->
```kryos
use std::ratelimit::{new_bucket, try_acquire}

// Before the accept loop (in main):
let now_ns = time_now_secs() * 1000000000
let mut bucket = new_bucket(100, 50, now_ns)  // 100 tokens cap, 50/sec refill

// Inside the accept loop:
if !try_acquire(bucket, time_now_secs() * 1000000000) {
    conn.write_all(http_429())
    conn.close()
    continue
}
```

## Step 5 — Tests

In `tests/api.kry`:

<!-- docs-example: skip -->
```kryos
@test
fn parses_simple_request() {
    let raw = "GET / HTTP/1.1\r\nHost: x\r\n\r\n"
    let req = parse_request(raw)
    assert(req.method == "GET")
    assert(req.path == "/")
}

@test
fn unknown_route_returns_404() {
    let req = Request { method: "GET", path: "/missing", body: "" }
    let resp = route(req)
    assert(contains(resp, "404"))
}
```

Run them:

```bash
kryos test
```

## Step 6 — Profile + bench

```bash
kryos profile src/main.kry        # call-count profile
kryos bench                       # if you've added @bench functions
```

## Step 7 — Deploy

See [docs/deploy/docker.md](../deploy/docker.md) for a multi-stage Dockerfile and [docs/deploy/systemd.md](../deploy/systemd.md) for the systemd unit. The binary is ~3MB stripped; ~25MB container image with distroless.

## Where to go next

- Add `std::sqlite` for persistent storage
- Add `std::log` to structured-log every request
- Add `std::circuit` if you're calling out to a flaky downstream
- Add `kryos audit` to your CI to scan for `API_KEY=` patterns in source
