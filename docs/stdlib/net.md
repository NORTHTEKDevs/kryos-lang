# std::net

HTTP client, WebSocket, TCP socket, and URL utility functions.

All functions in this module are available after `use std::net`. HTTP requests use a 30-second timeout and identify as `Kryos/1.0` user agent. WebSocket support requires the optional `websocket-client` Python package.

---

## HTTP

### http_get

```
http_get(url: str) -> str
```

Send an HTTP GET request and return the response body as a string.

**Example:**

```kryos
let html = http_get("https://example.com")
println(html)
```

**Edge cases:**

- Throws a runtime error if the request fails (network error, DNS failure, timeout).
- Does not throw on HTTP error status codes (4xx, 5xx) -- use `http_request` if you need the status code.
- Timeout is 30 seconds.

**See also:** `http_get_json`, `http_request`

---

### http_post

```
http_post(url: str, body: str, content_type: str?) -> str
```

Send an HTTP POST request with a string body. Returns the response body as a string. The optional third argument sets the `Content-Type` header (defaults to `"application/json"`).

**Example:**

```kryos
let payload = json_stringify({"name": "kryos", "version": 1})
let response = http_post("https://api.example.com/data", payload)
println(response)
```

```kryos
let xml = "<item><name>test</name></item>"
let resp = http_post("https://api.example.com/xml", xml, "application/xml")
```

**Edge cases:**

- The body is converted to a string and encoded as UTF-8.
- Throws a runtime error on network failure or timeout.

**See also:** `http_post_json`, `http_request`

---

### http_request

```
http_request(method: str, url: str, headers: map?, body: str?) -> map
```

Send a full HTTP request with control over method, headers, and body. Returns a map with three keys: `status` (integer), `body` (string), and `headers` (map).

**Example:**

```kryos
let resp = http_request("PUT", "https://api.example.com/items/1", 
    {"Authorization": "Bearer token123", "Content-Type": "application/json"},
    json_stringify({"name": "updated"})
)
println("Status: " + to_string(json_get(resp, "status")))
println("Body: " + json_get(resp, "body"))
```

```kryos
let resp = http_request("DELETE", "https://api.example.com/items/1",
    {"Authorization": "Bearer token123"}
)
```

**Edge cases:**

- On HTTP error responses (4xx, 5xx), returns the response normally with the error status code. Does not throw.
- On network-level failures (DNS, timeout, connection refused), throws a runtime error.
- The `method` string is uppercased automatically.
- `User-Agent: Kryos/1.0` is always included.

**See also:** `http_get`, `http_post`

---

### http_get_json

```
http_get_json(url: str, headers: map?) -> any
```

Send an HTTP GET request and parse the response as JSON. Returns the parsed value (map, array, string, number, bool, or none). The optional second argument is a headers map.

**Example:**

```kryos
let data = http_get_json("https://api.example.com/users")
for user in data {
    println(json_get(user, "name"))
}
```

```kryos
let data = http_get_json("https://api.example.com/me", 
    {"Authorization": "Bearer token123"}
)
```

**Edge cases:**

- Throws a runtime error if the response is not valid JSON.
- Sets `Accept: application/json` header automatically.

**See also:** `http_get`, `http_post_json`

---

### http_post_json

```
http_post_json(url: str, data: any, headers: map?) -> any
```

Send an HTTP POST request with a JSON body and parse the JSON response. The `data` argument is serialized to JSON automatically. Returns the parsed response.

**Example:**

```kryos
let result = http_post_json("https://api.example.com/users", 
    {"name": "Alice", "role": "admin"}
)
println(json_get(result, "id"))
```

**Edge cases:**

- The `data` argument is serialized with `json.dumps` -- it must be a JSON-compatible value.
- Sets both `Content-Type: application/json` and `Accept: application/json`.
- Throws if the response is not valid JSON.

**See also:** `http_post`, `http_get_json`

---

## WebSocket

WebSocket functions use an integer handle to track connections. Always close handles when done.

### ws_connect

```
ws_connect(url: str, headers: map?) -> i32
```

Open a WebSocket connection. Returns a handle (integer) used by `ws_send`, `ws_recv`, and `ws_close`. The optional second argument is a headers map for the handshake.

**Example:**

```kryos
let ws = ws_connect("wss://stream.example.com/v1")
```

```kryos
let ws = ws_connect("wss://api.example.com/ws", 
    {"Authorization": "Bearer token123"}
)
```

**Edge cases:**

- Requires the `websocket-client` Python package. Throws a runtime error with installation instructions if not available.
- Connection timeout is 30 seconds.
- Throws on connection failure.

**See also:** `ws_send`, `ws_recv`, `ws_close`

---

### ws_send

```
ws_send(handle: i32, data: str) -> bool
```

Send a string message on an open WebSocket connection. Returns `true` on success.

**Example:**

```kryos
let ws = ws_connect("wss://stream.example.com/v1")
ws_send(ws, json_stringify({"type": "subscribe", "channel": "prices"}))
```

**Edge cases:**

- Throws if the handle is not a valid WebSocket.
- Throws if the connection has been closed.

**See also:** `ws_recv`, `ws_close`

---

### ws_recv

```
ws_recv(handle: i32) -> str
```

Receive the next message from a WebSocket connection. Blocks until a message arrives.

**Example:**

```kryos
let ws = ws_connect("wss://stream.example.com/v1")
ws_send(ws, json_stringify({"type": "subscribe", "channel": "prices"}))
let msg = ws_recv(ws)
println("Received: " + msg)
ws_close(ws)
```

**Edge cases:**

- Blocks indefinitely until a message is received or the connection drops.
- Throws if the handle is invalid or the connection is closed by the server.

**See also:** `ws_send`, `ws_close`

---

### ws_close

```
ws_close(handle: i32) -> none
```

Close a WebSocket connection and release its handle.

**Example:**

```kryos
let ws = ws_connect("wss://stream.example.com/v1")
// ... use the connection ...
ws_close(ws)
```

**Edge cases:**

- Safe to call even if the connection has already been closed by the server.
- The handle becomes invalid after this call.

**See also:** `ws_connect`

---

## TCP

TCP functions use integer handles, similar to WebSocket handles.

### tcp_connect

```
tcp_connect(host: str, port: i32) -> i32
```

Open a TCP connection to a host and port. Returns a handle (integer).

**Example:**

```kryos
let sock = tcp_connect("example.com", 80)
tcp_send(sock, "GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
let response = tcp_recv(sock)
println(response)
tcp_close(sock)
```

**Edge cases:**

- Connection timeout is 30 seconds.
- Throws on connection failure (host unreachable, port closed, DNS failure).

**See also:** `tcp_send`, `tcp_recv`, `tcp_close`

---

### tcp_send

```
tcp_send(handle: i32, data: str) -> i32
```

Send data on a TCP connection. Returns the number of bytes sent.

**Example:**

```kryos
let sock = tcp_connect("example.com", 80)
let bytes_sent = tcp_send(sock, "PING\n")
println("Sent " + to_string(bytes_sent) + " bytes")
```

**Edge cases:**

- Data is encoded as UTF-8 before sending.
- Throws if the handle is not a valid TCP socket.
- May send fewer bytes than the data length in a single call.

**See also:** `tcp_recv`, `tcp_close`

---

### tcp_recv

```
tcp_recv(handle: i32, max_bytes: i32?) -> str
```

Receive data from a TCP connection. Blocks until data is available. The optional second argument sets the maximum bytes to read (defaults to 4096).

**Example:**

```kryos
let sock = tcp_connect("example.com", 80)
tcp_send(sock, "GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
let data = tcp_recv(sock)
println(data)
tcp_close(sock)
```

```kryos
let chunk = tcp_recv(sock, 1024)  // Read at most 1024 bytes
```

**Edge cases:**

- Blocks until at least one byte is available.
- Returns an empty string if the connection is closed by the remote side.
- Data is decoded as UTF-8.

**See also:** `tcp_send`, `tcp_close`

---

### tcp_close

```
tcp_close(handle: i32) -> none
```

Close a TCP connection and release its handle.

**Example:**

```kryos
tcp_close(sock)
```

**Edge cases:**

- Safe to call even if the connection has already been closed.
- The handle becomes invalid after this call.

**See also:** `tcp_connect`

---

## URL Utilities

### url_encode

```
url_encode(s: str) -> str
```

Percent-encode a string for use in URLs. All characters except ASCII letters, digits, and `_.-~` are encoded.

**Example:**

```kryos
let encoded = url_encode("hello world")
println(encoded)  // "hello%20world"
```

```kryos
let query = "q=" + url_encode("kryos language")
let url = "https://search.example.com?" + query
```

**Edge cases:**

- Encodes all special characters including `/`, `&`, `=`, and spaces.
- Safe to call on strings that are already partially encoded (but will double-encode them).

**See also:** `url_decode`

---

### url_decode

```
url_decode(s: str) -> str
```

Decode a percent-encoded string.

**Example:**

```kryos
let decoded = url_decode("hello%20world")
println(decoded)  // "hello world"
```

**Edge cases:**

- Decodes `%XX` sequences and `+` as space.

**See also:** `url_encode`
