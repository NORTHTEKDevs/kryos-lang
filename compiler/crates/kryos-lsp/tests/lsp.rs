//! Integration tests for the Kryos LSP server.
//!
//! Tests cover: initialization, completion, hover, go-to-definition, and diagnostics.
//! We drive the server by calling `handle_request` / `handle_notification` directly
//! with JSON-RPC payloads, then assert on the returned JSON values.

use kryos_lsp::LspServer;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a fresh server and initialize it via the "initialize" request.
fn initialized_server() -> LspServer {
    let mut server = LspServer::new();
    let resp = server
        .handle_request("initialize", &json!(1), &json!({}))
        .expect("initialize must return a response");
    assert!(
        resp.get("result").is_some(),
        "initialize response must have a result"
    );
    // Send the "initialized" notification (no response expected).
    let notifs = server.handle_notification("initialized", &json!({}));
    assert!(
        notifs.is_empty(),
        "initialized notification should produce no messages"
    );
    server
}

/// Open a document and return any notifications the server emits (diagnostics).
fn open_document(server: &mut LspServer, uri: &str, text: &str) -> Vec<Value> {
    server.handle_notification(
        "textDocument/didOpen",
        &json!({
            "textDocument": {
                "uri": uri,
                "languageId": "kryos",
                "version": 1,
                "text": text,
            }
        }),
    )
}

/// Send a completion request and return the response.
fn request_completion(server: &mut LspServer, uri: &str, line: u32, character: u32) -> Value {
    server
        .handle_request(
            "textDocument/completion",
            &json!(2),
            &json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .expect("completion must return a response")
}

/// Send a hover request and return the response.
fn request_hover(server: &mut LspServer, uri: &str, line: u32, character: u32) -> Value {
    server
        .handle_request(
            "textDocument/hover",
            &json!(3),
            &json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .expect("hover must return a response")
}

/// Send a go-to-definition request and return the response.
fn request_goto_definition(server: &mut LspServer, uri: &str, line: u32, character: u32) -> Value {
    server
        .handle_request(
            "textDocument/definition",
            &json!(4),
            &json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .expect("definition must return a response")
}

/// Extract completion labels from a completion response.
fn completion_labels(response: &Value) -> Vec<String> {
    response
        .pointer("/result/items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("label").and_then(|l| l.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// ===========================================================================
// 1. Server initialization
// ===========================================================================

#[test]
fn test_server_initializes_with_capabilities() {
    let mut server = LspServer::new();
    let resp = server
        .handle_request("initialize", &json!(1), &json!({}))
        .expect("initialize must return a response");

    let result = &resp["result"];

    // Capabilities
    let caps = &result["capabilities"];
    assert_eq!(caps["textDocumentSync"]["openClose"], json!(true));
    assert_eq!(caps["textDocumentSync"]["change"], json!(1));
    assert_eq!(caps["hoverProvider"], json!(true));
    assert_eq!(caps["definitionProvider"], json!(true));

    // Completion provider with trigger characters
    let trigger_chars = caps["completionProvider"]["triggerCharacters"]
        .as_array()
        .expect("triggerCharacters must be an array");
    let triggers: Vec<&str> = trigger_chars.iter().filter_map(|v| v.as_str()).collect();
    assert!(triggers.contains(&"."), "must trigger on '.'");
    assert!(triggers.contains(&":"), "must trigger on ':'");

    // Server info
    assert_eq!(result["serverInfo"]["name"], json!("kryos-lsp"));
    assert_eq!(result["serverInfo"]["version"], json!("0.1.0"));
}

#[test]
fn test_shutdown_returns_null_result() {
    let mut server = initialized_server();
    let resp = server
        .handle_request("shutdown", &json!(99), &json!({}))
        .expect("shutdown must return a response");
    assert_eq!(resp["result"], Value::Null);
}

#[test]
fn test_unknown_method_returns_error() {
    let mut server = initialized_server();
    let resp = server
        .handle_request("textDocument/doesNotExist", &json!(10), &json!({}))
        .expect("unknown method must return an error response");
    let error = &resp["error"];
    assert_eq!(error["code"], json!(-32601));
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("method not found"),
        "error message should mention 'method not found'"
    );
}

// ===========================================================================
// 2. Completion
// ===========================================================================

#[test]
fn test_completion_provides_keywords() {
    let mut server = initialized_server();
    let uri = "file:///test/keywords.kry";
    open_document(&mut server, uri, "fn main() { }");

    // Request completion at the beginning of the file body (line 0, col 14 = after "{ ")
    // With empty prefix, all keywords should appear.
    let resp = request_completion(&mut server, uri, 0, 14);
    let labels = completion_labels(&resp);

    let expected_keywords = [
        "let", "fn", "if", "for", "while", "match", "return", "struct", "enum",
    ];
    for kw in &expected_keywords {
        assert!(
            labels.contains(&kw.to_string()),
            "completion should include keyword '{kw}', got: {labels:?}"
        );
    }
}

#[test]
fn test_completion_provides_builtin_types() {
    let mut server = initialized_server();
    let uri = "file:///test/types.kry";
    open_document(&mut server, uri, "fn main() { }");

    let resp = request_completion(&mut server, uri, 0, 14);
    let labels = completion_labels(&resp);

    let expected_types = [
        "i32", "i64", "f64", "bool", "str", "Vec", "Map", "Option", "Result",
    ];
    for ty in &expected_types {
        assert!(
            labels.contains(&ty.to_string()),
            "completion should include type '{ty}', got: {labels:?}"
        );
    }
}

#[test]
fn test_completion_filters_by_prefix() {
    let mut server = initialized_server();
    let uri = "file:///test/prefix.kry";
    // Source: "fn main() { le }" -- cursor right after "le" at the trailing space (line 0, col 14)
    open_document(&mut server, uri, "fn main() { le }");

    let resp = request_completion(&mut server, uri, 0, 14);
    let labels = completion_labels(&resp);

    // "let" should match the "le" prefix
    assert!(
        labels.contains(&"let".to_string()),
        "should complete 'let' from prefix 'le'"
    );
    // "fn" should NOT match
    assert!(
        !labels.contains(&"fn".to_string()),
        "'fn' should not match prefix 'le'"
    );
}

#[test]
fn test_completion_includes_local_functions() {
    let mut server = initialized_server();
    let uri = "file:///test/local.kry";
    let source = "fn greet() { }\nfn main() { }";
    open_document(&mut server, uri, source);

    // Complete at start of main body (line 1, col 14)
    let resp = request_completion(&mut server, uri, 1, 14);
    let labels = completion_labels(&resp);

    assert!(
        labels.contains(&"greet".to_string()),
        "completion should include locally declared function 'greet', got: {labels:?}"
    );
    assert!(
        labels.contains(&"main".to_string()),
        "completion should include locally declared function 'main', got: {labels:?}"
    );
}

#[test]
fn test_completion_includes_local_structs_and_enums() {
    let mut server = initialized_server();
    let uri = "file:///test/structs.kry";
    let source = "struct Point { x: i32, y: i32 }\nenum Color { Red, Green, Blue }\nfn main() { }";
    open_document(&mut server, uri, source);

    let resp = request_completion(&mut server, uri, 2, 14);
    let labels = completion_labels(&resp);

    assert!(
        labels.contains(&"Point".to_string()),
        "completion should include struct 'Point'"
    );
    assert!(
        labels.contains(&"Color".to_string()),
        "completion should include enum 'Color'"
    );
}

#[test]
fn test_completion_for_unknown_document_returns_empty_items() {
    let mut server = initialized_server();
    let resp = request_completion(&mut server, "file:///nonexistent.kry", 0, 0);
    let items = resp
        .pointer("/result/items")
        .and_then(|v| v.as_array())
        .expect("items must be an array");
    assert!(
        items.is_empty(),
        "unknown document should yield empty completion list"
    );
}

// ===========================================================================
// 3. Hover
// ===========================================================================

#[test]
fn test_hover_on_keyword() {
    let mut server = initialized_server();
    let uri = "file:///test/hover_kw.kry";
    // "fn main() { }" -- hover over "fn" at (0, 0)
    open_document(&mut server, uri, "fn main() { }");

    let resp = request_hover(&mut server, uri, 0, 0);
    let contents = resp
        .pointer("/result/contents/value")
        .and_then(|v| v.as_str())
        .expect("hover should return markdown contents for keyword 'fn'");
    assert!(
        contents.contains("fn") && contents.contains("function"),
        "hover for 'fn' should describe function declaration, got: {contents}"
    );
}

#[test]
fn test_hover_on_builtin_type() {
    let mut server = initialized_server();
    let uri = "file:///test/hover_type.kry";
    // "fn add(x: i32) -> i32 { x }" -- hover over first "i32" at (0, 10)
    open_document(&mut server, uri, "fn add(x: i32) -> i32 { x }");

    let resp = request_hover(&mut server, uri, 0, 10);
    let contents = resp
        .pointer("/result/contents/value")
        .and_then(|v| v.as_str())
        .expect("hover should return info for 'i32'");
    assert!(
        contents.contains("i32") && contents.contains("32-bit"),
        "hover for 'i32' should describe the type, got: {contents}"
    );
}

#[test]
fn test_hover_on_function_shows_signature() {
    let mut server = initialized_server();
    let uri = "file:///test/hover_fn.kry";
    let source = "fn add(x: i32, y: i32) -> i32 { x }\nfn main() { add }";
    open_document(&mut server, uri, source);

    // Hover over "add" on line 1 (inside main body) at col 13
    let resp = request_hover(&mut server, uri, 1, 13);
    let contents = resp
        .pointer("/result/contents/value")
        .and_then(|v| v.as_str())
        .expect("hover should return signature for function 'add'");
    assert!(contents.contains("fn add"), "should contain function name");
    assert!(
        contents.contains("x: i32"),
        "should contain parameter types"
    );
    assert!(contents.contains("-> i32"), "should contain return type");
}

#[test]
fn test_hover_on_struct_shows_fields() {
    let mut server = initialized_server();
    let uri = "file:///test/hover_struct.kry";
    let source = "struct Point { x: i32, y: i32 }\nfn main() { Point }";
    open_document(&mut server, uri, source);

    // Hover over "Point" on line 1 at col 13
    let resp = request_hover(&mut server, uri, 1, 13);
    let contents = resp
        .pointer("/result/contents/value")
        .and_then(|v| v.as_str())
        .expect("hover should return struct info for 'Point'");
    assert!(
        contents.contains("struct Point"),
        "should contain struct name"
    );
    assert!(contents.contains("x: i32"), "should contain field x");
    assert!(contents.contains("y: i32"), "should contain field y");
}

#[test]
fn test_hover_on_unknown_symbol_returns_null() {
    let mut server = initialized_server();
    let uri = "file:///test/hover_unknown.kry";
    open_document(&mut server, uri, "fn main() { xyz }");

    let resp = request_hover(&mut server, uri, 0, 13);
    assert_eq!(
        resp["result"],
        Value::Null,
        "hover on unknown symbol should return null"
    );
}

// ===========================================================================
// 4. Go-to-definition
// ===========================================================================

#[test]
fn test_goto_definition_finds_function() {
    let mut server = initialized_server();
    let uri = "file:///test/goto_fn.kry";
    let source = "fn greet() { }\nfn main() { greet }";
    open_document(&mut server, uri, source);

    // Go to definition of "greet" referenced on line 1, col 12
    let resp = request_goto_definition(&mut server, uri, 1, 12);
    let result = &resp["result"];

    assert_eq!(
        result["uri"],
        json!(uri),
        "location URI should match the document"
    );

    let range = &result["range"];
    assert!(range.is_object(), "result should contain a range");

    // "fn greet() { }" starts at line 0
    assert_eq!(
        range["start"]["line"],
        json!(0),
        "greet definition should be on line 0"
    );
}

#[test]
fn test_goto_definition_finds_struct() {
    let mut server = initialized_server();
    let uri = "file:///test/goto_struct.kry";
    let source = "struct Point { x: i32, y: i32 }\nfn main() { Point }";
    open_document(&mut server, uri, source);

    // Go to definition of "Point" on line 1, col 13
    let resp = request_goto_definition(&mut server, uri, 1, 13);
    let result = &resp["result"];

    assert_eq!(result["uri"], json!(uri));
    assert_eq!(
        result["range"]["start"]["line"],
        json!(0),
        "Point definition should be on line 0"
    );
}

#[test]
fn test_goto_definition_unknown_symbol_returns_null() {
    let mut server = initialized_server();
    let uri = "file:///test/goto_unknown.kry";
    open_document(&mut server, uri, "fn main() { unknown_symbol }");

    let resp = request_goto_definition(&mut server, uri, 0, 13);
    assert_eq!(
        resp["result"],
        Value::Null,
        "go-to-definition on unknown symbol should return null"
    );
}

#[test]
fn test_goto_definition_for_unopened_document_returns_null() {
    let mut server = initialized_server();
    let resp = request_goto_definition(&mut server, "file:///nonexistent.kry", 0, 0);
    assert_eq!(resp["result"], Value::Null);
}

// ===========================================================================
// 5. Diagnostics on document open
// ===========================================================================

#[test]
fn test_diagnostics_published_on_open_valid_code() {
    let mut server = initialized_server();
    let uri = "file:///test/valid.kry";
    let notifs = open_document(&mut server, uri, "fn main() { }");

    // Should publish a diagnostics notification even for valid code (empty diagnostics list).
    assert_eq!(
        notifs.len(),
        1,
        "should emit exactly one publishDiagnostics notification"
    );

    let notif = &notifs[0];
    assert_eq!(notif["method"], json!("textDocument/publishDiagnostics"));
    assert_eq!(notif["params"]["uri"], json!(uri));

    let diags = notif["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics must be an array");
    assert!(
        diags.is_empty(),
        "valid code should produce zero diagnostics"
    );
}

#[test]
fn test_diagnostics_published_on_open_invalid_code() {
    let mut server = initialized_server();
    let uri = "file:///test/invalid.kry";
    // This is intentionally broken syntax -- missing closing brace.
    let notifs = open_document(&mut server, uri, "fn main() {");

    assert!(
        !notifs.is_empty(),
        "should emit a publishDiagnostics notification"
    );

    let notif = &notifs[0];
    assert_eq!(notif["method"], json!("textDocument/publishDiagnostics"));
    assert_eq!(notif["params"]["uri"], json!(uri));

    let diags = notif["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics must be an array");
    // The parser should report at least one error for the missing closing brace.
    assert!(
        !diags.is_empty(),
        "broken code should produce at least one diagnostic"
    );

    // Each diagnostic should have severity, range, and message.
    for d in diags {
        assert!(
            d.get("severity").is_some(),
            "diagnostic should have severity"
        );
        assert!(d.get("range").is_some(), "diagnostic should have range");
        assert!(d.get("message").is_some(), "diagnostic should have message");
        assert_eq!(
            d["source"],
            json!("kryos"),
            "diagnostic source should be 'kryos'"
        );
    }
}

#[test]
fn test_diagnostics_published_on_change() {
    let mut server = initialized_server();
    let uri = "file:///test/change.kry";
    open_document(&mut server, uri, "fn main() { }");

    // Send a didChange with broken code
    let notifs = server.handle_notification(
        "textDocument/didChange",
        &json!({
            "textDocument": { "uri": uri },
            "contentChanges": [{ "text": "fn main() {" }],
        }),
    );

    assert!(!notifs.is_empty(), "didChange should trigger diagnostics");
    let notif = &notifs[0];
    assert_eq!(notif["method"], json!("textDocument/publishDiagnostics"));

    let diags = notif["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics must be an array");
    assert!(
        !diags.is_empty(),
        "broken code after change should produce diagnostics"
    );
}

#[test]
fn test_diagnostics_cleared_on_close() {
    let mut server = initialized_server();
    let uri = "file:///test/close.kry";
    open_document(&mut server, uri, "fn main() { }");

    let notifs = server.handle_notification(
        "textDocument/didClose",
        &json!({ "textDocument": { "uri": uri } }),
    );

    assert_eq!(
        notifs.len(),
        1,
        "didClose should emit one notification to clear diagnostics"
    );
    let notif = &notifs[0];
    assert_eq!(notif["method"], json!("textDocument/publishDiagnostics"));
    let diags = notif["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics must be an array");
    assert!(
        diags.is_empty(),
        "closing a document should clear diagnostics"
    );
}

// ===========================================================================
// 6. Protocol layer
// ===========================================================================

#[test]
fn test_protocol_read_write_roundtrip() {
    use kryos_lsp::protocol;

    let msg = json!({"jsonrpc": "2.0", "method": "initialize", "id": 1, "params": {}});

    // Write the message to a buffer
    let mut buf: Vec<u8> = Vec::new();
    protocol::write_message(&mut buf, &msg).expect("write_message should succeed");

    // Read it back
    let mut cursor = std::io::Cursor::new(buf);
    let parsed = protocol::read_message(&mut cursor)
        .expect("read_message should not error")
        .expect("read_message should return Some");

    assert_eq!(parsed["method"], json!("initialize"));
    assert_eq!(parsed["id"], json!(1));
}

#[test]
fn test_protocol_parse_request() {
    use kryos_lsp::protocol::{self, Message};

    let value = json!({"jsonrpc": "2.0", "id": 42, "method": "shutdown", "params": {}});
    match protocol::parse_message(value) {
        Some(Message::Request(req)) => {
            assert_eq!(req.method, "shutdown");
            assert_eq!(req.id, json!(42));
        }
        other => panic!("expected Request, got {:?}", other),
    }
}

#[test]
fn test_protocol_parse_notification() {
    use kryos_lsp::protocol::{self, Message};

    let value = json!({"jsonrpc": "2.0", "method": "initialized", "params": {}});
    match protocol::parse_message(value) {
        Some(Message::Notification(notif)) => {
            assert_eq!(notif.method, "initialized");
        }
        other => panic!("expected Notification, got {:?}", other),
    }
}

#[test]
fn test_protocol_make_error_response() {
    use kryos_lsp::protocol;

    let resp = protocol::make_error_response(json!(5), -32601, "method not found");
    assert_eq!(resp["id"], json!(5));
    assert_eq!(resp["error"]["code"], json!(-32601));
    assert_eq!(resp["error"]["message"], json!("method not found"));
}

// ===========================================================================
// 7. Direct module-level tests (bypassing server)
// ===========================================================================

// -- diagnostics::check_source --

#[test]
fn test_check_source_valid_returns_empty() {
    let (diags, _sm) = kryos_lsp::diagnostics::check_source("fn main() { }", "file:///t.kry");
    assert!(
        diags.is_empty(),
        "valid source should yield no diagnostics, got: {diags:?}"
    );
}

#[test]
fn test_check_source_parse_error_returns_diagnostics() {
    let (diags, _sm) = kryos_lsp::diagnostics::check_source("fn main() {", "file:///t.kry");
    assert!(
        !diags.is_empty(),
        "missing closing brace should yield diagnostics"
    );
    // Every diagnostic should be an error (severity 1)
    for d in &diags {
        assert_eq!(
            d["severity"],
            json!(1),
            "parse errors should be severity 1 (Error)"
        );
    }
}

#[test]
fn test_check_source_type_error_pipeline_does_not_panic() {
    // Even if the type checker doesn't flag this, the pipeline should not crash.
    let (diags, _sm) = kryos_lsp::diagnostics::check_source(
        r#"fn main() { let x: i32 = "hello" }"#,
        "file:///t.kry",
    );
    let _ = diags; // Just verify no panic.
}

#[test]
fn test_publish_diagnostics_structure() {
    let fake_diag = json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } },
        "severity": 1,
        "source": "kryos",
        "message": "test error"
    });
    let notif = kryos_lsp::diagnostics::publish_diagnostics("file:///x.kry", vec![fake_diag]);
    assert_eq!(notif["jsonrpc"], json!("2.0"));
    assert_eq!(notif["method"], json!("textDocument/publishDiagnostics"));
    assert_eq!(notif["params"]["uri"], json!("file:///x.kry"));
    assert_eq!(notif["params"]["diagnostics"].as_array().unwrap().len(), 1);
}

// -- completion::get_completions --

#[test]
fn test_get_completions_std_modules() {
    let source = "use std::io";
    let items = kryos_lsp::completion::get_completions(source, 0, 10);
    let labels: Vec<String> = items
        .iter()
        .filter_map(|i| i["label"].as_str().map(String::from))
        .collect();
    assert!(
        labels.contains(&"io".to_string()),
        "should suggest 'io' in std:: context"
    );
    assert!(
        labels.contains(&"iter".to_string()),
        "should suggest 'iter' in std:: context"
    );
}

#[test]
fn test_get_completions_includes_all_keyword_kinds() {
    // Empty source, offset 0 = no prefix, should return everything
    let items = kryos_lsp::completion::get_completions("", 0, 0);
    let labels: Vec<String> = items
        .iter()
        .filter_map(|i| i["label"].as_str().map(String::from))
        .collect();
    // Spot-check keywords from different categories
    for kw in &[
        "let", "fn", "struct", "enum", "trait", "impl", "actor", "spawn", "select", "match", "try",
    ] {
        assert!(
            labels.contains(&kw.to_string()),
            "should include keyword '{kw}'"
        );
    }
}

#[test]
fn test_get_completions_item_kinds() {
    let source = "struct Foo { }\nfn bar() { }\nenum Baz { A }";
    let items = kryos_lsp::completion::get_completions(source, 0, 0);

    let foo = items
        .iter()
        .find(|i| i["label"] == "Foo")
        .expect("should find 'Foo'");
    assert_eq!(foo["kind"], 22, "struct kind should be 22");
    assert_eq!(foo["detail"], "struct");

    let bar = items
        .iter()
        .find(|i| i["label"] == "bar")
        .expect("should find 'bar'");
    assert_eq!(bar["kind"], 3, "function kind should be 3");
    assert_eq!(bar["detail"], "function");

    let baz = items
        .iter()
        .find(|i| i["label"] == "Baz")
        .expect("should find 'Baz'");
    assert_eq!(baz["kind"], 13, "enum kind should be 13");
    assert_eq!(baz["detail"], "enum");
}

// -- hover::get_hover --

#[test]
fn test_get_hover_returns_markdown() {
    let result = kryos_lsp::hover::get_hover("fn main() { }", 0, 0);
    let hover = result.expect("hover on 'fn' should return Some");
    assert_eq!(
        hover["contents"]["kind"], "markdown",
        "hover contents kind should be markdown"
    );
}

#[test]
fn test_get_hover_all_numeric_types() {
    for ty in &[
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
    ] {
        let source = format!("fn main() {{ let x: {} = 0 }}", ty);
        let result = kryos_lsp::hover::get_hover(&source, 0, 19);
        assert!(result.is_some(), "hover on '{}' should return info", ty);
        let contents = result.unwrap();
        let value = contents["contents"]["value"].as_str().unwrap();
        assert!(
            value.contains(ty),
            "hover for '{}' should mention the type, got: {}",
            ty,
            value
        );
    }
}

#[test]
fn test_get_hover_on_empty_source_returns_none() {
    let result = kryos_lsp::hover::get_hover("", 0, 0);
    assert!(result.is_none(), "hover on empty source should return None");
}

#[test]
fn test_get_hover_actor_keyword() {
    let result = kryos_lsp::hover::get_hover("actor MyActor { }", 0, 0);
    let hover = result.expect("hover on 'actor' should return info");
    let value = hover["contents"]["value"].as_str().unwrap();
    assert!(
        value.contains("actor"),
        "hover should mention 'actor': {value}"
    );
}

// -- goto_def::goto_definition --

#[test]
fn test_goto_definition_finds_enum() {
    let source = "enum Color { Red, Green, Blue }\nfn main() { Color }";
    let result = kryos_lsp::goto_def::goto_definition(source, 1, 13);
    let loc = result.expect("should find enum 'Color' definition");
    assert_eq!(
        loc["range"]["start"]["line"], 0,
        "Color is defined on line 0"
    );
}

#[test]
fn test_goto_definition_empty_source_returns_none() {
    let result = kryos_lsp::goto_def::goto_definition("", 0, 0);
    assert!(result.is_none(), "empty source should return None");
}

#[test]
fn test_goto_definition_self_reference() {
    // Hovering on a function name at its own declaration should still find the definition
    let source = "fn greet() { }";
    let result = kryos_lsp::goto_def::goto_definition(source, 0, 3);
    let loc = result.expect("should find definition of 'greet' even at declaration site");
    assert_eq!(loc["range"]["start"]["line"], 0);
}

#[test]
fn test_goto_definition_range_has_both_start_and_end() {
    let source = "fn hello() { }\nfn main() { hello }";
    let loc = kryos_lsp::goto_def::goto_definition(source, 1, 13).unwrap();
    let range = &loc["range"];
    // start and end should both be present and end should be >= start
    let start_char = range["start"]["character"].as_u64().unwrap();
    let end_char = range["end"]["character"].as_u64().unwrap();
    assert!(
        end_char >= start_char,
        "end character should be >= start character"
    );
}

// ===========================================================================
// 8. End-to-end: full JSON-RPC conversation over byte streams
// ===========================================================================

#[test]
fn test_full_jsonrpc_conversation() {
    use kryos_lsp::protocol;

    // Build a sequence of LSP messages as raw bytes (Content-Length framed).
    let mut input_buf: Vec<u8> = Vec::new();

    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    protocol::write_message(&mut input_buf, &init_request).unwrap();

    let initialized_notif = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    protocol::write_message(&mut input_buf, &initialized_notif).unwrap();

    let shutdown_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": {}
    });
    protocol::write_message(&mut input_buf, &shutdown_request).unwrap();

    // Feed all messages and collect responses by reading from the input buffer
    let mut reader = std::io::Cursor::new(input_buf);
    let mut output_buf: Vec<u8> = Vec::new();
    let mut server = LspServer::new();

    // Process messages manually (can't use server.run() since it reads stdin)
    loop {
        let msg = match protocol::read_message(&mut reader) {
            Ok(Some(value)) => value,
            Ok(None) => break,
            Err(_) => continue,
        };

        let parsed = match protocol::parse_message(msg) {
            Some(m) => m,
            None => continue,
        };

        match parsed {
            kryos_lsp::protocol::Message::Request(req) => {
                if let Some(resp) = server.handle_request(&req.method, &req.id, &req.params) {
                    protocol::write_message(&mut output_buf, &resp).unwrap();
                }
            }
            kryos_lsp::protocol::Message::Notification(notif) => {
                let notifs = server.handle_notification(&notif.method, &notif.params);
                for n in notifs {
                    protocol::write_message(&mut output_buf, &n).unwrap();
                }
            }
            _ => {}
        }
    }

    // Parse responses from output buffer
    let mut out_reader = std::io::Cursor::new(output_buf);
    let mut responses: Vec<Value> = Vec::new();
    while let Ok(Some(v)) = protocol::read_message(&mut out_reader) {
        responses.push(v);
    }

    // Should have exactly 2 responses: initialize result + shutdown result
    assert_eq!(
        responses.len(),
        2,
        "should get 2 responses (initialize + shutdown)"
    );

    // First response: initialize
    assert_eq!(responses[0]["id"], json!(1));
    assert!(responses[0]["result"]["capabilities"].is_object());

    // Second response: shutdown
    assert_eq!(responses[1]["id"], json!(2));
    assert_eq!(responses[1]["result"], Value::Null);
}
