//! JSON parsing and serialization for the Kryos native stdlib.
//!
//! Provides a handwritten JSON parser/serializer with no external dependencies.
//! JSON values are represented as i64 handles pointing to heap-allocated JsonNode objects.
//!
//! KryosString layout: `{ len: i64, cap: i64, data: *mut u8 }`
//! KryosArray layout: `{ len: i64, cap: i64, elem_size: i64, ref_count: i64, data: *mut u8 }`

use std::alloc::Layout;
use std::alloc::{alloc, dealloc};
use std::collections::HashMap;
use std::ptr;
use std::sync::{atomic::AtomicI64, Mutex};

#[repr(C)]
struct KryosString {
    len: i64,
    cap: i64,
    data: *mut u8,
}

#[repr(C)]
struct KryosArray {
    len: i64,
    cap: i64,
    elem_size: i64,
    ref_count: i64,
    data: *mut u8,
}

enum JsonNode {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<i64>),            // handles to child nodes
    Object(Vec<(String, i64)>), // key-value pairs; values are handles
}

static NODE_TABLE: Mutex<Option<NodeTable>> = Mutex::new(None);
static NEXT_NODE_ID: AtomicI64 = AtomicI64::new(1000);

struct NodeTable {
    map: HashMap<i64, Box<JsonNode>>,
}

#[allow(dead_code)]
impl NodeTable {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    fn insert(&mut self, node: JsonNode) -> i64 {
        let id = NEXT_NODE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.map.insert(id, Box::new(node));
        id
    }

    fn get(&self, id: i64) -> Option<&JsonNode> {
        self.map.get(&id).map(|b| b.as_ref())
    }

    fn get_mut(&mut self, id: i64) -> Option<&mut JsonNode> {
        self.map.get_mut(&id).map(|b| b.as_mut())
    }

    fn remove(&mut self, id: i64) -> Option<Box<JsonNode>> {
        self.map.remove(&id)
    }
}

fn with_node_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut NodeTable) -> R,
{
    let mut guard = NODE_TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let table = guard.get_or_insert_with(NodeTable::new);
    f(table)
}

fn ks_to_string(handle: i64) -> Option<String> {
    if handle == 0 {
        return None;
    }
    unsafe {
        let ks = handle as *const KryosString;
        if ks.is_null() {
            return None;
        }
        let len = (*ks).len as usize;
        let data = (*ks).data;
        if data.is_null() {
            return None;
        }
        let slice = std::slice::from_raw_parts(data, len);
        Some(String::from_utf8_lossy(slice).into_owned())
    }
}

fn string_to_ks(s: &str) -> i64 {
    unsafe {
        let len = s.len() as i64;
        let cap = len;
        let layout = Layout::from_size_align(cap as usize + 1, 1).unwrap();
        let data = alloc(layout);
        if data.is_null() {
            return 0;
        }
        if len > 0 {
            ptr::copy_nonoverlapping(s.as_ptr(), data, len as usize);
        }
        *data.add(len as usize) = 0;

        let ks = alloc(Layout::new::<KryosString>()) as *mut KryosString;
        if ks.is_null() {
            dealloc(data, layout);
            return 0;
        }
        (*ks).len = len;
        (*ks).cap = cap;
        (*ks).data = data;
        ks as i64
    }
}

fn alloc_node(node: JsonNode) -> i64 {
    with_node_table(|table| table.insert(node))
}

fn with_node<F, R>(handle: i64, f: F) -> Option<R>
where
    F: FnOnce(&JsonNode) -> R,
{
    with_node_table(|table| table.get(handle).map(f))
}

// ============================================================================
// JSON Parser (Recursive Descent)
// ============================================================================

struct Parser {
    chars: Vec<u8>,
    pos: usize,
}

impl Parser {
    fn new(s: &str) -> Self {
        Self {
            chars: s.as_bytes().to_vec(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        if self.pos < self.chars.len() {
            Some(self.chars[self.pos])
        } else {
            None
        }
    }

    fn advance(&mut self) -> Option<u8> {
        if self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<i64, ()> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'n') => self.parse_null(),
            Some(b't') | Some(b'f') => self.parse_bool(),
            Some(b'"') => self.parse_string(),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            _ => Err(()),
        }
    }

    fn parse_null(&mut self) -> Result<i64, ()> {
        if self.consume_str("null") {
            Ok(alloc_node(JsonNode::Null))
        } else {
            Err(())
        }
    }

    fn parse_bool(&mut self) -> Result<i64, ()> {
        if self.consume_str("true") {
            Ok(alloc_node(JsonNode::Bool(true)))
        } else if self.consume_str("false") {
            Ok(alloc_node(JsonNode::Bool(false)))
        } else {
            Err(())
        }
    }

    fn consume_str(&mut self, s: &str) -> bool {
        let bytes = s.as_bytes();
        if self.pos + bytes.len() > self.chars.len() {
            return false;
        }
        for (i, &b) in bytes.iter().enumerate() {
            if self.chars[self.pos + i] != b {
                return false;
            }
        }
        self.pos += bytes.len();
        true
    }

    fn parse_string(&mut self) -> Result<i64, ()> {
        if self.advance() != Some(b'"') {
            return Err(());
        }
        let mut result = String::new();
        loop {
            match self.advance() {
                None => return Err(()),
                Some(b'"') => return Ok(alloc_node(JsonNode::Str(result))),
                Some(b'\\') => {
                    match self.advance() {
                        Some(b'"') => result.push('"'),
                        Some(b'\\') => result.push('\\'),
                        Some(b'/') => result.push('/'),
                        Some(b'b') => result.push('\x08'),
                        Some(b'f') => result.push('\x0c'),
                        Some(b'n') => result.push('\n'),
                        Some(b'r') => result.push('\r'),
                        Some(b't') => result.push('\t'),
                        Some(b'u') => {
                            // Simple \uXXXX parsing: read 4 hex digits, assume valid
                            let mut code = 0u32;
                            for _ in 0..4 {
                                if let Some(ch) = self.advance() {
                                    let digit = match ch {
                                        b'0'..=b'9' => (ch - b'0') as u32,
                                        b'a'..=b'f' => (ch - b'a' + 10) as u32,
                                        b'A'..=b'F' => (ch - b'A' + 10) as u32,
                                        _ => return Err(()),
                                    };
                                    code = (code << 4) | digit;
                                } else {
                                    return Err(());
                                }
                            }
                            if let Some(c) = char::from_u32(code) {
                                result.push(c);
                            } else {
                                return Err(());
                            }
                        }
                        _ => return Err(()),
                    }
                }
                Some(ch) => result.push(ch as char),
            }
        }
    }

    fn parse_number(&mut self) -> Result<i64, ()> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.advance();
        }
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(());
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.advance();
        }
        if self.peek() == Some(b'.') {
            self.advance();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance();
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.advance();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.advance();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.advance();
            }
        }
        let num_str = std::str::from_utf8(&self.chars[start..self.pos]).map_err(|_| ())?;
        let val = num_str.parse::<f64>().map_err(|_| ())?;
        Ok(alloc_node(JsonNode::Number(val)))
    }

    fn parse_array(&mut self) -> Result<i64, ()> {
        if self.advance() != Some(b'[') {
            return Err(());
        }
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.advance();
            return Ok(alloc_node(JsonNode::Array(items)));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.skip_whitespace();
                }
                Some(b']') => {
                    self.advance();
                    return Ok(alloc_node(JsonNode::Array(items)));
                }
                _ => return Err(()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<i64, ()> {
        if self.advance() != Some(b'{') {
            return Err(());
        }
        let mut pairs = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok(alloc_node(JsonNode::Object(pairs)));
        }
        loop {
            let handle = self.parse_string()?;
            let key = match with_node(handle, |n| match n {
                JsonNode::Str(s) => Some(s.clone()),
                _ => None,
            }) {
                Some(Some(s)) => s,
                _ => return Err(()),
            };
            self.skip_whitespace();
            if self.advance() != Some(b':') {
                return Err(());
            }
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.skip_whitespace();
                }
                Some(b'}') => {
                    self.advance();
                    return Ok(alloc_node(JsonNode::Object(pairs)));
                }
                _ => return Err(()),
            }
        }
    }
}

// ============================================================================
// JSON Serializer
// ============================================================================

/// Stringify a node, looking up child handles in `table` directly to avoid
/// re-locking the global mutex on every recursion (which would deadlock).
fn stringify_node_with_table(node: &JsonNode, out: &mut String, table: &NodeTable) {
    match node {
        JsonNode::Null => out.push_str("null"),
        JsonNode::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonNode::Number(n) => {
            if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                out.push_str(&format!("{}", *n as i64));
            } else {
                out.push_str(&format!("{}", n));
            }
        }
        JsonNode::Str(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\x08' => out.push_str("\\b"),
                    '\x0c' => out.push_str("\\f"),
                    ch => {
                        if ch.is_control() {
                            out.push_str(&format!("\\u{:04x}", ch as u32));
                        } else {
                            out.push(ch);
                        }
                    }
                }
            }
            out.push('"');
        }
        JsonNode::Array(items) => {
            out.push('[');
            for (i, &handle) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Some(item) = table.get(handle) {
                    stringify_node_with_table(item, out, table);
                }
            }
            out.push(']');
        }
        JsonNode::Object(pairs) => {
            out.push('{');
            for (i, (key, value_handle)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('"');
                for ch in key.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        ch => out.push(ch),
                    }
                }
                out.push_str("\":");
                if let Some(val) = table.get(*value_handle) {
                    stringify_node_with_table(val, out, table);
                }
            }
            out.push('}');
        }
    }
}

// ============================================================================
// Exported FFI Functions
// ============================================================================

/// Parse a JSON string into a JsonNode tree, return root handle. -1 on error.
#[no_mangle]
pub extern "C" fn kryos_json_parse(str_handle: i64) -> i64 {
    match ks_to_string(str_handle) {
        Some(s) => {
            let mut parser = Parser::new(&s);
            parser.parse_value().unwrap_or(-1)
        }
        None => -1,
    }
}

/// Serialize a JsonNode tree to a Kryos string handle.
#[no_mangle]
pub extern "C" fn kryos_json_stringify(node_handle: i64) -> i64 {
    let mut result = String::new();
    with_node_table(|table| {
        if let Some(node) = table.get(node_handle) {
            stringify_node_with_table(node, &mut result, table);
        }
    });
    string_to_ks(&result)
}

/// Create a JSON object from two parallel Kryos arrays: keys and values.
#[no_mangle]
pub extern "C" fn kryos_json_object(keys_handle: i64, vals_handle: i64) -> i64 {
    unsafe {
        let keys = keys_handle as *const KryosArray;
        let vals = vals_handle as *const KryosArray;

        if keys.is_null() || vals.is_null() {
            return -1;
        }

        let key_len = (*keys).len as usize;
        let val_len = (*vals).len as usize;

        if key_len != val_len {
            return -1;
        }

        let key_data = (*keys).data as *const i64;
        let val_data = (*vals).data as *const i64;

        if key_data.is_null() || val_data.is_null() {
            return -1;
        }

        let mut pairs = Vec::new();
        for i in 0..key_len {
            let key_handle = *key_data.add(i);
            let val_handle = *val_data.add(i);

            if let Some(key_str) = ks_to_string(key_handle) {
                pairs.push((key_str, val_handle));
            } else {
                return -1;
            }
        }

        alloc_node(JsonNode::Object(pairs))
    }
}

/// Create a JSON array from a Kryos array of i64 JsonNode handles.
#[no_mangle]
pub extern "C" fn kryos_json_array(items_handle: i64) -> i64 {
    unsafe {
        let arr = items_handle as *const KryosArray;
        if arr.is_null() {
            return -1;
        }

        let len = (*arr).len as usize;
        let data = (*arr).data as *const i64;

        if data.is_null() {
            return -1;
        }

        let mut items = Vec::new();
        for i in 0..len {
            items.push(*data.add(i));
        }

        alloc_node(JsonNode::Array(items))
    }
}

/// Create a JSON string node from a KryosString handle.
#[no_mangle]
pub extern "C" fn kryos_json_string(str_handle: i64) -> i64 {
    match ks_to_string(str_handle) {
        Some(s) => alloc_node(JsonNode::Str(s)),
        None => -1,
    }
}

/// Create a JSON number node from f64.
#[no_mangle]
pub extern "C" fn kryos_json_number(val: f64) -> i64 {
    alloc_node(JsonNode::Number(val))
}

/// Create a JSON bool node from i64 (0=false, nonzero=true).
#[no_mangle]
pub extern "C" fn kryos_json_bool(val: i64) -> i64 {
    alloc_node(JsonNode::Bool(val != 0))
}

/// Create a JSON null node.
#[no_mangle]
pub extern "C" fn kryos_json_null() -> i64 {
    alloc_node(JsonNode::Null)
}

/// Get object field by string key. Returns node handle or 0 (null) if not found.
#[no_mangle]
pub extern "C" fn kryos_json_get(obj_handle: i64, key_handle: i64) -> i64 {
    match ks_to_string(key_handle) {
        Some(key) => with_node(obj_handle, |node| match node {
            JsonNode::Object(pairs) => {
                for (k, v) in pairs {
                    if k == &key {
                        return *v;
                    }
                }
                0
            }
            _ => 0,
        })
        .unwrap_or(0),
        None => 0,
    }
}

/// Get array element by index. Returns node handle or 0 if out of bounds.
#[no_mangle]
pub extern "C" fn kryos_json_get_index(arr_handle: i64, index: i64) -> i64 {
    if index < 0 {
        return 0;
    }
    with_node(arr_handle, |node| match node {
        JsonNode::Array(items) => {
            let idx = index as usize;
            if idx < items.len() {
                items[idx]
            } else {
                0
            }
        }
        _ => 0,
    })
    .unwrap_or(0)
}

/// Convert a JsonNode to a Kryos string handle (for strings, stringify for others).
#[no_mangle]
pub extern "C" fn kryos_json_to_str(node_handle: i64) -> i64 {
    let handle_out: Option<i64> = None;
    let mut bare_string: Option<String> = None;
    with_node_table(|table| {
        if let Some(node) = table.get(node_handle) {
            match node {
                JsonNode::Str(s) => bare_string = Some(s.clone()),
                _ => {
                    let mut result = String::new();
                    stringify_node_with_table(node, &mut result, table);
                    bare_string = Some(result);
                }
            }
        }
    });
    match bare_string {
        Some(s) => string_to_ks(&s),
        None => handle_out.unwrap_or(0),
    }
}

/// Convert a JsonNode number to i64 (truncates).
#[no_mangle]
pub extern "C" fn kryos_json_to_int(node_handle: i64) -> i64 {
    with_node(node_handle, |node| match node {
        JsonNode::Number(n) => *n as i64,
        _ => 0,
    })
    .unwrap_or(0)
}

/// Convert a JsonNode number to f64.
#[no_mangle]
pub extern "C" fn kryos_json_to_float(node_handle: i64) -> f64 {
    with_node(node_handle, |node| match node {
        JsonNode::Number(n) => *n,
        _ => 0.0,
    })
    .unwrap_or(0.0)
}

/// Returns 1 if node is null or handle is 0, 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_json_is_null(node_handle: i64) -> i64 {
    if node_handle == 0 {
        return 1;
    }
    with_node(node_handle, |node| match node {
        JsonNode::Null => 1,
        _ => 0,
    })
    .unwrap_or(1)
}

/// Returns array/object length, or 0 for other types.
#[no_mangle]
pub extern "C" fn kryos_json_length(node_handle: i64) -> i64 {
    with_node(node_handle, |node| match node {
        JsonNode::Array(items) => items.len() as i64,
        JsonNode::Object(pairs) => pairs.len() as i64,
        _ => 0,
    })
    .unwrap_or(0)
}

/// Returns a Kryos string handle with the type name.
#[no_mangle]
pub extern "C" fn kryos_json_type(node_handle: i64) -> i64 {
    let type_name = if node_handle == 0 {
        "null"
    } else {
        with_node(node_handle, |node| match node {
            JsonNode::Null => "null",
            JsonNode::Bool(_) => "bool",
            JsonNode::Number(_) => "number",
            JsonNode::Str(_) => "string",
            JsonNode::Array(_) => "array",
            JsonNode::Object(_) => "object",
        })
        .unwrap_or("null")
    };
    string_to_ks(type_name)
}
