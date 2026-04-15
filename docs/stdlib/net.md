# std::net

HTTP client, TCP sockets, and URL parsing. Built around typed response structs: `HttpResponse`, `TcpStream`, and `TcpListener`.

HTTP requests use a 30-second default timeout and identify as `Kryos/1.0` in the user agent header.

```kryos
use std::net
```

---

## Types

### HttpResponse

The return type of all HTTP functions. Fields:

| Field        | Type  | Description                              |
|--------------|-------|------------------------------------------|
| `status`     | `i64` | HTTP status code (e.g. `200`, `404`)     |
| `body`       | `str` | Response body as a UTF-8 string          |
| `headers`    | `str` | Raw response headers                     |

**Example:**
```kryos
use std::net

let resp = http_get("https://example.com")
println(resp.status)   // 200
println(resp.body)     // HTML content
```

---

### TcpStream

A connected TCP socket returned by `connect`. Methods:

| Method             | Description                         |
|--------------------|-------------------------------------|
| `send(data: str)`  | Send data over the connection        |
| `recv() -> str`    | Receive available data               |
| `close()`          | Close the connection                 |

---

### TcpListener

A listening TCP server socket returned by `bind` or `listen`. Methods:

| Method              | Description                                    |
|---------------------|------------------------------------------------|
| `accept() -> TcpStream` | Block until a client connects; return the stream |
| `close()`           | Stop listening and release the port            |

---

## HTTP Client

### http_get

`http_get(url: str) -> HttpResponse`

Send an HTTP GET request to `url` and return the response.

**Example:**
```kryos
use std::net

let resp = http_get("https://api.example.com/data")
if resp.status == 200 {
    println(resp.body)
} else {
    println(resp.status)
}
```

---

### http_post

`http_post(url: str, body: str, content_type: str) -> HttpResponse`

Send an HTTP POST request with the given body and `Content-Type` header.

**Example:**
```kryos
use std::net

let resp = http_post(
    "https://api.example.com/submit",
    "name=Alice&score=42",
    "application/x-www-form-urlencoded"
)
println(resp.status)   // e.g. 201
```

---

### http_post_json

`http_post_json(url: str, json_body: str) -> HttpResponse`

Send an HTTP POST request with `Content-Type: application/json`. The `json_body` argument must be a valid JSON string.

**Example:**
```kryos
use std::net
use std::json

let payload = json_object(
    ["user", "score"],
    [json_string("Alice"), json_number(42.0)]
)
let resp = http_post_json("https://api.example.com/scores", stringify(payload))
println(resp.status)
println(resp.body)
```

---

## TCP Sockets

### connect

`connect(host: str, port: i32) -> TcpStream`

Open a TCP connection to `host:port` and return a `TcpStream`. Throws on connection failure.

**Example:**
```kryos
use std::net

let stream = connect("127.0.0.1", 8080)
stream.send("hello\n")
let response = stream.recv()
println(response)
stream.close()
```

---

### bind

`bind(host: str, port: i32) -> TcpListener`

Bind a TCP socket to `host:port` and return a `TcpListener`. Throws if the address is already in use.

**Example:**
```kryos
use std::net

let listener = bind("0.0.0.0", 9000)
let client = listener.accept()
let msg = client.recv()
println(msg)
client.close()
listener.close()
```

---

### listen

`listen(host: str, port: i32) -> TcpListener`

Alias for `bind`. Binds and returns a `TcpListener` ready to accept connections.

**Example:**
```kryos
use std::net

let server = listen("127.0.0.1", 3000)
let conn = server.accept()
conn.send("Welcome\n")
conn.close()
server.close()
```

---

## URL Utilities

### parse_url

`parse_url(url: str) -> [str]`

Parse a URL string into its components. Returns an array of strings in the order:
`[scheme, host, port, path, query, fragment]`.

Missing components are returned as empty strings.

**Example:**
```kryos
use std::net

let parts = parse_url("https://api.example.com:8443/v1/users?page=2#top")
println(parts[0])   // https
println(parts[1])   // api.example.com
println(parts[2])   // 8443
println(parts[3])   // /v1/users
println(parts[4])   // page=2
println(parts[5])   // top
```

---

## HTTP Response Utilities

### parse_http_response

`parse_http_response(raw: str) -> HttpResponse`

Parse a raw HTTP response string (e.g. from a low-level TCP socket) into an `HttpResponse` struct.

**Example:**
```kryos
use std::net

let stream = connect("example.com", 80)
stream.send("GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
let raw = stream.recv()
let resp = parse_http_response(raw)
println(resp.status)
println(resp.body)
stream.close()
```

---

## Complete Example

```kryos
use std::net
use std::json

// Make a GET request and parse the JSON response
let resp = http_get("https://api.example.com/status")

if resp.status == 200 {
    let data = parse(resp.body)
    let ok = to_bool(get(data, "ok"))
    println(ok)
} else {
    println(resp.status)
}

// POST JSON data
let payload = json_object(
    ["event", "user"],
    [json_string("login"), json_string("alice")]
)
let post_resp = http_post_json(
    "https://api.example.com/events",
    stringify(payload)
)
println(post_resp.status)

// Parse a URL
let parts = parse_url("https://example.com/api/v1?token=abc")
println(parts[1])   // example.com
println(parts[3])   // /api/v1
println(parts[4])   // token=abc
```
