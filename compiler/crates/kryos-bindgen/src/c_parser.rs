//! Minimal C header parser for Kryos bindgen.
//!
//! Parses a practical subset of C header files:
//! - Function declarations
//! - Struct definitions
//! - Enum definitions
//! - Typedefs
//! - Simple `#define` constants
//!
//! Ignores: comments, `#include`, `#ifdef`/`#ifndef`/`#endif`, macros,
//! inline function bodies, and other preprocessor directives.

/// A parsed C type.
#[derive(Debug, Clone, PartialEq)]
pub enum CType {
    Void,
    Char,
    UChar,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    LongLong,
    ULongLong,
    Float,
    Double,
    SizeT,
    Bool,
    Pointer(Box<CType>),
    Array(Box<CType>, Option<usize>),
    Named(String),
}

/// A parameter in a C function declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct CParam {
    pub name: Option<String>,
    pub ty: CType,
}

/// A field in a C struct.
#[derive(Debug, Clone, PartialEq)]
pub struct CField {
    pub name: String,
    pub ty: CType,
}

/// A top-level item parsed from a C header.
#[derive(Debug, Clone, PartialEq)]
pub enum CItem {
    Function {
        name: String,
        params: Vec<CParam>,
        ret_ty: CType,
        variadic: bool,
    },
    Struct {
        name: String,
        fields: Vec<CField>,
    },
    Enum {
        name: String,
        variants: Vec<(String, Option<i64>)>,
    },
    Typedef {
        name: String,
        target: CType,
    },
    Define {
        name: String,
        value: String,
    },
}

/// Parse C header source into a list of items, collecting errors.
pub fn parse_c_header(source: &str) -> Result<Vec<CItem>, Vec<String>> {
    let mut parser = CHeaderParser::new(source);
    parser.parse();
    if parser.errors.is_empty() {
        Ok(parser.items)
    } else {
        Err(parser.errors)
    }
}

// ---------------------------------------------------------------------------
// Internal parser
// ---------------------------------------------------------------------------

struct CHeaderParser<'a> {
    source: &'a str,
    pos: usize,
    items: Vec<CItem>,
    errors: Vec<String>,
}

impl<'a> CHeaderParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            items: Vec::new(),
            errors: Vec::new(),
        }
    }

    // -- helpers --

    fn remaining(&self) -> &'a str {
        &self.source[self.pos..]
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.source.len());
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_whitespace() {
                self.advance(ch.len_utf8());
            } else {
                break;
            }
        }
    }

    fn skip_line(&mut self) {
        if let Some(nl) = self.remaining().find('\n') {
            self.advance(nl + 1);
        } else {
            self.pos = self.source.len();
        }
    }

    fn skip_block_comment(&mut self) {
        // Assumes we are positioned right after "/*"
        if let Some(end) = self.remaining().find("*/") {
            self.advance(end + 2);
        } else {
            self.pos = self.source.len();
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            self.skip_whitespace();
            if self.remaining().starts_with("//") {
                self.skip_line();
            } else if self.remaining().starts_with("/*") {
                self.advance(2);
                self.skip_block_comment();
            } else {
                break;
            }
        }
    }

    fn expect_char(&mut self, ch: char) -> bool {
        self.skip_whitespace_and_comments();
        if self.peek_char() == Some(ch) {
            self.advance(ch.len_utf8());
            true
        } else {
            false
        }
    }

    fn read_ident(&mut self) -> Option<String> {
        self.skip_whitespace_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.advance(1);
            } else {
                break;
            }
        }
        if self.pos > start {
            Some(self.source[start..self.pos].to_string())
        } else {
            None
        }
    }

    // -- type parsing --

    fn parse_base_type(&mut self) -> Option<CType> {
        self.skip_whitespace_and_comments();

        // Consume leading `const` or `volatile` (ignore for type mapping)
        loop {
            let saved = self.pos;
            if let Some(ident) = self.read_ident() {
                if ident == "const" || ident == "volatile" {
                    continue;
                }
                // Put it back
                self.pos = saved;
            } else {
                self.pos = saved;
            }
            break;
        }

        // Read the type tokens
        let mut tokens: Vec<String> = Vec::new();

        // Consume type-like identifiers (may be multi-word like `unsigned long long`)
        loop {
            let saved = self.pos;
            if let Some(ident) = self.read_ident() {
                if ident == "const" || ident == "volatile" {
                    // skip mid-type qualifiers
                    continue;
                }
                if is_type_keyword(&ident) || (tokens.is_empty() && is_type_start(&ident)) {
                    tokens.push(ident);
                    continue;
                }
                // Not a type keyword; put it back -- it's likely a parameter name
                self.pos = saved;
                break;
            } else {
                break;
            }
        }

        if tokens.is_empty() {
            return None;
        }

        // Check for `struct <name>` or `enum <name>`
        if tokens.len() == 1 && (tokens[0] == "struct" || tokens[0] == "enum") {
            if let Some(name) = self.read_ident() {
                return Some(CType::Named(name));
            }
        }

        Some(resolve_type_tokens(&tokens))
    }

    fn parse_type(&mut self) -> Option<CType> {
        let base = self.parse_base_type()?;
        self.parse_pointer_suffix(base)
    }

    fn parse_pointer_suffix(&mut self, base: CType) -> Option<CType> {
        let mut ty = base;
        loop {
            self.skip_whitespace_and_comments();
            // Consume const between stars
            let saved = self.pos;
            if let Some(ident) = self.read_ident() {
                if ident != "const" && ident != "volatile" {
                    self.pos = saved;
                }
                // if const/volatile, just consumed it; continue checking for *
            }
            self.skip_whitespace_and_comments();
            if self.peek_char() == Some('*') {
                self.advance(1);
                ty = CType::Pointer(Box::new(ty));
                // Consume trailing const/volatile after *
                loop {
                    self.skip_whitespace_and_comments();
                    let saved2 = self.pos;
                    if let Some(ident) = self.read_ident() {
                        if ident == "const" || ident == "volatile" {
                            continue;
                        }
                        self.pos = saved2;
                    }
                    break;
                }
            } else {
                break;
            }
        }
        Some(ty)
    }

    // -- top-level parsing --

    fn parse(&mut self) {
        loop {
            self.skip_whitespace_and_comments();
            if self.is_eof() {
                break;
            }

            let loop_start = self.pos;

            // Preprocessor line
            if self.peek_char() == Some('#') {
                self.parse_preprocessor();
                // Guard: ensure progress
                if self.pos <= loop_start {
                    self.skip_line();
                }
                continue;
            }

            // Try to parse a declaration
            let saved = self.pos;
            if let Some(ident) = self.read_ident() {
                match ident.as_str() {
                    "typedef" => {
                        self.parse_typedef();
                    }
                    "struct" => {
                        self.parse_struct();
                    }
                    "enum" => {
                        self.parse_enum();
                    }
                    "extern" => {
                        // `extern "C" { ... }` -- skip the wrapper, parse contents
                        self.skip_whitespace_and_comments();
                        // skip string literal like "C"
                        if self.peek_char() == Some('"') {
                            self.advance(1);
                            while self.peek_char() != Some('"') && !self.is_eof() {
                                self.advance(1);
                            }
                            if self.peek_char() == Some('"') {
                                self.advance(1);
                            }
                        }
                        self.skip_whitespace_and_comments();
                        if self.peek_char() == Some('{') {
                            self.advance(1);
                            // Parse inner declarations until '}'
                            loop {
                                self.skip_whitespace_and_comments();
                                if self.peek_char() == Some('}') {
                                    self.advance(1);
                                    self.expect_char(';'); // optional trailing semicolon
                                    break;
                                }
                                if self.is_eof() {
                                    break;
                                }
                                let inner_loop_start = self.pos;
                                // Parse one declaration inside extern block
                                let inner_saved = self.pos;
                                if let Some(inner_ident) = self.read_ident() {
                                    match inner_ident.as_str() {
                                        "typedef" => self.parse_typedef(),
                                        "struct" => self.parse_struct(),
                                        "enum" => self.parse_enum(),
                                        _ => {
                                            self.pos = inner_saved;
                                            self.parse_function_or_skip();
                                        }
                                    }
                                } else {
                                    self.skip_line();
                                }
                                // Guard: ensure progress
                                if self.pos <= inner_loop_start {
                                    self.skip_line();
                                }
                            }
                        }
                    }
                    _ => {
                        // Could be a function declaration (return type starts here)
                        self.pos = saved;
                        self.parse_function_or_skip();
                    }
                }
            } else {
                // Unknown character, skip it
                self.advance(1);
            }

            // Guard: ensure progress to prevent infinite loops
            if self.pos <= loop_start {
                self.skip_line();
            }
        }
    }

    fn parse_preprocessor(&mut self) {
        // We're at '#'
        self.advance(1);
        self.skip_whitespace();
        if let Some(directive) = self.read_ident() {
            match directive.as_str() {
                "define" => {
                    self.parse_define();
                }
                _ => {
                    // include, ifdef, ifndef, endif, pragma, etc. -- skip line
                    self.skip_line();
                }
            }
        } else {
            self.skip_line();
        }
    }

    fn parse_define(&mut self) {
        self.skip_whitespace();
        if let Some(name) = self.read_ident() {
            // Check for function-like macro (has `(` immediately after name)
            if self.peek_char() == Some('(') {
                // Function-like macro -- skip
                self.skip_line();
                return;
            }
            self.skip_whitespace();
            // Read value until end of line (handling line continuations)
            let start = self.pos;
            loop {
                let rem = self.remaining();
                if rem.is_empty() {
                    break;
                }
                if let Some(nl) = rem.find('\n') {
                    // Check if line continues (backslash before newline)
                    if nl > 0 && rem.as_bytes()[nl - 1] == b'\\' {
                        self.advance(nl + 1);
                        continue;
                    }
                    self.advance(nl + 1);
                    break;
                } else {
                    self.pos = self.source.len();
                    break;
                }
            }
            let raw_value = self.source[start..self.pos].trim().to_string();
            if !raw_value.is_empty() {
                // Only keep simple numeric or string constants
                let trimmed = raw_value.trim();
                if is_simple_constant(trimmed) {
                    self.items.push(CItem::Define {
                        name,
                        value: strip_numeric_suffix(trimmed),
                    });
                }
            }
        } else {
            self.skip_line();
        }
    }

    fn parse_typedef(&mut self) {
        self.skip_whitespace_and_comments();

        // Check for `typedef struct { ... } name;`
        let saved = self.pos;
        if let Some(kw) = self.read_ident() {
            if kw == "struct" {
                self.skip_whitespace_and_comments();
                // Could be `typedef struct name { ... } alias;` or `typedef struct { ... } alias;`
                let maybe_name = self.read_ident();
                self.skip_whitespace_and_comments();
                if self.peek_char() == Some('{') {
                    // Parse struct body
                    self.advance(1);
                    let fields = self.parse_struct_fields();
                    self.skip_whitespace_and_comments();
                    // Read the typedef name
                    let mut ptr_count = 0;
                    while self.peek_char() == Some('*') {
                        self.advance(1);
                        self.skip_whitespace_and_comments();
                        ptr_count += 1;
                    }
                    if let Some(alias) = self.read_ident() {
                        self.expect_char(';');
                        // Emit the struct with the alias name (or original name)
                        let struct_name = maybe_name.unwrap_or_else(|| alias.clone());
                        self.items.push(CItem::Struct {
                            name: struct_name.clone(),
                            fields,
                        });
                        if ptr_count > 0 {
                            let mut ty = CType::Named(struct_name);
                            for _ in 0..ptr_count {
                                ty = CType::Pointer(Box::new(ty));
                            }
                            self.items.push(CItem::Typedef {
                                name: alias,
                                target: ty,
                            });
                        } else if alias != struct_name {
                            self.items.push(CItem::Typedef {
                                name: alias,
                                target: CType::Named(struct_name),
                            });
                        }
                    } else {
                        self.expect_char(';');
                    }
                    return;
                }
                // Not a struct definition -- fall through to regular typedef
                self.pos = saved;
            } else if kw == "enum" {
                self.skip_whitespace_and_comments();
                let maybe_name = self.read_ident();
                self.skip_whitespace_and_comments();
                if self.peek_char() == Some('{') {
                    self.advance(1);
                    let variants = self.parse_enum_variants();
                    self.skip_whitespace_and_comments();
                    if let Some(alias) = self.read_ident() {
                        self.expect_char(';');
                        let enum_name = maybe_name.unwrap_or_else(|| alias.clone());
                        self.items.push(CItem::Enum {
                            name: enum_name.clone(),
                            variants,
                        });
                        if alias != enum_name {
                            self.items.push(CItem::Typedef {
                                name: alias,
                                target: CType::Named(enum_name),
                            });
                        }
                    } else {
                        self.expect_char(';');
                    }
                    return;
                }
                self.pos = saved;
            } else {
                self.pos = saved;
            }
        } else {
            self.pos = saved;
        }

        // Regular typedef: `typedef <type> <name>;`
        if let Some(target) = self.parse_type() {
            self.skip_whitespace_and_comments();
            // Handle pointer indirection on the alias side
            // e.g., `typedef int *intptr;`
            // The pointer was already consumed by parse_type
            if let Some(name) = self.read_ident() {
                // Check for array suffix: `typedef int arr[10];`
                self.skip_whitespace_and_comments();
                let final_ty = if self.peek_char() == Some('[') {
                    self.advance(1);
                    let size = self.read_array_size();
                    self.expect_char(']');
                    CType::Array(Box::new(target), size)
                } else {
                    target
                };
                self.expect_char(';');
                self.items.push(CItem::Typedef {
                    name,
                    target: final_ty,
                });
            } else {
                // Skip to semicolon
                self.skip_to_char(';');
                self.advance(1);
            }
        } else {
            self.skip_to_char(';');
            self.advance(1);
        }
    }

    fn parse_struct(&mut self) {
        self.skip_whitespace_and_comments();
        let name = match self.read_ident() {
            Some(n) => n,
            None => {
                self.errors.push("expected struct name".to_string());
                self.skip_to_char('}');
                self.advance(1);
                self.expect_char(';');
                return;
            }
        };
        self.skip_whitespace_and_comments();
        if self.peek_char() != Some('{') {
            // Forward declaration: `struct foo;`
            self.skip_to_char(';');
            self.advance(1);
            return;
        }
        self.advance(1); // skip '{'

        let fields = self.parse_struct_fields();
        self.skip_whitespace_and_comments();
        // Optional variable name after closing brace (skip it)
        let _ = self.read_ident();
        self.expect_char(';');

        self.items.push(CItem::Struct { name, fields });
    }

    fn parse_struct_fields(&mut self) -> Vec<CField> {
        let mut fields = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.peek_char() == Some('}') {
                self.advance(1);
                break;
            }
            if self.is_eof() {
                self.errors
                    .push("unexpected EOF in struct body".to_string());
                break;
            }

            if let Some(ty) = self.parse_type() {
                self.skip_whitespace_and_comments();
                if let Some(name) = self.read_ident() {
                    // Check for array suffix
                    self.skip_whitespace_and_comments();
                    let final_ty = if self.peek_char() == Some('[') {
                        self.advance(1);
                        let size = self.read_array_size();
                        self.expect_char(']');
                        CType::Array(Box::new(ty), size)
                    } else {
                        ty
                    };
                    fields.push(CField { name, ty: final_ty });
                }
                self.expect_char(';');
            } else {
                // Skip unknown content
                self.skip_to_char(';');
                self.advance(1);
            }
        }
        fields
    }

    fn parse_enum(&mut self) {
        self.skip_whitespace_and_comments();
        let name = match self.read_ident() {
            Some(n) => n,
            None => {
                self.errors.push("expected enum name".to_string());
                self.skip_to_char('}');
                self.advance(1);
                self.expect_char(';');
                return;
            }
        };
        self.skip_whitespace_and_comments();
        if self.peek_char() != Some('{') {
            // Forward declaration or usage
            self.skip_to_char(';');
            self.advance(1);
            return;
        }
        self.advance(1); // skip '{'

        let variants = self.parse_enum_variants();
        self.skip_whitespace_and_comments();
        // Optional variable name after closing brace
        let _ = self.read_ident();
        self.expect_char(';');

        self.items.push(CItem::Enum { name, variants });
    }

    fn parse_enum_variants(&mut self) -> Vec<(String, Option<i64>)> {
        let mut variants = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.peek_char() == Some('}') {
                self.advance(1);
                break;
            }
            if self.is_eof() {
                self.errors.push("unexpected EOF in enum body".to_string());
                break;
            }

            if let Some(name) = self.read_ident() {
                self.skip_whitespace_and_comments();
                let value = if self.peek_char() == Some('=') {
                    self.advance(1);
                    self.skip_whitespace_and_comments();
                    self.parse_integer_value()
                } else {
                    None
                };
                variants.push((name, value));
                self.skip_whitespace_and_comments();
                if self.peek_char() == Some(',') {
                    self.advance(1);
                }
            } else {
                self.advance(1);
            }
        }
        variants
    }

    fn parse_integer_value(&mut self) -> Option<i64> {
        self.skip_whitespace_and_comments();
        let start = self.pos;
        let negative = if self.peek_char() == Some('-') {
            self.advance(1);
            true
        } else {
            false
        };
        self.skip_whitespace();

        if self.remaining().starts_with("0x") || self.remaining().starts_with("0X") {
            self.advance(2);
            let hex_start = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_hexdigit() {
                    self.advance(1);
                } else {
                    break;
                }
            }
            // Skip suffixes
            self.skip_integer_suffix();
            let hex_str = &self.source[hex_start..self.pos]
                .trim_end_matches(|c: char| c == 'u' || c == 'U' || c == 'l' || c == 'L');
            if let Ok(v) = i64::from_str_radix(hex_str, 16) {
                return Some(if negative { -v } else { v });
            }
        } else {
            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_digit() {
                    self.advance(1);
                } else {
                    break;
                }
            }
            self.skip_integer_suffix();
        }

        let num_str = &self.source[start..self.pos];
        // Strip suffixes for parsing
        let cleaned: String = num_str
            .chars()
            .filter(|c| {
                c.is_ascii_digit() || *c == '-' || *c == 'x' || *c == 'X' || c.is_ascii_hexdigit()
            })
            .collect();
        cleaned.parse::<i64>().ok()
    }

    fn skip_integer_suffix(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch == 'u' || ch == 'U' || ch == 'l' || ch == 'L' {
                self.advance(1);
            } else {
                break;
            }
        }
    }

    fn parse_function_or_skip(&mut self) {
        // Try parsing as: <return_type> <name>(<params>);
        let saved = self.pos;
        if let Some(ret_ty) = self.parse_type() {
            self.skip_whitespace_and_comments();
            if let Some(name) = self.read_ident() {
                self.skip_whitespace_and_comments();
                if self.peek_char() == Some('(') {
                    self.advance(1);
                    let (params, variadic) = self.parse_function_params();
                    self.skip_whitespace_and_comments();
                    // Skip optional attributes like __attribute__((..))
                    self.skip_attributes();
                    self.expect_char(';');
                    self.items.push(CItem::Function {
                        name,
                        params,
                        ret_ty,
                        variadic,
                    });
                    return;
                }
            }
        }
        // Could not parse as function -- skip to next semicolon or line
        self.pos = saved;
        self.skip_to_char(';');
        if !self.is_eof() {
            self.advance(1);
        }
    }

    fn parse_function_params(&mut self) -> (Vec<CParam>, bool) {
        let mut params = Vec::new();
        let mut variadic = false;

        loop {
            self.skip_whitespace_and_comments();
            if self.peek_char() == Some(')') {
                self.advance(1);
                break;
            }
            if self.is_eof() {
                self.errors
                    .push("unexpected EOF in function parameters".to_string());
                break;
            }

            // Check for variadic `...`
            if self.remaining().starts_with("...") {
                self.advance(3);
                variadic = true;
                self.skip_whitespace_and_comments();
                if self.peek_char() == Some(')') {
                    self.advance(1);
                }
                break;
            }

            // Check for `void)` meaning no parameters
            let saved = self.pos;
            if let Some(ident) = self.read_ident() {
                if ident == "void" {
                    self.skip_whitespace_and_comments();
                    if self.peek_char() == Some(')') {
                        self.advance(1);
                        break;
                    }
                }
                self.pos = saved;
            }

            // Parse parameter type
            if let Some(ty) = self.parse_type() {
                self.skip_whitespace_and_comments();
                // Try to read parameter name (optional)
                let name = if self.peek_char() != Some(',')
                    && self.peek_char() != Some(')')
                    && self.peek_char() != Some('.')
                {
                    // Check for array syntax after name: `int arr[]`
                    let param_name = self.read_ident();
                    self.skip_whitespace_and_comments();
                    if self.peek_char() == Some('[') {
                        self.advance(1);
                        let _size = self.read_array_size();
                        self.expect_char(']');
                    }
                    param_name
                } else {
                    None
                };
                params.push(CParam { name, ty });
            }

            self.skip_whitespace_and_comments();
            if self.peek_char() == Some(',') {
                self.advance(1);
            }
        }

        (params, variadic)
    }

    fn skip_attributes(&mut self) {
        self.skip_whitespace_and_comments();
        // __attribute__((...))
        if self.remaining().starts_with("__attribute__") {
            self.advance(13);
            self.skip_whitespace_and_comments();
            if self.peek_char() == Some('(') {
                let mut depth = 0;
                loop {
                    match self.peek_char() {
                        Some('(') => {
                            depth += 1;
                            self.advance(1);
                        }
                        Some(')') => {
                            depth -= 1;
                            self.advance(1);
                            if depth == 0 {
                                break;
                            }
                        }
                        None => break,
                        _ => self.advance(1),
                    }
                }
            }
        }
    }

    fn read_array_size(&mut self) -> Option<usize> {
        self.skip_whitespace_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.advance(1);
            } else {
                break;
            }
        }
        if self.pos > start {
            self.source[start..self.pos].parse::<usize>().ok()
        } else {
            None
        }
    }

    fn skip_to_char(&mut self, ch: char) {
        while let Some(c) = self.peek_char() {
            if c == ch {
                return;
            }
            self.advance(c.len_utf8());
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_type_keyword(s: &str) -> bool {
    matches!(
        s,
        "void" | "char" | "short" | "int" | "long" | "float" | "double" | "signed" | "unsigned"
    )
}

fn is_type_start(s: &str) -> bool {
    is_type_keyword(s)
        || matches!(
            s,
            "size_t"
                | "ssize_t"
                | "bool"
                | "_Bool"
                | "int8_t"
                | "int16_t"
                | "int32_t"
                | "int64_t"
                | "uint8_t"
                | "uint16_t"
                | "uint32_t"
                | "uint64_t"
                | "uintptr_t"
                | "intptr_t"
                | "ptrdiff_t"
                | "struct"
                | "enum"
        )
        || s.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
}

fn resolve_type_tokens(tokens: &[String]) -> CType {
    let joined: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();

    match joined.as_slice() {
        ["void"] => CType::Void,
        ["char"] | ["signed", "char"] => CType::Char,
        ["unsigned", "char"] => CType::UChar,
        ["short"] | ["signed", "short"] | ["short", "int"] | ["signed", "short", "int"] => {
            CType::Short
        }
        ["unsigned", "short"] | ["unsigned", "short", "int"] => CType::UShort,
        ["int"] | ["signed"] | ["signed", "int"] => CType::Int,
        ["unsigned"] | ["unsigned", "int"] => CType::UInt,
        ["long"] | ["signed", "long"] | ["long", "int"] | ["signed", "long", "int"] => CType::Long,
        ["unsigned", "long"] | ["unsigned", "long", "int"] => CType::ULong,
        ["long", "long"]
        | ["signed", "long", "long"]
        | ["long", "long", "int"]
        | ["signed", "long", "long", "int"] => CType::LongLong,
        ["unsigned", "long", "long"] | ["unsigned", "long", "long", "int"] => CType::ULongLong,
        ["float"] => CType::Float,
        ["double"] => CType::Double,
        ["long", "double"] => CType::Double, // map long double to double
        ["size_t"] => CType::SizeT,
        ["ssize_t"] => CType::Long, // approximate
        ["bool"] | ["_Bool"] => CType::Bool,
        ["int8_t"] => CType::Char,
        ["int16_t"] => CType::Short,
        ["int32_t"] => CType::Int,
        ["int64_t"] => CType::LongLong,
        ["uint8_t"] => CType::UChar,
        ["uint16_t"] => CType::UShort,
        ["uint32_t"] => CType::UInt,
        ["uint64_t"] => CType::ULongLong,
        ["uintptr_t"] | ["intptr_t"] | ["ptrdiff_t"] => CType::SizeT,
        // Single identifier -- named type (typedef, struct, etc.)
        [name] => CType::Named(name.to_string()),
        _ => {
            // Fallback: treat as named type with joined name
            CType::Named(tokens.join(" "))
        }
    }
}

/// Returns true if the string looks like a simple numeric or string constant.
fn is_simple_constant(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // String literal
    if s.starts_with('"') && s.ends_with('"') {
        return true;
    }
    // Character literal
    if s.starts_with('\'') && s.ends_with('\'') {
        return true;
    }
    // Numeric: optional minus, then digits or 0x hex, optional suffix
    let s = s.strip_prefix('-').unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    let s = if s.starts_with("0x") || s.starts_with("0X") {
        &s[2..]
    } else {
        s
    };
    // Must start with digit or hex digit
    let first = s.chars().next().unwrap_or(' ');
    if !first.is_ascii_digit() && !first.is_ascii_hexdigit() {
        return false;
    }
    // Remaining: digits, hex, dots, suffixes
    s.chars().all(|c| {
        c.is_ascii_hexdigit()
            || c == '.'
            || c == 'u'
            || c == 'U'
            || c == 'l'
            || c == 'L'
            || c == 'f'
            || c == 'F'
    })
}

/// Strip C numeric suffixes (u, U, l, L, f, F, ul, UL, ull, ULL, etc.)
fn strip_numeric_suffix(s: &str) -> String {
    // Don't strip from string or char literals
    if s.starts_with('"') || s.starts_with('\'') {
        return s.to_string();
    }
    let s_trimmed = s.trim_end_matches(|c: char| {
        c == 'u' || c == 'U' || c == 'l' || c == 'L' || c == 'f' || c == 'F'
    });
    if s_trimmed.is_empty() {
        s.to_string()
    } else {
        s_trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_function() {
        let items = parse_c_header("int puts(const char* s);").unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Function {
                name,
                params,
                ret_ty,
                variadic,
            } => {
                assert_eq!(name, "puts");
                assert_eq!(*ret_ty, CType::Int);
                assert!(!variadic);
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].ty, CType::Pointer(Box::new(CType::Char)));
                assert_eq!(params[0].name, Some("s".to_string()));
            }
            other => panic!("expected Function, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_struct() {
        let items = parse_c_header("struct timeval { long tv_sec; long tv_usec; };").unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Struct { name, fields } => {
                assert_eq!(name, "timeval");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "tv_sec");
                assert_eq!(fields[0].ty, CType::Long);
                assert_eq!(fields[1].name, "tv_usec");
                assert_eq!(fields[1].ty, CType::Long);
            }
            other => panic!("expected Struct, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_enum() {
        let items = parse_c_header("enum color { RED = 0, GREEN = 1, BLUE = 2 };").unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Enum { name, variants } => {
                assert_eq!(name, "color");
                assert_eq!(variants.len(), 3);
                assert_eq!(variants[0], ("RED".to_string(), Some(0)));
                assert_eq!(variants[1], ("GREEN".to_string(), Some(1)));
                assert_eq!(variants[2], ("BLUE".to_string(), Some(2)));
            }
            other => panic!("expected Enum, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_typedef() {
        let items = parse_c_header("typedef unsigned int uint32_t;").unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Typedef { name, target } => {
                assert_eq!(name, "uint32_t");
                assert_eq!(*target, CType::UInt);
            }
            other => panic!("expected Typedef, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_define() {
        let items = parse_c_header("#define EXIT_SUCCESS 0").unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Define { name, value } => {
                assert_eq!(name, "EXIT_SUCCESS");
                assert_eq!(value, "0");
            }
            other => panic!("expected Define, got {:?}", other),
        }
    }

    #[test]
    fn test_variadic_function() {
        let items = parse_c_header("int printf(const char* fmt, ...);").unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Function { variadic, .. } => {
                assert!(*variadic);
            }
            other => panic!("expected Function, got {:?}", other),
        }
    }

    #[test]
    fn test_void_pointer() {
        let items = parse_c_header("void* malloc(size_t size);").unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Function {
                name,
                ret_ty,
                params,
                ..
            } => {
                assert_eq!(name, "malloc");
                assert_eq!(*ret_ty, CType::Pointer(Box::new(CType::Void)));
                assert_eq!(params[0].ty, CType::SizeT);
            }
            other => panic!("expected Function, got {:?}", other),
        }
    }

    #[test]
    fn test_ignores_comments() {
        let src = r#"
// This is a comment
/* Multi-line
   comment */
int foo(void);
"#;
        let items = parse_c_header(src).unwrap();
        assert_eq!(items.len(), 1);
        match &items[0] {
            CItem::Function { name, params, .. } => {
                assert_eq!(name, "foo");
                assert!(params.is_empty());
            }
            other => panic!("expected Function, got {:?}", other),
        }
    }

    #[test]
    fn test_ignores_include() {
        let src = r#"
#include <stdio.h>
#include "myheader.h"
int bar(void);
"#;
        let items = parse_c_header(src).unwrap();
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], CItem::Function { name, .. } if name == "bar"));
    }
}
