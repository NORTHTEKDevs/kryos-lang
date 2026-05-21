//! `kryos doc serve` — generate HTML docs into a temp dir, then serve them
//! over HTTP on 127.0.0.1:<port> until Ctrl-C.
//!
//! Useful for previewing changes to `///` doc comments without bouncing back
//! and forth between editor and file:// URLs.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct DocServeOptions {
    /// Bind address, e.g. "127.0.0.1:8088". Default 127.0.0.1:8088.
    pub address: Option<String>,
    /// Source files to document. Default: discover .kry files in cwd.
    pub files: Vec<String>,
}

pub fn execute(opts: DocServeOptions) -> Result<(), String> {
    let addr = opts.address.clone().unwrap_or_else(|| "127.0.0.1:8088".to_string());

    // Step 1: generate HTML docs into a temp directory.
    let tmp = std::env::temp_dir().join(format!("kryos_docs_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("kryos doc serve: mkdir {}: {e}", tmp.display()))?;
    eprintln!("\x1b[1mkryos doc serve\x1b[0m generating into {}", tmp.display());

    let tmp_str = tmp.to_string_lossy().to_string();
    super::doc::execute(&opts.files, Some(&tmp_str), true)?;

    // Step 2: bind a TCP listener.
    let listener = TcpListener::bind(&addr)
        .map_err(|e| format!("kryos doc serve: bind {addr}: {e}"))?;
    eprintln!("\x1b[32mlistening\x1b[0m on http://{addr}");
    eprintln!("press Ctrl-C to stop");

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Err(e) = handle_connection(stream, &tmp) {
            eprintln!("kryos doc serve: handler: {e}");
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream, root: &Path) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;

    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => 0,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = parse_request_path(&req).unwrap_or("/".to_string());
    let normalized = if path == "/" { "index.html".to_string() } else {
        path.trim_start_matches('/').to_string()
    };

    let file_path = root.join(&normalized);
    let response = match std::fs::read(&file_path) {
        Ok(body) => {
            let mime = guess_mime(&file_path);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let mut out = header.into_bytes();
            out.extend_from_slice(&body);
            out
        }
        Err(_) => {
            let body = b"<h1>404 Not Found</h1>";
            format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes()
            .into_iter()
            .chain(body.iter().copied())
            .collect()
        }
    };

    let _ = stream.write_all(&response);
    let _ = stream.flush();
    Ok(())
}

fn parse_request_path(req: &str) -> Option<String> {
    let first = req.lines().next()?;
    let mut parts = first.split_whitespace();
    let _method = parts.next()?;
    let path = parts.next()?;
    Some(path.to_string())
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "application/javascript",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        _ => "text/plain; charset=utf-8",
    }
}
