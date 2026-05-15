//! HTTP/2 client for Kryos — backed by reqwest's blocking client.
//!
//! The client uses ALPN to negotiate HTTP/2 automatically, falling back to
//! HTTP/1.1 when the server doesn't support h2. A single `OnceLock` client
//! instance is shared across all calls so the connection pool is reused.

use std::sync::OnceLock;

#[cfg(feature = "http2")]
use reqwest::blocking::Client;

#[cfg(feature = "http2")]
static CLIENT: OnceLock<Client> = OnceLock::new();

#[cfg(feature = "http2")]
fn get_client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent("kryos/1.6")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build reqwest client")
    })
}

/// Perform an HTTP GET and return the response body. Returns empty string on error.
#[cfg(feature = "http2")]
pub fn http2_get(url: &str) -> String {
    match get_client().get(url).send() {
        Ok(resp) => resp.text().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Perform an HTTP POST with the given body and return the response body.
/// Returns empty string on error.
#[cfg(feature = "http2")]
pub fn http2_post(url: &str, body: &str) -> String {
    match get_client()
        .post(url)
        .body(body.to_string())
        .send()
    {
        Ok(resp) => resp.text().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Perform a full HTTP request with method, url, headers (newline-delimited), and body.
/// Returns `"{status}\n{response_headers}\n\n{body}"` or empty string on error.
#[cfg(feature = "http2")]
pub fn http2_request(method: &str, url: &str, headers: &str, body: &str) -> String {
    use reqwest::header::{HeaderName, HeaderValue};

    let method_str = if method.is_empty() { "GET" } else { method };
    let req_method = match reqwest::Method::from_bytes(method_str.as_bytes()) {
        Ok(m) => m,
        Err(_) => reqwest::Method::GET,
    };

    let mut builder = get_client()
        .request(req_method, url)
        .body(body.to_string());

    // Parse headers: "Name1: val1\nName2: val2"
    for line in headers.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(colon_pos) = line.find(':') {
            let name = line[..colon_pos].trim();
            let value = line[colon_pos + 1..].trim();
            if let (Ok(hname), Ok(hval)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                builder = builder.header(hname, hval);
            }
        }
    }

    let resp = match builder.send() {
        Ok(r) => r,
        Err(_) => return String::new(),
    };

    let status = resp.status().as_u16();

    // Build response headers block
    let mut resp_headers = String::new();
    for (name, value) in resp.headers() {
        resp_headers.push_str(name.as_str());
        resp_headers.push_str(": ");
        resp_headers.push_str(value.to_str().unwrap_or(""));
        resp_headers.push('\n');
    }

    let resp_body = resp.text().unwrap_or_default();

    format!("{status}\n{resp_headers}\n{resp_body}")
}

// Stubs when http2 feature is disabled
#[cfg(not(feature = "http2"))]
pub fn http2_get(_url: &str) -> String {
    String::new()
}

#[cfg(not(feature = "http2"))]
pub fn http2_post(_url: &str, _body: &str) -> String {
    String::new()
}

#[cfg(not(feature = "http2"))]
pub fn http2_request(_method: &str, _url: &str, _headers: &str, _body: &str) -> String {
    String::new()
}
