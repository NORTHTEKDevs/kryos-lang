# std::server

HTTP server creation, routing, middleware, and request/response handling.

All functions in this module are available after `use std::server`. The server uses Python's `http.server` internally with threading for concurrent request handling. Suitable for development and lightweight production use.

---

## Server Lifecycle

### http_app

```
http_app() -> i32
```

Create a new HTTP application instance. Returns a handle (integer) used by all other server functions.

**Example:**

```kryos
let app = http_app()
```

**See also:** `app_get`, `app_listen`

---

### app_listen

```
app_listen(handle: i32, port: i32) -> none
```

Start the HTTP server on the given port. This call blocks forever (until Ctrl+C). Binds to `0.0.0.0` so the server is accessible from any network interface.

**Example:**

```kryos
let app = http_app()
app_get(app, "/", fn(req) {
    respond(200, json_stringify({"message": "hello"}))
})
app_listen(app, 3000)
```

**Edge cases:**

- Blocks the current thread. Code after `app_listen` will not execute until the server is shut down.
- Prints `Listening on :PORT` to stdout on startup.
- Handles `KeyboardInterrupt` (Ctrl+C) for graceful shutdown.
- Each request is handled in its own thread.
- Maximum request body size is 10 MB. Requests exceeding this return HTTP 413.

**See also:** `http_app`

---

## Routing

Route handlers receive a request map and use `respond` to set the response.

The request map has these keys:

| Key | Type | Description |
|-----|------|-------------|
| `method` | `str` | HTTP method (`"GET"`, `"POST"`, etc.) |
| `path` | `str` | Request path (e.g., `"/api/users"`) |
| `params` | `map` | URL parameters from route patterns (e.g., `{id: "42"}`) |
| `query` | `map` | Query string parameters |
| `headers` | `map` | Request headers (all lowercase keys) |
| `body` | `str` | Request body as a string |

### app_get

```
app_get(handle: i32, path: str, handler: fn) -> i32
```

Register a handler for GET requests matching the given path. Returns the app handle for chaining.

**Example:**

```kryos
app_get(app, "/api/health", fn(req) {
    respond(200, json_stringify({"status": "ok"}))
})
```

**See also:** `app_post`, `app_put`, `app_delete`

---

### app_post

```
app_post(handle: i32, path: str, handler: fn) -> i32
```

Register a handler for POST requests.

**Example:**

```kryos
app_post(app, "/api/users", fn(req) {
    let data = json_parse(json_get(req, "body"))
    let name = json_get(data, "name")
    respond(201, json_stringify({"created": name}))
})
```

**See also:** `app_get`, `app_put`, `app_delete`

---

### app_put

```
app_put(handle: i32, path: str, handler: fn) -> i32
```

Register a handler for PUT requests.

**Example:**

```kryos
app_put(app, "/api/users/:id", fn(req) {
    let id = json_get(json_get(req, "params"), "id")
    let data = json_parse(json_get(req, "body"))
    respond(200, json_stringify({"updated": id}))
})
```

**See also:** `app_get`, `app_post`, `app_delete`

---

### app_delete

```
app_delete(handle: i32, path: str, handler: fn) -> i32
```

Register a handler for DELETE requests.

**Example:**

```kryos
app_delete(app, "/api/users/:id", fn(req) {
    let id = json_get(json_get(req, "params"), "id")
    respond(200, json_stringify({"deleted": id}))
})
```

**See also:** `app_get`, `app_post`, `app_put`

---

## Route Parameters

Route paths support named parameters with the `:param` syntax. Parameters are extracted into the `params` field of the request map.

**Example:**

```kryos
app_get(app, "/api/users/:id", fn(req) {
    let id = json_get(json_get(req, "params"), "id")
    respond(200, json_stringify({"user_id": id}))
})

// GET /api/users/42 -> params = {"id": "42"}
```

```kryos
app_get(app, "/api/repos/:owner/:name", fn(req) {
    let params = json_get(req, "params")
    let owner = json_get(params, "owner")
    let name = json_get(params, "name")
    respond(200, json_stringify({"repo": owner + "/" + name}))
})

// GET /api/repos/alice/kryos -> params = {"owner": "alice", "name": "kryos"}
```

**Edge cases:**

- Parameters match exactly one path segment. `/users/:id` matches `/users/42` but not `/users/42/posts`.
- Parameter values are always strings.
- If no route matches, the server returns HTTP 404 with `{"error": "not found"}`.

---

## Response

### respond

```
respond(status: i32, body: str, headers: map?) -> none
```

Set the response for the current request. Must be called inside a route handler or middleware.

**Example:**

```kryos
respond(200, json_stringify({"message": "success"}))
```

```kryos
respond(201, json_stringify({"id": 1}), {
    "X-Request-Id": "abc-123"
})
```

```kryos
// HTML response
respond(200, "<h1>Hello</h1>", {
    "Content-Type": "text/html"
})
```

**Edge cases:**

- If no `Content-Type` header is provided, defaults to `application/json`.
- If `respond` is not called in a handler, the server returns an empty 200 response.
- Calling `respond` multiple times in the same handler overwrites the previous response.

---

## Middleware

Middleware functions run before route handlers. They receive the request map and a `next` function. Call `next()` to continue to the next middleware or the route handler. Skip calling `next()` to short-circuit the chain (e.g., for auth failures or rate limiting).

### app_use

```
app_use(handle: i32, middleware: fn) -> i32
```

Register middleware on an app. Middleware runs in the order it is registered. Returns the app handle for chaining.

**Example:**

```kryos
// Custom auth middleware
app_use(app, fn(req, next) {
    let auth = json_get(json_get(req, "headers"), "authorization")
    if auth == "Bearer secret123" {
        next()
    } else {
        respond(401, json_stringify({"error": "unauthorized"}))
    }
})
```

```kryos
// Logging middleware
app_use(app, fn(req, next) {
    println("Request: " + json_get(req, "method") + " " + json_get(req, "path"))
    next()
})
```

**Edge cases:**

- Middleware must call `next()` to pass control to the next middleware or the route handler. If `next()` is not called, the request stops at that middleware.
- Middleware registered later runs closer to the route handler (outside-in execution order).

**See also:** `cors_middleware`, `logger_middleware`, `rate_limit`

---

## Built-in Middleware

### cors_middleware

```
cors_middleware
```

A pre-built middleware that adds CORS headers to all responses and handles OPTIONS preflight requests automatically. Pass it directly to `app_use` -- it is a function value, not a constructor.

Adds these headers:
- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS`
- `Access-Control-Allow-Headers: Content-Type, Authorization`

OPTIONS requests receive a 204 response with `Access-Control-Max-Age: 86400`.

**Example:**

```kryos
let app = http_app()
app_use(app, cors_middleware)
```

**See also:** `app_use`

---

### logger_middleware

```
logger_middleware
```

A pre-built middleware that prints request logs to stdout in the format: `METHOD /path STATUS TIMEms`.

**Example:**

```kryos
let app = http_app()
app_use(app, logger_middleware)
// Output: "  GET /api/users 200 3.2ms"
```

**See also:** `app_use`

---

### serve_static

```
serve_static(directory: str) -> fn
```

Return middleware that serves static files from the given directory. If a request path matches a file in the directory, the file is served with the appropriate MIME type. If no file matches, the request passes to the next middleware or route handler.

**Example:**

```kryos
let app = http_app()
app_use(app, serve_static("./public"))
// GET /index.html -> serves ./public/index.html
// GET /css/style.css -> serves ./public/css/style.css
```

**Edge cases:**

- Includes path traversal protection. Requests containing `..` that would escape the base directory are passed to the next handler.
- MIME types are auto-detected from the file extension.
- Files are served as UTF-8 text.

**See also:** `app_use`

---

### rate_limit

```
rate_limit(max_requests: i32, window_seconds: i32) -> fn
```

Return middleware that rate-limits requests by client IP address. If a client exceeds `max_requests` within `window_seconds`, they receive HTTP 429 with `{"error": "too many requests"}`.

**Example:**

```kryos
let app = http_app()
app_use(app, rate_limit(100, 60))  // 100 requests per 60 seconds
```

```kryos
// Strict rate limit for auth endpoint
let auth_app = http_app()
app_use(auth_app, rate_limit(5, 300))  // 5 attempts per 5 minutes
```

**Edge cases:**

- Uses the `X-Forwarded-For` header for client IP. Falls back to `127.0.0.1` if not present.
- Rate limit state is per-process and resets on server restart.
- Thread-safe -- uses locking internally.

**See also:** `app_use`

---

## Complete Example

A full HTTP API server with middleware, routing, and error handling:

```kryos
use std::server
use std::json

let app = http_app()

// Middleware stack
app_use(app, cors_middleware)
app_use(app, logger_middleware)
app_use(app, rate_limit(100, 60))

// Routes
app_get(app, "/api/health", fn(req) {
    respond(200, json_stringify({"status": "ok"}))
})

app_get(app, "/api/users/:id", fn(req) {
    let id = json_get(json_get(req, "params"), "id")
    respond(200, json_stringify({"id": id, "name": "Alice"}))
})

app_post(app, "/api/users", fn(req) {
    let body = json_parse(json_get(req, "body"))
    let name = json_get(body, "name")
    respond(201, json_stringify({"created": name}))
})

app_listen(app, 3000)
```
