//! Reference Kryos package registry server.
//!
//! Serves the on-disk Git index of the registry over a read-only HTTP/1.1
//! JSON API. Intentionally dependency-free; uses only `std`.
//!
//! Routes:
//!   GET /v1/health                       → "ok"
//!   GET /v1/packages/<name>              → all versions, NDJSON
//!   GET /v1/packages/<name>/<version>    → single version, JSON
//!   GET /v1/search?q=<query>             → matching names, JSON array
//!
//! Usage:
//!   kryos-registry-server --index /var/lib/kryos-registry --addr 0.0.0.0:8080
//!
//! Operational model:
//!   - The server periodically `git pull`s the index repo (background thread).
//!   - All data is read from the working tree.
//!   - There are no write endpoints. Publishing is PR-based.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_INDEX: &str = "./kryos-registry-index";
const PULL_INTERVAL_SECS: u64 = 300; // 5 minutes

fn main() {
    let mut addr = DEFAULT_ADDR.to_string();
    let mut index = PathBuf::from(DEFAULT_INDEX);
    let mut no_pull = false;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--addr" => {
                addr = args.get(i + 1).cloned().unwrap_or(addr);
                i += 2;
            }
            "--index" => {
                index = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                i += 2;
            }
            "--no-pull" => {
                no_pull = true;
                i += 1;
            }
            "--help" | "-h" => {
                print_usage();
                return;
            }
            _ => {
                eprintln!("unknown arg: {}", args[i]);
                print_usage();
                std::process::exit(2);
            }
        }
    }

    if !index.exists() {
        eprintln!(
            "index path does not exist: {} — clone the registry index first:",
            index.display()
        );
        eprintln!("  git clone https://github.com/NORTHTEKDevs/kryos-registry {}", index.display());
        std::process::exit(1);
    }

    let index = Arc::new(Mutex::new(index));

    // Background puller.
    if !no_pull {
        let index = Arc::clone(&index);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(PULL_INTERVAL_SECS));
            let path = { index.lock().unwrap().clone() };
            let _ = Command::new("git")
                .args(["pull", "--ff-only"])
                .current_dir(&path)
                .output();
        });
    }

    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        eprintln!("failed to bind {addr}: {e}");
        std::process::exit(1);
    });
    eprintln!("kryos-registry-server listening on http://{addr}");
    eprintln!("  index: {}", index.lock().unwrap().display());

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let index = Arc::clone(&index);
                thread::spawn(move || {
                    let _ = handle(s, index);
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage: kryos-registry-server [--addr ADDR] [--index PATH] [--no-pull]

Options:
  --addr ADDR    Bind address (default: {DEFAULT_ADDR})
  --index PATH   Path to the index Git checkout (default: {DEFAULT_INDEX})
  --no-pull      Disable background git pull
  --help, -h     Print this help"
    );
}

// ─── HTTP handler ──────────────────────────────────────────────────────────

fn handle(mut stream: TcpStream, index: Arc<Mutex<PathBuf>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    // Parse request line.
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let request_line = request_line.trim_end();
    let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return write_response(&mut stream, 400, "text/plain", b"bad request");
    }
    let method = parts[0];
    let target = parts[1];

    // Drain headers.
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    if method != "GET" {
        return write_response(&mut stream, 405, "text/plain", b"method not allowed");
    }

    let (path, query) = match target.find('?') {
        Some(i) => (&target[..i], &target[i + 1..]),
        None => (target, ""),
    };

    let index_path = { index.lock().unwrap().clone() };

    match route(path, query, &index_path) {
        Ok((status, content_type, body)) => write_response(&mut stream, status, content_type, &body),
        Err(e) => {
            eprintln!("handler error: {e}");
            write_response(&mut stream, 500, "text/plain", b"internal error")
        }
    }
}

fn route(path: &str, query: &str, index: &Path) -> Result<(u16, &'static str, Vec<u8>), String> {
    if path == "/v1/health" {
        return Ok((200, "text/plain", b"ok".to_vec()));
    }

    if path == "/v1/search" {
        let params = parse_query(query);
        let q = params.get("q").map(|s| s.as_str()).unwrap_or("");
        if q.is_empty() {
            return Ok((400, "application/json", b"{\"error\":\"missing q\"}".to_vec()));
        }
        let results = search_index(index, q)?;
        let body = json_string_array(&results);
        return Ok((200, "application/json", body.into_bytes()));
    }

    if let Some(rest) = path.strip_prefix("/v1/packages/") {
        let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        match segs.len() {
            1 => {
                let name = segs[0];
                if !is_valid_name(name) {
                    return Ok((400, "application/json", b"{\"error\":\"bad name\"}".to_vec()));
                }
                let body = read_index_entry(index, name)?;
                match body {
                    Some(b) => Ok((200, "application/x-ndjson", b.into_bytes())),
                    None => Ok((404, "application/json", b"{\"error\":\"not found\"}".to_vec())),
                }
            }
            2 => {
                let name = segs[0];
                let version = segs[1];
                if !is_valid_name(name) {
                    return Ok((400, "application/json", b"{\"error\":\"bad name\"}".to_vec()));
                }
                let body = read_index_entry(index, name)?.unwrap_or_default();
                for line in body.lines() {
                    if extract_field(line, "version").as_deref() == Some(version) {
                        return Ok((200, "application/json", line.as_bytes().to_vec()));
                    }
                }
                Ok((404, "application/json", b"{\"error\":\"version not found\"}".to_vec()))
            }
            _ => Ok((404, "application/json", b"{\"error\":\"not found\"}".to_vec())),
        }
    } else {
        Ok((404, "application/json", b"{\"error\":\"not found\"}".to_vec()))
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: public, max-age=60\r\n\
         X-Kryos-Registry: 1\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

// ─── index reads ───────────────────────────────────────────────────────────

fn read_index_entry(index: &Path, name: &str) -> Result<Option<String>, String> {
    let prefix = if name.len() >= 2 { &name[..2] } else { name };
    let p = index.join(prefix).join(format!("{name}.json"));
    if !p.exists() {
        return Ok(None);
    }
    let mut buf = String::new();
    std::fs::File::open(&p)
        .and_then(|mut f| f.read_to_string(&mut buf))
        .map_err(|e| format!("read {p:?}: {e}"))?;
    Ok(Some(buf))
}

fn search_index(index: &Path, query: &str) -> Result<Vec<String>, String> {
    let q = query.to_lowercase();
    let mut out = Vec::new();
    let entries = std::fs::read_dir(index).map_err(|e| format!("read {index:?}: {e}"))?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = match p.file_name().and_then(|n| n.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let subs = match std::fs::read_dir(&p) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for sub in subs.flatten() {
            let path = sub.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.to_lowercase().contains(&q) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

// ─── minimal helpers (no serde) ────────────────────────────────────────────

fn parse_query(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in s.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            out.insert(url_decode(k), url_decode(v));
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(' ');
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            let c = u8::from_str_radix(h, 16).unwrap_or(b'?');
            out.push(c as char);
            i += 3;
        } else {
            out.push(b as char);
            i += 1;
        }
    }
    out
}

fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn extract_field(json_line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let start = json_line.find(&pat)?;
    let after = &json_line[start + pat.len()..];
    let quote = after.find('"')?;
    let rest = &after[quote + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_string_array(items: &[String]) -> String {
    let mut s = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('"');
        for c in item.chars() {
            match c {
                '"' => s.push_str("\\\""),
                '\\' => s.push_str("\\\\"),
                '\n' => s.push_str("\\n"),
                '\r' => s.push_str("\\r"),
                '\t' => s.push_str("\\t"),
                c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
                c => s.push(c),
            }
        }
        s.push('"');
    }
    s.push(']');
    s
}
