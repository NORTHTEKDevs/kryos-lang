use crate::token::*;
use kryos_errors::{Diagnostic, Span};

/// Kryos lexer — tokenizes UTF-8 source into a token stream.
pub struct Lexer<'src> {
    src: &'src str,
    bytes: &'src [u8],
    file_id: u32,
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'src> Lexer<'src> {
    pub fn new(src: &'src str, file_id: u32) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            file_id,
            pos: 0,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn tokenize(mut self) -> Vec<Token> {
        while !self.at_end() {
            self.skip_whitespace_and_comments();
            if self.at_end() {
                break;
            }
            self.scan_token();
        }
        self.emit(TokenKind::Eof, self.pos, self.pos, String::new());
        self.tokens
    }

    /// Like `tokenize`, but also returns any lexer-level diagnostics
    /// (currently: unterminated string literals).
    pub fn tokenize_with_diagnostics(mut self) -> (Vec<Token>, Vec<Diagnostic>) {
        while !self.at_end() {
            self.skip_whitespace_and_comments();
            if self.at_end() {
                break;
            }
            self.scan_token();
        }
        self.emit(TokenKind::Eof, self.pos, self.pos, String::new());
        (self.tokens, self.diagnostics)
    }

    #[allow(dead_code)]
    fn emit_diag(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn peek(&self) -> u8 {
        if self.at_end() {
            0
        } else {
            self.bytes[self.pos]
        }
    }

    fn peek_at(&self, offset: usize) -> u8 {
        let idx = self.pos + offset;
        if idx >= self.bytes.len() {
            0
        } else {
            self.bytes[idx]
        }
    }

    fn advance(&mut self) -> u8 {
        let ch = self.bytes[self.pos];
        self.pos += 1;
        ch
    }

    /// Consume one full UTF-8 scalar value at `self.pos`, advancing past all
    /// of its bytes, and return it as a `char`. On a malformed sequence one
    /// byte is consumed and U+FFFD is returned so we never silently re-encode
    /// raw UTF-8 bytes as Latin-1 codepoints (which would double-encode).
    fn advance_utf8(&mut self) -> char {
        let b0 = self.bytes[self.pos];
        if b0 < 0x80 {
            self.pos += 1;
            return b0 as char;
        }
        let width = match b0 {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => {
                self.pos += 1;
                return '\u{FFFD}';
            }
        };
        if self.pos + width > self.bytes.len() {
            self.pos += 1;
            return '\u{FFFD}';
        }
        let slice = &self.bytes[self.pos..self.pos + width];
        match std::str::from_utf8(slice) {
            Ok(s) => {
                let ch = s.chars().next().unwrap();
                self.pos += width;
                ch
            }
            Err(_) => {
                self.pos += 1;
                '\u{FFFD}'
            }
        }
    }

    fn match_char(&mut self, expected: u8) -> bool {
        if self.at_end() || self.bytes[self.pos] != expected {
            return false;
        }
        self.pos += 1;
        true
    }

    fn emit(&mut self, kind: TokenKind, start: usize, end: usize, text: String) {
        self.tokens.push(Token {
            kind,
            span: Span::new(self.file_id, start as u32, end as u32),
            text,
        });
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while !self.at_end() && matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n') {
                self.advance();
            }
            if self.at_end() {
                return;
            }
            if self.peek() == b'/' && self.peek_at(1) == b'/' {
                if self.peek_at(2) == b'/' && self.peek_at(3) != b'/' {
                    // Doc comment: /// ...
                    let start = self.pos;
                    self.advance();
                    self.advance();
                    self.advance(); // skip ///
                                    // Skip leading space after ///
                    if !self.at_end() && self.peek() == b' ' {
                        self.advance();
                    }
                    let content_start = self.pos;
                    while !self.at_end() && self.peek() != b'\n' {
                        self.advance();
                    }
                    let content = self.src[content_start..self.pos].trim_end().to_string();
                    self.emit(TokenKind::DocComment, start, self.pos, content);
                    continue;
                }
                // Regular comment: skip
                while !self.at_end() && self.peek() != b'\n' {
                    self.advance();
                }
                continue;
            }
            if self.peek() == b'/' && self.peek_at(1) == b'*' {
                let block_start = self.pos;
                self.advance();
                self.advance();
                let mut depth = 1u32;
                while !self.at_end() && depth > 0 {
                    if self.peek() == b'/' && self.peek_at(1) == b'*' {
                        self.advance();
                        self.advance();
                        depth += 1;
                    } else if self.peek() == b'*' && self.peek_at(1) == b'/' {
                        self.advance();
                        self.advance();
                        depth -= 1;
                    } else {
                        self.advance();
                    }
                }
                if depth > 0 {
                    let span = Span::new(self.file_id, block_start as u32, (block_start + 2) as u32);
                    let diag = Diagnostic::error("unterminated block comment")
                        .with_label(span, "this block comment is never closed")
                        .with_note("block comments must end with `*/`");
                    self.emit_diag(diag);
                }
                continue;
            }
            break;
        }
    }

    fn scan_token(&mut self) {
        let start = self.pos;
        let ch = self.advance();

        match ch {
            b'(' => self.emit(TokenKind::LParen, start, self.pos, "(".into()),
            b')' => self.emit(TokenKind::RParen, start, self.pos, ")".into()),
            b'{' => self.emit(TokenKind::LBrace, start, self.pos, "{".into()),
            b'}' => self.emit(TokenKind::RBrace, start, self.pos, "}".into()),
            b'[' => self.emit(TokenKind::LBracket, start, self.pos, "[".into()),
            b']' => self.emit(TokenKind::RBracket, start, self.pos, "]".into()),
            b';' => self.emit(TokenKind::Semicolon, start, self.pos, ";".into()),
            b',' => self.emit(TokenKind::Comma, start, self.pos, ",".into()),
            b'~' => self.emit(TokenKind::Tilde, start, self.pos, "~".into()),
            b'@' => self.emit(TokenKind::At, start, self.pos, "@".into()),
            b'#' => self.emit(TokenKind::Hash, start, self.pos, "#".into()),
            b'?' => self.emit(TokenKind::Question, start, self.pos, "?".into()),
            b'^' => self.emit(TokenKind::Caret, start, self.pos, "^".into()),
            b'&' => {
                if self.match_char(b'&') {
                    self.emit(TokenKind::AmpAmp, start, self.pos, "&&".into());
                } else {
                    self.emit(TokenKind::Amp, start, self.pos, "&".into());
                }
            }

            b'+' => {
                if self.match_char(b'=') {
                    self.emit(TokenKind::PlusEq, start, self.pos, "+=".into());
                } else {
                    self.emit(TokenKind::Plus, start, self.pos, "+".into());
                }
            }
            b'-' => {
                if self.match_char(b'>') {
                    self.emit(TokenKind::Arrow, start, self.pos, "->".into());
                } else if self.match_char(b'=') {
                    self.emit(TokenKind::MinusEq, start, self.pos, "-=".into());
                } else {
                    self.emit(TokenKind::Minus, start, self.pos, "-".into());
                }
            }
            b'*' => {
                if self.match_char(b'*') {
                    self.emit(TokenKind::Power, start, self.pos, "**".into());
                } else if self.match_char(b'=') {
                    self.emit(TokenKind::StarEq, start, self.pos, "*=".into());
                } else {
                    self.emit(TokenKind::Star, start, self.pos, "*".into());
                }
            }
            b'/' => {
                if self.match_char(b'=') {
                    self.emit(TokenKind::SlashEq, start, self.pos, "/=".into());
                } else {
                    self.emit(TokenKind::Slash, start, self.pos, "/".into());
                }
            }
            b'%' => self.emit(TokenKind::Percent, start, self.pos, "%".into()),

            b'=' => {
                if self.match_char(b'=') {
                    self.emit(TokenKind::EqEq, start, self.pos, "==".into());
                } else if self.match_char(b'>') {
                    self.emit(TokenKind::FatArrow, start, self.pos, "=>".into());
                } else {
                    self.emit(TokenKind::Eq, start, self.pos, "=".into());
                }
            }
            b'!' => {
                if self.match_char(b'=') {
                    self.emit(TokenKind::BangEq, start, self.pos, "!=".into());
                } else {
                    self.emit(TokenKind::Bang, start, self.pos, "!".into());
                }
            }
            b'<' => {
                if self.match_char(b'=') {
                    self.emit(TokenKind::LtEq, start, self.pos, "<=".into());
                } else if self.match_char(b'<') {
                    self.emit(TokenKind::Shl, start, self.pos, "<<".into());
                } else {
                    self.emit(TokenKind::Lt, start, self.pos, "<".into());
                }
            }
            b'>' => {
                if self.match_char(b'=') {
                    self.emit(TokenKind::GtEq, start, self.pos, ">=".into());
                } else if self.match_char(b'>') {
                    self.emit(TokenKind::Shr, start, self.pos, ">>".into());
                } else {
                    self.emit(TokenKind::Gt, start, self.pos, ">".into());
                }
            }
            b'|' => {
                if self.match_char(b'|') {
                    self.emit(TokenKind::PipePipe, start, self.pos, "||".into());
                } else {
                    self.emit(TokenKind::Pipe, start, self.pos, "|".into());
                }
            }

            b':' => {
                if self.match_char(b':') {
                    self.emit(TokenKind::ColonColon, start, self.pos, "::".into());
                } else {
                    self.emit(TokenKind::Colon, start, self.pos, ":".into());
                }
            }
            b'.' => {
                if self.match_char(b'.') {
                    if self.match_char(b'=') {
                        self.emit(TokenKind::DotDotEq, start, self.pos, "..=".into());
                    } else {
                        self.emit(TokenKind::DotDot, start, self.pos, "..".into());
                    }
                } else {
                    self.emit(TokenKind::Dot, start, self.pos, ".".into());
                }
            }

            b'"' => self.scan_string(start),
            b'\'' => self.scan_char(start),
            b'0'..=b'9' => self.scan_number(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.scan_identifier(start),

            _ => {
                let text = String::from(ch as char);
                self.emit(TokenKind::Error, start, self.pos, text);
            }
        }
    }

    fn scan_string(&mut self, start: usize) {
        let mut text = String::new();
        let mut has_interpolation = false;

        while !self.at_end() && self.peek() != b'"' {
            if self.peek() == b'{' {
                // `{{` -> literal `{`.  Lets templates that need braces
                // (CSS / JSON / shell scripts) coexist with interpolation
                // without resorting to `\{` everywhere.  Mirrors Rust /
                // Python f-string conventions.
                if self.peek_at(1) == b'{' {
                    self.advance(); // consume first `{`
                    self.advance(); // consume second `{`
                    text.push('{');
                    continue;
                }
                if !text.is_empty() || !has_interpolation {
                    self.emit(TokenKind::StringPart, start, self.pos, text.clone());
                    text.clear();
                }
                has_interpolation = true;
                let brace_start = self.pos;
                self.advance();
                self.emit(TokenKind::InterpStart, brace_start, self.pos, "{".into());

                while !self.at_end() && self.peek() != b'}' {
                    self.skip_whitespace_and_comments();
                    if !self.at_end() && self.peek() != b'}' {
                        self.scan_token();
                    }
                }
                if !self.at_end() {
                    let end_start = self.pos;
                    self.advance();
                    self.emit(TokenKind::InterpEnd, end_start, self.pos, "}".into());
                }
                continue;
            }

            // `}}` outside an interpolation -> literal `}`.  Symmetric with
            // `{{` so users can write `"if (x) {{ foo() }}"` without escapes.
            // A bare `}` is still allowed (passes through) for back-compat.
            if self.peek() == b'}' && self.peek_at(1) == b'}' {
                self.advance();
                self.advance();
                text.push('}');
                continue;
            }

            if self.peek() == b'\\' {
                self.advance();
                if !self.at_end() {
                    let esc = self.advance();
                    match esc {
                        b'n' => text.push('\n'),
                        b't' => text.push('\t'),
                        b'r' => text.push('\r'),
                        b'\\' => text.push('\\'),
                        b'"' => text.push('"'),
                        b'0' => text.push('\0'),
                        // \{ and \} escape the interpolation braces so that
                        // string literals can contain literal `{` and `}`
                        // characters (useful for embedded JSON, format
                        // templates, etc.).
                        b'{' => text.push('{'),
                        b'}' => text.push('}'),
                        _ => {
                            text.push('\\');
                            text.push(esc as char);
                        }
                    }
                }
                continue;
            }

            text.push(self.advance_utf8());
        }

        if !self.at_end() {
            self.advance(); // closing "
        } else {
            // Unterminated string literal — emit a precise diagnostic that
            // points at the opening quote, so users don't see misleading
            // "unexpected end of file" from the parser further down.
            let span = Span::new(self.file_id, start as u32, (start + 1) as u32);
            let diag = Diagnostic::error("unterminated string literal")
                .with_label(span, "this string is never closed")
                .with_note("add a matching `\"` before end of file");
            self.emit_diag(diag);
        }

        if has_interpolation {
            self.emit(TokenKind::StringPart, start, self.pos, text);
        } else {
            self.emit(TokenKind::String, start, self.pos, text);
        }
    }

    fn scan_char(&mut self, start: usize) {
        let ch = if self.peek() == b'\\' {
            self.advance();
            match self.advance() {
                b'n' => '\n',
                b't' => '\t',
                b'r' => '\r',
                b'\\' => '\\',
                b'\'' => '\'',
                b'0' => '\0',
                c => c as char,
            }
        } else {
            self.advance_utf8()
        };

        if !self.at_end() && self.peek() == b'\'' {
            self.advance();
        } else if self.at_end() {
            let span = Span::new(self.file_id, start as u32, (start + 1) as u32);
            let diag = Diagnostic::error("unterminated character literal")
                .with_label(span, "this character literal is never closed")
                .with_note("character literals are a single quoted code point: `'a'`");
            self.emit_diag(diag);
        }

        self.emit(TokenKind::Char, start, self.pos, ch.to_string());
    }

    fn scan_number(&mut self, start: usize) {
        if self.src.as_bytes()[start] == b'0' && !self.at_end() {
            match self.peek() {
                b'x' | b'X' => {
                    self.advance();
                    while !self.at_end() && (self.peek().is_ascii_hexdigit() || self.peek() == b'_')
                    {
                        self.advance();
                    }
                    let text = self.src[start..self.pos].to_string();
                    self.emit(TokenKind::Integer, start, self.pos, text);
                    return;
                }
                b'b' | b'B' => {
                    self.advance();
                    while !self.at_end() && matches!(self.peek(), b'0' | b'1' | b'_') {
                        self.advance();
                    }
                    let text = self.src[start..self.pos].to_string();
                    self.emit(TokenKind::Integer, start, self.pos, text);
                    return;
                }
                b'o' | b'O' => {
                    self.advance();
                    while !self.at_end() && matches!(self.peek(), b'0'..=b'7' | b'_') {
                        self.advance();
                    }
                    let text = self.src[start..self.pos].to_string();
                    self.emit(TokenKind::Integer, start, self.pos, text);
                    return;
                }
                _ => {}
            }
        }

        while !self.at_end() && (self.peek().is_ascii_digit() || self.peek() == b'_') {
            self.advance();
        }

        let mut is_float = false;

        if !self.at_end()
            && self.peek() == b'.'
            && self.peek_at(1) != b'.'
            && self.peek_at(1).is_ascii_digit()
        {
            is_float = true;
            self.advance();
            while !self.at_end() && (self.peek().is_ascii_digit() || self.peek() == b'_') {
                self.advance();
            }
        }

        if !self.at_end() && matches!(self.peek(), b'e' | b'E') {
            is_float = true;
            self.advance();
            if !self.at_end() && matches!(self.peek(), b'+' | b'-') {
                self.advance();
            }
            while !self.at_end() && self.peek().is_ascii_digit() {
                self.advance();
            }
        }

        let text = self.src[start..self.pos].to_string();
        let kind = if is_float {
            TokenKind::Float
        } else {
            TokenKind::Integer
        };
        self.emit(kind, start, self.pos, text);
    }

    fn scan_identifier(&mut self, start: usize) {
        while !self.at_end() && (self.peek().is_ascii_alphanumeric() || self.peek() == b'_') {
            self.advance();
        }

        let word = &self.src[start..self.pos];

        let kind = if let Some(kw) = keyword_lookup(word) {
            kw
        } else if is_builtin_type(word) {
            TokenKind::TypeIdent
        } else {
            TokenKind::Ident
        };

        self.emit(kind, start, self.pos, word.to_string());
    }
}
