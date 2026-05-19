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
use std::net::{tcp_listen, accept, send, recv_str, close}
use std::log::{log_emit, log_set_level}

struct Request {
    method: str,
    path: str,
    body: str,
}

@capabilities(net, io)
fn main() {
    log_set_level(2)  // INFO
    let listener = tcp_listen("127.0.0.1", 8080)
    log_emit(2, "listening", "addr=127.0.0.1:8080")

    loop {
        let conn = accept(listener)
        if conn < 0 { continue }
        let raw = recv_str(conn, 8192)
        let req = parse_request(raw)
        let resp = route(req)
        send(conn, resp)
        close(conn)
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

    // Body comes after the empty line.
    let mut body: str = ""
    let mut in_body: bool = false
    let mut i: i64 = 1
    while i < len(lines) {
        if in_body { body = body + lines[i] }
        if len(lines[i]) == 0 && !in_body { in_body = true }
        i = i + 1
    }
    return Request { method: method, path: path, body: body }
}

fn route(req: Request) -> str {
    if req.path == "/" && req.method == "GET" {
        return http_ok("text/plain", "todos here")
    }
    return http_404()
}

fn http_ok(content_type: str, body: str) -> str {
    return "HTTP/1.1 200 OK\r\n" +
           "Content-Type: " + content_type + "\r\n" +
           "Content-Length: " + to_string(len(body)) + "\r\n" +
           "Connection: close\r\n\r\n" + body
}

fn http_404() -> str {
    let body = "404 not found\n"
    return "HTTP/1.1 404 Not Found\r\n" +
           "Content-Length: " + to_string(len(body)) + "\r\n" +
           "Connection: close\r\n\r\n" + body
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

For `POST /todos` we'll accept `{"title": "..."}` and store it. We won't fully implement state mutation in this snippet (Kryos's mutex story is the next add) — for now show the JSON parse path:

```kryos
use std::json::{json_parse, json_string_field}

fn handle_create(req: Request) -> str {
    let obj = json_parse(req.body)
    let title = json_string_field(obj, "title")
    if len(title) == 0 {
        return http_400("title required")
    }
    // ... store ...
    let payload = "{\"id\": 1, \"title\": \"" + title + "\"}"
    return http_ok("application/json", payload)
}

fn http_400(msg: str) -> str {
    let body = "{\"error\": \"" + msg + "\"}"
    return "HTTP/1.1 400 Bad Request\r\n" +
           "Content-Type: application/json\r\n" +
           "Content-Length: " + to_string(len(body)) + "\r\n" +
           "Connection: close\r\n\r\n" + body
}
```

## Step 4 — Rate limit per request

Add a token bucket via `std::ratelimit`. Refuse 429s when the bucket is empty.

```kryos
use std::ratelimit::{ratelimit_init, ratelimit_try_acquire}
use std::datetime::time_now_nanos

let mut bucket: [i64] = [0, 0, 0, 0]
ratelimit_init(bucket, 100, 50, time_now_nanos())  // 100 tokens cap, 50/sec refill

// In the accept loop:
if ratelimit_try_acquire(bucket, time_now_nanos()) == 0 {
    send(conn, http_429())
    close(conn)
    continue
}
```

## Step 5 — Tests

In `tests/api.kry`:

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
