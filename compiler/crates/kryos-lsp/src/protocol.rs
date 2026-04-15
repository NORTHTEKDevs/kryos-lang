//! JSON-RPC protocol types for LSP communication.

use serde_json::Value;

/// JSON-RPC request.
#[derive(Debug, Clone)]
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

/// JSON-RPC response.
#[derive(Debug, Clone)]
pub struct Response {
    pub id: Value,
    pub result: Option<Value>,
    pub error: Option<ResponseError>,
}

#[derive(Debug, Clone)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

/// JSON-RPC notification (no id, no response expected).
#[derive(Debug, Clone)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

/// LSP message types.
#[derive(Debug)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

/// Parse a JSON-RPC message from a JSON value.
pub fn parse_message(value: Value) -> Option<Message> {
    let obj = value.as_object()?;
    let method = obj.get("method").and_then(|m| m.as_str()).map(String::from);
    let id = obj.get("id").cloned();
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    match (id, method) {
        (Some(id), Some(method)) => Some(Message::Request(Request { id, method, params })),
        (None, Some(method)) => Some(Message::Notification(Notification { method, params })),
        (Some(id), None) => {
            let result = obj.get("result").cloned();
            let error = obj.get("error").and_then(|e| {
                let eo = e.as_object()?;
                Some(ResponseError {
                    code: eo.get("code")?.as_i64()? as i32,
                    message: eo.get("message")?.as_str()?.to_string(),
                    data: eo.get("data").cloned(),
                })
            });
            Some(Message::Response(Response { id, result, error }))
        }
        _ => None,
    }
}

/// Serialize a response to JSON.
pub fn make_response(id: Value, result: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

/// Serialize an error response to JSON.
pub fn make_error_response(id: Value, code: i32, message: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        },
    })
}

/// Serialize a notification to JSON.
pub fn make_notification(method: &str, params: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
}

/// Read a single LSP message from a reader (Content-Length header + JSON body).
pub fn read_message(reader: &mut impl std::io::BufRead) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut header_line = String::new();

    // Read headers
    loop {
        header_line.clear();
        let bytes_read = reader.read_line(&mut header_line)?;
        if bytes_read == 0 {
            return Ok(None); // EOF
        }
        let line = header_line.trim();
        if line.is_empty() {
            break; // End of headers
        }
        if let Some(len_str) = line.strip_prefix("Content-Length: ") {
            content_length = len_str.parse().ok();
        }
    }

    let length = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    let value: Value = serde_json::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(Some(value))
}

/// Write an LSP message to a writer.
pub fn write_message(writer: &mut impl std::io::Write, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    writer.flush()
}
