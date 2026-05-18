# kryos-http-router

Minimal HTTP/1.1 method+path parser + response builder. Drop-in for any TCP accept loop.

## Install

```bash
kryos pkg add kryos-http-router
```

## Use

```kryos
use kryos_http_router::{parse_request, format_response, ok, not_found}
use std::net::{tcp_listen, accept, send, recv_str, close}

@capabilities(net, io)
fn main() {
    let listener = tcp_listen("127.0.0.1", 8080)
    loop {
        let conn = accept(listener)
        if conn < 0 { continue }
        let raw = recv_str(conn, 8192)
        let req = parse_request(raw)

        let resp = if req.path == "/" {
            ok("hello\n")
        } else {
            not_found()
        }

        send(conn, format_response(resp))
        close(conn)
    }
}
```

## API

| Function | Purpose |
| --- | --- |
| `parse_request(raw: str) -> Request` | Parse method + path + headers |
| `format_response(r: Response) -> str` | Build the wire string (with Content-Length + Connection: close) |
| `ok(body: str) -> Response` | 200 OK shortcut |
| `not_found() -> Response` | 404 shortcut |
| `internal_error(msg: str) -> Response` | 500 with custom body |

## Notes

- HTTP/1.1 only. No keep-alive, no chunked encoding, no HTTP/2.
- Headers are returned as raw `Name: value` strings — no parsing of header values.
- For multi-handler dispatch you wrap this in your own `match req.method + " " + req.path` block.
