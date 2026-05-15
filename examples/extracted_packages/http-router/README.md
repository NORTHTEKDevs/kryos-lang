# http-router

Simple HTTP request/response structs and routing helpers for Kryos.

## Installation

```
kryos pkg add http-router
```

## Usage

```kryos
import "http-router"

fn handle(req: Request) -> Response {
    if path_matches("/", req.path) {
        return make_response(200, "Welcome!")
    }
    if path_matches("/health", req.path) {
        return make_response(200, "OK")
    }
    if path_starts_with("/api/", req.path) {
        return make_response(200, "API: " + req.path)
    }
    return make_response(404, "Not Found")
}

fn main() {
    let req = Request { method: "GET", path: "/health", body: "" }
    let resp = handle(req)
    println(format_http_response(resp))
}
```

## API

### Types

- `struct Request { method: str, path: str, body: str }` — HTTP request
- `struct Response { status: i64, body: str }` — HTTP response

### Functions

- `fn make_response(status: i64, body: str) -> Response` — create a response
- `fn format_http_response(resp: Response) -> str` — full HTTP/1.1 wire format with Content-Length
- `fn format_http_response_json(resp: Response) -> str` — same but with `application/json` Content-Type
- `fn path_matches(pattern: str, path: str) -> bool` — exact match, prefix `/*`, or `*` wildcard
- `fn path_starts_with(prefix: str, path: str) -> bool` — prefix check
- `fn parse_request_line(raw: str) -> [str]` — parses `"GET /path HTTP/1.1"` into `[method, path]`
- `fn extract_body(raw: str) -> str` — returns body after blank-line separator
- `fn extract_path_segment(prefix: str, path: str) -> str` — extracts segment after prefix

## License

MIT — see https://github.com/NORTHTEKDevs/kryos-registry
