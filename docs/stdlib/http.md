# std::http

HTTP client, server, and routing primitives. Covers URL parsing, header management, request building, a full HTTP client (requires `@capabilities(net)`), a lightweight router, and a TCP server.

```kryos
use std::http
```

---

## Types

### Url

```kryos
struct Url {
    scheme:   str,
    host:     str,
    port:     i64,
    path:     str,
    query:    str,
    fragment: str,
    raw:      str
}
```

---

### Headers

```kryos
struct Headers {
    keys:   [str],
    values: [str]
}
```

---

### Request

```kryos
struct Request {
    method:  str,
    url:     Url,
    headers: Headers,
    body:    str
}
```

`url` is a parsed `Url` struct, not a raw string -- use `req.url.path`, `req.url.query`, etc. (see `Url` above). To get the original raw URL text, use `req.url.raw`.

---

### Response

```kryos
struct Response {
    status:      i64,
    status_text: str,
    headers:     Headers,
    body:        str
}
```

---

## URL

### parse_url

`parse_url(raw: str) -> Url`

Parse a URL string into its components. Throws on malformed input.

---

### url_to_string

`url_to_string(u: Url) -> str`

Serialize a `Url` back to its canonical string form.

**Example:**
```kryos
use std::http

let u = parse_url("https://api.example.com:8080/v1/items?limit=10#top")
println(u.scheme)    // "https"
println(u.host)      // "api.example.com"
println(u.port)      // 8080
println(u.path)      // "/v1/items"
println(u.query)     // "limit=10"
println(u.fragment)  // "top"
```

---

## Headers

### new_headers

`new_headers() -> Headers`

Create an empty header map.

---

### set_header

`set_header(h: Headers, key: str, value: str)`

Set (or replace) a header.

---

### get_header

`get_header(h: Headers, key: str) -> str`

Return the value for `key`, or an empty string if not present.

---

### has_header

`has_header(h: Headers, key: str) -> bool`

Return `true` if the header exists.

---

### headers_to_string

`headers_to_string(h: Headers) -> str`

Serialize headers to a `"Key: Value\r\n"` formatted string.

---

### parse_headers

`parse_headers(raw: str) -> Headers`

Parse a raw header block into a `Headers` struct.

**Example:**
```kryos
use std::http

let h = new_headers()
set_header(h, "Content-Type", "application/json")
set_header(h, "Authorization", "Bearer token123")

println(has_header(h, "Content-Type"))   // true
println(get_header(h, "Authorization"))  // "Bearer token123"
```

---

## Request Builders

### new_request

`new_request(method: str, url_str: str) -> Request`

Create a `Request` with empty headers and no body.

---

### with_body

`with_body(req: Request, body: str) -> Request`

Return a copy of `req` with `body` set.

---

### with_json_body

`with_json_body(req: Request, body: str) -> Request`

Return a copy of `req` with `body` set and `Content-Type: application/json` added.

---

### serialize_request

`serialize_request(req: Request) -> str`

Serialize `req` to a raw HTTP/1.1 request string.

---

### parse_response

`parse_response(raw: str) -> Response`

Parse a raw HTTP response string into a `Response` struct.

---

## HTTP Client

All HTTP client functions require `@capabilities(net)` on the calling function or module.

### http_send

`http_send(req: Request) -> Response`

Execute `req` and return the response.

---

### get

`get(url: str) -> Response`

Send a `GET` request to `url`.

---

### post

`post(url: str, body: str) -> Response`

Send a `POST` request with a plain-text body.

---

### post_json

`post_json(url: str, json_body: str) -> Response`

Send a `POST` request with a JSON body (`Content-Type: application/json` set automatically).

---

### put

`put(url: str, body: str) -> Response`

Send a `PUT` request.

---

### delete

`delete(url: str) -> Response`

Send a `DELETE` request.

---

### patch

`patch(url: str, body: str) -> Response`

Send a `PATCH` request.

---

### head

`head(url: str) -> Response`

Send a `HEAD` request. The response body is always empty.

**Example:**
```kryos
use std::http

@capabilities(net)
fn fetch_user(id: i64) -> Response {
    return get("https://api.example.com/users/" + id)
}

@capabilities(net)
fn create_item(payload: str) -> Response {
    return post_json("https://api.example.com/items", payload)
}

let resp = fetch_user(42)
println(resp.status)   // 200
println(resp.body)
```

---

## Router

A pattern-matching HTTP router for server-side code.

### Route

```kryos
struct Route {
    method:  str,
    path:    str,
    handler: fn(Request) -> Response
}
```

---

### Router

```kryos
struct Router {
    routes: [Route]
}
```

---

### new_router

`new_router() -> Router`

Create an empty `Router`.

---

### add_route

`add_route(router: Router, method: str, path: str, handler: fn(Request) -> Response)`

Register a route for the given HTTP method and path pattern.

---

### route_get

`route_get(router: Router, path: str, handler: fn(Request) -> Response)`

Shorthand for `add_route(router, "GET", path, handler)`.

---

### route_post

`route_post(router: Router, path: str, handler: fn(Request) -> Response)`

Shorthand for `add_route(router, "POST", path, handler)`.

---

### route_put

`route_put(router: Router, path: str, handler: fn(Request) -> Response)`

Shorthand for `add_route(router, "PUT", path, handler)`.

---

### route_delete

`route_delete(router: Router, path: str, handler: fn(Request) -> Response)`

Shorthand for `add_route(router, "DELETE", path, handler)`.

---

### match_route

`match_route(router: Router, method: str, path: str) -> Route`

Find the first registered `Route` whose method and path match. Takes the method and path directly (NOT a `Request`), and returns the matched `Route` -- call `route.handler(req)` yourself to get a `Response`. **Throws** (`throw "no matching route for ..."`) if nothing matches; it does not return a 404 `Response` itself. `http_serve` (below) wraps this call in a `try`/`catch` and turns an uncaught throw into a `404 Not Found` response for you -- if you call `match_route` directly, wrap it in `try`/`catch` yourself.

```kryos
try {
    let route = match_route(router, req.method, req.url.path)
    resp = route.handler(req)
} catch e {
    resp = text_response(404, "Not Found")
}
```

---

## Server

### parse_request

`parse_request(raw: str) -> Request`

Parse a raw HTTP/1.1 request string.

---

### serialize_response

`serialize_response(resp: Response) -> str`

Serialize a `Response` to a raw HTTP/1.1 response string.

---

### text_response

`text_response(status: i64, body: str) -> Response`

Convenience constructor: build a `Response` with `Content-Type: text/plain`.

---

### json_response

`json_response(status: i64, body: str) -> Response`

Convenience constructor: build a `Response` with `Content-Type: application/json`.

---

### http_serve

`http_serve(port: i64, router: Router) -> void`

Bind a TCP server to `port` and dispatch incoming requests through `router` (each connection handled on its own `spawn`ed task; an unmatched route or a handler panic is caught internally and turned into a `404`/`500` response, not a crash). Blocks indefinitely. Requires `@capabilities(net)`. There is no separate `listen` function -- this is the only server entry point.

**Example:**
```kryos
use std::http

let router = new_router()

route_get(router, "/", fn(req: Request) -> Response {
    return text_response(200, "Welcome to Kryos")
})

route_get(router, "/health", fn(req: Request) -> Response {
    return json_response(200, "{{\"status\": \"ok\"}}")
})

route_post(router, "/echo", fn(req: Request) -> Response {
    return text_response(200, req.body)
})

@capabilities(net)
fn start() {
    println("listening on :8080")
    http_serve(8080, router)
}

start()
```

Note the doubled `{{`/`}}` in the JSON literals above: **every string literal interpolates** (CLAUDE.md hard rule 4), so a bare `{` opens interpolation and a JSON object literal must escape its braces as `{{`/`}}` (or be built with `+`) -- see `docs/learn/common-errors.md`.

---

## Complete Example

```kryos
use std::http

// Client: fetch and post
@capabilities(net)
fn run_client() {
    let resp = get("https://httpbin.org/get")
    println(resp.status)   // 200

    let payload = "{{\"name\": \"kryos\", \"version\": \"0.3.4\"}}"
    let created = post_json("https://httpbin.org/post", payload)
    println(created.status)   // 200
}

// Server: minimal REST API
let api = new_router()

route_get(api, "/ping", fn(req: Request) -> Response {
    return json_response(200, "{{\"pong\": true}}")
})

route_post(api, "/data", fn(req: Request) -> Response {
    // req.body contains the raw request payload
    return json_response(201, "{{\"received\": true}}")
})

// Error response helper
let not_found_resp = json_response(404, "{{\"error\": \"not found\"}}")

@capabilities(net)
fn serve() {
    println("server running on :3000")
    listen(3000, api)
}
```
