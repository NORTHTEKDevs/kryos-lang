# Kryos Self-Hosting Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the Python bootstrap compiler with a production-grade Rust compiler, then rewrite it in Kryos for full self-hosting — zero Python dependency.

**Architecture:** Cargo workspace under `compiler/` with 18 crates. Dual-backend: Cranelift (dev/JIT/REPL) + LLVM via inkwell (release). Ownership + ARC memory model. Hybrid stdlib (Rust FFI for syscalls, Kryos for everything else). C ABI + bindgen. Full toolchain: formatter, doc gen, test runner, package manager, LSP.

**Tech Stack:** Rust (2021 edition), Cranelift, inkwell (LLVM bindings), clap (CLI), lsp-server + lsp-types (LSP), logos (lexer perf optional), serde/serde_json (serialization), ring (crypto), regex (stdlib native)

**Design doc:** `docs/plans/2026-04-02-self-hosting-design.md`

---

## Phase 1: Compiler Foundation (Rust Bootstrap)

### Task 1: Cargo Workspace Scaffolding

**Files:**
- Create: `compiler/Cargo.toml`
- Create: `compiler/crates/kryos-errors/Cargo.toml`
- Create: `compiler/crates/kryos-errors/src/lib.rs`
- Create: `compiler/crates/kryos-ast/Cargo.toml`
- Create: `compiler/crates/kryos-ast/src/lib.rs`
- Create: `compiler/crates/kryos-lexer/Cargo.toml`
- Create: `compiler/crates/kryos-lexer/src/lib.rs`

**Step 1: Create workspace Cargo.toml**

```toml
# compiler/Cargo.toml
[workspace]
resolver = "2"
members = [
    "crates/kryos-errors",
    "crates/kryos-ast",
    "crates/kryos-lexer",
    "crates/kryos-parser",
    "crates/kryos-types",
    "crates/kryos-ownership",
    "crates/kryos-capabilities",
    "crates/kryos-mir",
    "crates/kryos-codegen-cranelift",
    "crates/kryos-codegen-llvm",
    "crates/kryos-linker",
    "crates/kryos-bindgen",
    "crates/kryos-stdlib-native",
    "crates/kryos-driver",
    "crates/kryos-cli",
    "crates/kryos-lsp",
    "crates/kryos-package",
    "crates/kryos-rt",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/FrostbyteDevTeam/kryos-lang"

[workspace.dependencies]
# Internal crates
kryos-errors = { path = "crates/kryos-errors" }
kryos-ast = { path = "crates/kryos-ast" }
kryos-lexer = { path = "crates/kryos-lexer" }
kryos-parser = { path = "crates/kryos-parser" }
kryos-types = { path = "crates/kryos-types" }
kryos-ownership = { path = "crates/kryos-ownership" }
kryos-capabilities = { path = "crates/kryos-capabilities" }
kryos-mir = { path = "crates/kryos-mir" }
kryos-codegen-cranelift = { path = "crates/kryos-codegen-cranelift" }
kryos-codegen-llvm = { path = "crates/kryos-codegen-llvm" }
kryos-linker = { path = "crates/kryos-linker" }
kryos-bindgen = { path = "crates/kryos-bindgen" }
kryos-stdlib-native = { path = "crates/kryos-stdlib-native" }
kryos-driver = { path = "crates/kryos-driver" }
kryos-cli = { path = "crates/kryos-cli" }
kryos-lsp = { path = "crates/kryos-lsp" }
kryos-package = { path = "crates/kryos-package" }
kryos-rt = { path = "crates/kryos-rt" }

# External dependencies
cranelift-codegen = "0.116"
cranelift-frontend = "0.116"
cranelift-jit = "0.116"
cranelift-module = "0.116"
cranelift-native = "0.116"
inkwell = { version = "0.5", features = ["llvm18-0"] }
clap = { version = "4", features = ["derive"] }
lsp-server = "0.7"
lsp-types = "0.97"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ring = "0.17"
regex = "1"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
```

**Step 2: Create kryos-errors crate (minimal stub)**

```toml
# compiler/crates/kryos-errors/Cargo.toml
[package]
name = "kryos-errors"
version.workspace = true
edition.workspace = true

[dependencies]
```

```rust
// compiler/crates/kryos-errors/src/lib.rs
//! Kryos diagnostic engine — errors, warnings, source spans.

/// Source location span: file_id, start byte offset, end byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const DUMMY: Span = Span { file_id: 0, start: 0, end: 0 };

    pub fn new(file_id: u32, start: u32, end: u32) -> Self {
        Self { file_id, start, end }
    }

    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(self.file_id, other.file_id);
        Span {
            file_id: self.file_id,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Info,
    Help,
}

/// A labeled span within a diagnostic.
#[derive(Debug, Clone)]
pub struct Label {
    pub span: Span,
    pub message: String,
    pub is_primary: bool,
}

/// A single diagnostic (error, warning, etc.).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub code: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            code: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: Level::Warning,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            code: None,
        }
    }

    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            is_primary: self.labels.is_empty(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

/// Source file registry — maps file IDs to names and contents.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

#[derive(Debug)]
pub struct SourceFile {
    pub name: String,
    pub source: String,
    line_starts: Vec<u32>,
}

impl SourceMap {
    pub fn add_file(&mut self, name: String, source: String) -> u32 {
        let id = self.files.len() as u32;
        let line_starts = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| (i + 1) as u32))
            .collect();
        self.files.push(SourceFile { name, source, line_starts });
        id
    }

    pub fn get_file(&self, id: u32) -> Option<&SourceFile> {
        self.files.get(id as usize)
    }

    /// Convert a byte offset to (line, column), both 1-based.
    pub fn offset_to_line_col(&self, file_id: u32, offset: u32) -> (u32, u32) {
        let file = &self.files[file_id as usize];
        let line = file.line_starts.partition_point(|&s| s <= offset);
        let line_start = file.line_starts[line - 1];
        (line as u32, offset - line_start + 1)
    }
}

/// Renders a diagnostic to a string (rustc-style output).
pub fn render_diagnostic(diag: &Diagnostic, source_map: &SourceMap) -> String {
    let level_str = match diag.level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Info => "info",
        Level::Help => "help",
    };

    let mut out = String::new();
    if let Some(ref code) = diag.code {
        out.push_str(&format!("{level_str}[{code}]: {}\n", diag.message));
    } else {
        out.push_str(&format!("{level_str}: {}\n", diag.message));
    }

    for label in &diag.labels {
        if let Some(file) = source_map.get_file(label.span.file_id) {
            let (line, col) = source_map.offset_to_line_col(label.span.file_id, label.span.start);
            let arrow = if label.is_primary { "-->" } else { "   " };
            out.push_str(&format!(" {arrow} {}:{line}:{col}\n", file.name));

            // Show the source line
            let line_idx = (line - 1) as usize;
            if line_idx < file.line_starts.len() {
                let start = file.line_starts[line_idx] as usize;
                let end = file.line_starts.get(line_idx + 1)
                    .map(|&s| s as usize)
                    .unwrap_or(file.source.len());
                let src_line = &file.source[start..end].trim_end();
                out.push_str(&format!("  {line} | {src_line}\n"));

                // Underline
                let col_start = (col - 1) as usize;
                let span_len = (label.span.end - label.span.start) as usize;
                let padding = " ".repeat(col_start);
                let underline = "^".repeat(span_len.max(1));
                let line_num_width = format!("{line}").len();
                let gutter = " ".repeat(line_num_width + 2);
                out.push_str(&format!("{gutter}| {padding}{underline} {}\n", label.message));
            }
        }
    }

    for note in &diag.notes {
        out.push_str(&format!("  = note: {note}\n"));
    }

    out
}
```

**Step 3: Create kryos-ast crate (stub)**

```toml
# compiler/crates/kryos-ast/Cargo.toml
[package]
name = "kryos-ast"
version.workspace = true
edition.workspace = true

[dependencies]
kryos-errors.workspace = true
serde = { workspace = true, optional = true }

[features]
default = []
serialize = ["serde"]
```

```rust
// compiler/crates/kryos-ast/src/lib.rs
//! Kryos AST node definitions.
pub use kryos_errors::Span;
```

**Step 4: Create kryos-lexer crate (stub)**

```toml
# compiler/crates/kryos-lexer/Cargo.toml
[package]
name = "kryos-lexer"
version.workspace = true
edition.workspace = true

[dependencies]
kryos-errors.workspace = true

[dev-dependencies]
```

```rust
// compiler/crates/kryos-lexer/src/lib.rs
//! Kryos lexer — tokenizes UTF-8 source into a token stream.
pub use kryos_errors::Span;
```

**Step 5: Verify workspace compiles**

Run: `cd compiler && cargo check`
Expected: compiles with 0 errors, 0 warnings

**Step 6: Commit**

```bash
git add compiler/
git commit -m "feat(compiler): scaffold Cargo workspace — 18 crates, dual-backend deps"
```

---

### Task 2: Error & Diagnostic System (kryos-errors)

Tests and implementation already in Task 1's kryos-errors code. This task adds tests.

**Files:**
- Create: `compiler/crates/kryos-errors/tests/diagnostics.rs`

**Step 1: Write tests for Span, SourceMap, Diagnostic rendering**

```rust
// compiler/crates/kryos-errors/tests/diagnostics.rs
use kryos_errors::*;

#[test]
fn span_merge() {
    let a = Span::new(0, 5, 10);
    let b = Span::new(0, 8, 15);
    let merged = a.merge(b);
    assert_eq!(merged.start, 5);
    assert_eq!(merged.end, 15);
}

#[test]
fn source_map_line_col() {
    let mut sm = SourceMap::default();
    let fid = sm.add_file("test.kry".into(), "let x = 42\nlet y = 10\n".into());
    assert_eq!(sm.offset_to_line_col(fid, 0), (1, 1));  // 'l' of first let
    assert_eq!(sm.offset_to_line_col(fid, 4), (1, 5));  // 'x'
    assert_eq!(sm.offset_to_line_col(fid, 11), (1, 12)); // '\n'
    assert_eq!(sm.offset_to_line_col(fid, 12), (2, 1));  // 'l' of second let
}

#[test]
fn diagnostic_rendering() {
    let mut sm = SourceMap::default();
    let fid = sm.add_file("main.kry".into(), "let x = 42\n".into());
    let diag = Diagnostic::error("type mismatch")
        .with_label(Span::new(fid, 8, 10), "expected string, found i32")
        .with_code("E0308");
    let rendered = render_diagnostic(&diag, &sm);
    assert!(rendered.contains("error[E0308]: type mismatch"));
    assert!(rendered.contains("main.kry:1:9"));
    assert!(rendered.contains("expected string, found i32"));
}

#[test]
fn diagnostic_warning() {
    let diag = Diagnostic::warning("unused variable")
        .with_note("prefix with _ to silence");
    let sm = SourceMap::default();
    let rendered = render_diagnostic(&diag, &sm);
    assert!(rendered.contains("warning: unused variable"));
    assert!(rendered.contains("prefix with _ to silence"));
}
```

**Step 2: Run tests**

Run: `cd compiler && cargo test -p kryos-errors`
Expected: 4 tests pass

**Step 3: Commit**

```bash
git add compiler/crates/kryos-errors/
git commit -m "feat(errors): diagnostic engine — spans, source map, rustc-style rendering"
```

---

### Task 3: Token Definitions (kryos-lexer)

Port all token types from `kryos/compiler/tokens.py`.

**Files:**
- Create: `compiler/crates/kryos-lexer/src/token.rs`
- Modify: `compiler/crates/kryos-lexer/src/lib.rs`

**Step 1: Write the token types**

```rust
// compiler/crates/kryos-lexer/src/token.rs
use kryos_errors::Span;

/// Every token kind the Kryos lexer produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // --- Literals ---
    Integer,       // 42, 0xFF, 0b1010, 0o77, 1_000_000
    Float,         // 3.14, 1.0e10, 2.5e-3
    String,        // "hello" (complete, no interpolation)
    StringPart,    // segment of interpolated string
    InterpStart,   // opening { inside interpolated string
    InterpEnd,     // closing } inside interpolated string
    Char,          // 'a', '\n'
    True,          // true
    False,         // false
    None,          // none

    // --- Identifiers ---
    Ident,         // user-defined names
    TypeIdent,     // built-in type identifiers (i32, Tensor, etc.)

    // --- Keywords ---
    Let, Mut, Fn, Return, If, Else, Elif,
    For, While, In, Break, Continue,
    Struct, Enum, Impl, Trait,
    Pub, Use, Extern, As, Mod, Type,
    Actor, Spawn, Select, Send, Recv, Ask, Chan,
    Parallel, Quantum, Comptime,
    Match, And, Or, Not,
    Try, Catch, Throw,
    Shared, Weak, Move,   // ARC/ownership keywords

    // --- Operators ---
    Plus, Minus, Star, Slash, Percent, Power,  // + - * / % **
    At,                                         // @ (matrix mul + annotation prefix)
    EqEq, BangEq, Lt, Gt, LtEq, GtEq,        // == != < > <= >=
    Eq, PlusEq, MinusEq, StarEq, SlashEq,     // = += -= *= /=
    Amp, Pipe, Caret, Tilde, Shl, Shr,        // & | ^ ~ << >>

    // --- Punctuation ---
    Arrow,      // ->
    FatArrow,   // =>
    ColonColon, // ::
    DotDot,     // ..
    DotDotEq,   // ..=
    Dot,        // .
    Colon,      // :
    Semicolon,  // ;
    Comma,      // ,

    // --- Grouping ---
    LParen, RParen,     // ( )
    LBrace, RBrace,     // { }
    LBracket, RBracket, // [ ]

    // --- Special ---
    Newline,
    Eof,
    Error,
}

impl TokenKind {
    /// Returns true if this token is a keyword.
    pub fn is_keyword(self) -> bool {
        matches!(self,
            Self::Let | Self::Mut | Self::Fn | Self::Return |
            Self::If | Self::Else | Self::Elif |
            Self::For | Self::While | Self::In | Self::Break | Self::Continue |
            Self::Struct | Self::Enum | Self::Impl | Self::Trait |
            Self::Pub | Self::Use | Self::Extern | Self::As | Self::Mod | Self::Type |
            Self::Actor | Self::Spawn | Self::Select | Self::Send | Self::Recv |
            Self::Ask | Self::Chan | Self::Parallel | Self::Quantum | Self::Comptime |
            Self::Match | Self::And | Self::Or | Self::Not |
            Self::Try | Self::Catch | Self::Throw |
            Self::Shared | Self::Weak | Self::Move |
            Self::True | Self::False | Self::None
        )
    }
}

/// A single token with its span and source text.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span, text: impl Into<String>) -> Self {
        Self { kind, span, text: text.into() }
    }

    pub fn dummy(kind: TokenKind) -> Self {
        Self { kind, span: Span::DUMMY, text: String::new() }
    }
}

/// Keyword lookup — maps string to TokenKind.
pub fn keyword_lookup(word: &str) -> Option<TokenKind> {
    Some(match word {
        "let" => TokenKind::Let,
        "mut" => TokenKind::Mut,
        "fn" => TokenKind::Fn,
        "return" => TokenKind::Return,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "elif" => TokenKind::Elif,
        "for" => TokenKind::For,
        "while" => TokenKind::While,
        "in" => TokenKind::In,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "impl" => TokenKind::Impl,
        "trait" => TokenKind::Trait,
        "pub" => TokenKind::Pub,
        "use" => TokenKind::Use,
        "extern" => TokenKind::Extern,
        "as" => TokenKind::As,
        "mod" => TokenKind::Mod,
        "type" => TokenKind::Type,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "none" => TokenKind::None,
        "actor" => TokenKind::Actor,
        "spawn" => TokenKind::Spawn,
        "select" => TokenKind::Select,
        "send" => TokenKind::Send,
        "recv" => TokenKind::Recv,
        "ask" => TokenKind::Ask,
        "chan" => TokenKind::Chan,
        "parallel" => TokenKind::Parallel,
        "quantum" => TokenKind::Quantum,
        "comptime" => TokenKind::Comptime,
        "match" => TokenKind::Match,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "try" => TokenKind::Try,
        "catch" => TokenKind::Catch,
        "throw" => TokenKind::Throw,
        "shared" => TokenKind::Shared,
        "weak" => TokenKind::Weak,
        "move" => TokenKind::Move,
        _ => return Option::None,
    })
}

/// Built-in type names recognized by the lexer.
pub fn is_builtin_type(name: &str) -> bool {
    matches!(name,
        "i8" | "i16" | "i32" | "i64" | "i128" |
        "u8" | "u16" | "u32" | "u64" | "u128" |
        "f32" | "f64" |
        "bool" | "str" | "char" | "usize" | "isize" |
        "Tensor" | "Vec" | "Map" | "Set" | "Option" | "Result" | "Secret" | "Qubit" | "Qureg"
    )
}

/// Attribute keyword lookup — for tokens after @.
pub fn attribute_lookup(word: &str) -> bool {
    matches!(word,
        "capabilities" | "compute" | "export" | "layout" |
        "real_time" | "no_std" | "zero_copy" | "target" |
        "differentiable" | "test" | "bench" | "actor" |
        "budget" | "sandbox" | "repr" | "copy" | "allocator"
    )
}
```

**Step 2: Update lib.rs**

```rust
// compiler/crates/kryos-lexer/src/lib.rs
pub mod token;

pub use token::*;
pub use kryos_errors::Span;
```

**Step 3: Write tests for keyword_lookup and is_builtin_type**

Create `compiler/crates/kryos-lexer/tests/tokens.rs`:

```rust
use kryos_lexer::*;

#[test]
fn all_keywords_resolve() {
    assert_eq!(keyword_lookup("let"), Some(TokenKind::Let));
    assert_eq!(keyword_lookup("fn"), Some(TokenKind::Fn));
    assert_eq!(keyword_lookup("match"), Some(TokenKind::Match));
    assert_eq!(keyword_lookup("shared"), Some(TokenKind::Shared));
    assert_eq!(keyword_lookup("weak"), Some(TokenKind::Weak));
    assert_eq!(keyword_lookup("move"), Some(TokenKind::Move));
    assert_eq!(keyword_lookup("notakeyword"), None);
}

#[test]
fn builtin_types() {
    assert!(is_builtin_type("i32"));
    assert!(is_builtin_type("f64"));
    assert!(is_builtin_type("Vec"));
    assert!(is_builtin_type("Option"));
    assert!(is_builtin_type("usize"));
    assert!(!is_builtin_type("MyStruct"));
}

#[test]
fn token_kind_is_keyword() {
    assert!(TokenKind::Let.is_keyword());
    assert!(TokenKind::Shared.is_keyword());
    assert!(!TokenKind::Plus.is_keyword());
    assert!(!TokenKind::Integer.is_keyword());
}
```

**Step 4: Run tests**

Run: `cd compiler && cargo test -p kryos-lexer`
Expected: 3 tests pass

**Step 5: Commit**

```bash
git add compiler/crates/kryos-lexer/
git commit -m "feat(lexer): token definitions — all keywords, operators, builtins from Python bootstrap"
```

---

### Task 4: Lexer Implementation (kryos-lexer)

Full tokenizer ported from Python's `Lexer` class.

**Files:**
- Create: `compiler/crates/kryos-lexer/src/lexer.rs`
- Modify: `compiler/crates/kryos-lexer/src/lib.rs`
- Create: `compiler/crates/kryos-lexer/tests/lexer.rs`

**Step 1: Write lexer tests first**

```rust
// compiler/crates/kryos-lexer/tests/lexer.rs
use kryos_lexer::*;

fn lex(src: &str) -> Vec<Token> {
    Lexer::new(src, 0).tokenize()
}

fn kinds(src: &str) -> Vec<TokenKind> {
    lex(src).into_iter().map(|t| t.kind).collect()
}

#[test]
fn empty_source() {
    assert_eq!(kinds(""), vec![TokenKind::Eof]);
}

#[test]
fn let_binding() {
    assert_eq!(
        kinds("let x = 42"),
        vec![TokenKind::Let, TokenKind::Ident, TokenKind::Eq, TokenKind::Integer, TokenKind::Eof]
    );
}

#[test]
fn float_literal() {
    let tokens = lex("3.14");
    assert_eq!(tokens[0].kind, TokenKind::Float);
    assert_eq!(tokens[0].text, "3.14");
}

#[test]
fn string_literal() {
    let tokens = lex(r#""hello world""#);
    assert_eq!(tokens[0].kind, TokenKind::String);
    assert_eq!(tokens[0].text, "hello world");
}

#[test]
fn char_literal() {
    let tokens = lex("'a'");
    assert_eq!(tokens[0].kind, TokenKind::Char);
    assert_eq!(tokens[0].text, "a");
}

#[test]
fn hex_binary_octal() {
    assert_eq!(lex("0xFF")[0].text, "0xFF");
    assert_eq!(lex("0b1010")[0].text, "0b1010");
    assert_eq!(lex("0o77")[0].text, "0o77");
}

#[test]
fn underscore_separators() {
    let tokens = lex("1_000_000");
    assert_eq!(tokens[0].kind, TokenKind::Integer);
    assert_eq!(tokens[0].text, "1_000_000");
}

#[test]
fn operators() {
    assert_eq!(
        kinds("+ - * / % ** -> => :: .. ..= == != <= >="),
        vec![
            TokenKind::Plus, TokenKind::Minus, TokenKind::Star, TokenKind::Slash,
            TokenKind::Percent, TokenKind::Power, TokenKind::Arrow, TokenKind::FatArrow,
            TokenKind::ColonColon, TokenKind::DotDot, TokenKind::DotDotEq,
            TokenKind::EqEq, TokenKind::BangEq, TokenKind::LtEq, TokenKind::GtEq,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn function_definition() {
    assert_eq!(
        kinds("fn add(a: i32, b: i32) -> i32 { return a + b }"),
        vec![
            TokenKind::Fn, TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::Ident, TokenKind::Colon, TokenKind::TypeIdent, TokenKind::Comma,
            TokenKind::Ident, TokenKind::Colon, TokenKind::TypeIdent,
            TokenKind::RParen,
            TokenKind::Arrow, TokenKind::TypeIdent,
            TokenKind::LBrace,
            TokenKind::Return, TokenKind::Ident, TokenKind::Plus, TokenKind::Ident,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn annotation_at() {
    assert_eq!(
        kinds("@export fn main() {}"),
        vec![
            TokenKind::At, TokenKind::Ident,
            TokenKind::Fn, TokenKind::Ident,
            TokenKind::LParen, TokenKind::RParen,
            TokenKind::LBrace, TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn comments_skipped() {
    assert_eq!(
        kinds("let x = 1 // comment\nlet y = 2"),
        vec![
            TokenKind::Let, TokenKind::Ident, TokenKind::Eq, TokenKind::Integer,
            TokenKind::Let, TokenKind::Ident, TokenKind::Eq, TokenKind::Integer,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn multiline_comments() {
    assert_eq!(
        kinds("let a /* block\ncomment */ = 1"),
        vec![
            TokenKind::Let, TokenKind::Ident, TokenKind::Eq, TokenKind::Integer,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn string_interpolation() {
    let tokens = lex(r#""hello {name}!""#);
    let k: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(k, vec![
        TokenKind::StringPart,   // "hello "
        TokenKind::InterpStart,  // {
        TokenKind::Ident,        // name
        TokenKind::InterpEnd,    // }
        TokenKind::StringPart,   // "!"
        TokenKind::Eof,
    ]);
}

#[test]
fn escape_sequences() {
    let tokens = lex(r#""\n\t\\\"""#);
    assert_eq!(tokens[0].kind, TokenKind::String);
    assert_eq!(tokens[0].text, "\n\t\\\"");
}

#[test]
fn scientific_notation() {
    let tokens = lex("1.5e10 2.5e-3");
    assert_eq!(tokens[0].kind, TokenKind::Float);
    assert_eq!(tokens[0].text, "1.5e10");
    assert_eq!(tokens[1].kind, TokenKind::Float);
    assert_eq!(tokens[1].text, "2.5e-3");
}

#[test]
fn span_tracking() {
    let tokens = lex("let x");
    // "let" starts at byte 0, length 3
    assert_eq!(tokens[0].span.start, 0);
    assert_eq!(tokens[0].span.end, 3);
    // "x" starts at byte 4
    assert_eq!(tokens[1].span.start, 4);
    assert_eq!(tokens[1].span.end, 5);
}

#[test]
fn shared_weak_move_keywords() {
    assert_eq!(
        kinds("shared weak move"),
        vec![TokenKind::Shared, TokenKind::Weak, TokenKind::Move, TokenKind::Eof]
    );
}

#[test]
fn actor_concurrency_keywords() {
    assert_eq!(
        kinds("actor spawn select send recv ask chan"),
        vec![
            TokenKind::Actor, TokenKind::Spawn, TokenKind::Select,
            TokenKind::Send, TokenKind::Recv, TokenKind::Ask, TokenKind::Chan,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn bitwise_operators() {
    assert_eq!(
        kinds("& | ^ ~ << >>"),
        vec![
            TokenKind::Amp, TokenKind::Pipe, TokenKind::Caret,
            TokenKind::Tilde, TokenKind::Shl, TokenKind::Shr,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn assignment_operators() {
    assert_eq!(
        kinds("+= -= *= /="),
        vec![
            TokenKind::PlusEq, TokenKind::MinusEq, TokenKind::StarEq, TokenKind::SlashEq,
            TokenKind::Eof,
        ]
    );
}
```

**Step 2: Run tests — verify they fail**

Run: `cd compiler && cargo test -p kryos-lexer -- --test lexer`
Expected: compilation error (Lexer struct not defined yet)

**Step 3: Implement the Lexer**

Create `compiler/crates/kryos-lexer/src/lexer.rs` with a full port of the Python lexer:

```rust
// compiler/crates/kryos-lexer/src/lexer.rs
use kryos_errors::Span;
use crate::token::*;

/// Kryos lexer — tokenizes UTF-8 source into a token stream.
pub struct Lexer<'src> {
    src: &'src str,
    bytes: &'src [u8],
    file_id: u32,
    pos: usize,
    tokens: Vec<Token>,
}

impl<'src> Lexer<'src> {
    pub fn new(src: &'src str, file_id: u32) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            file_id,
            pos: 0,
            tokens: Vec::new(),
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

    // --- Character access ---

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn peek(&self) -> u8 {
        if self.at_end() { 0 } else { self.bytes[self.pos] }
    }

    fn peek_at(&self, offset: usize) -> u8 {
        let idx = self.pos + offset;
        if idx >= self.bytes.len() { 0 } else { self.bytes[idx] }
    }

    fn advance(&mut self) -> u8 {
        let ch = self.bytes[self.pos];
        self.pos += 1;
        ch
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

    // --- Whitespace & comments ---

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace (but not newlines if we want significant newlines later)
            while !self.at_end() && matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n') {
                self.advance();
            }
            if self.at_end() {
                return;
            }
            // Line comment
            if self.peek() == b'/' && self.peek_at(1) == b'/' {
                while !self.at_end() && self.peek() != b'\n' {
                    self.advance();
                }
                continue;
            }
            // Block comment
            if self.peek() == b'/' && self.peek_at(1) == b'*' {
                self.advance(); // /
                self.advance(); // *
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
                continue;
            }
            break;
        }
    }

    // --- Main scanner ---

    fn scan_token(&mut self) {
        let start = self.pos;
        let ch = self.advance();

        match ch {
            // Grouping
            b'(' => self.emit(TokenKind::LParen, start, self.pos, "(".into()),
            b')' => self.emit(TokenKind::RParen, start, self.pos, ")".into()),
            b'{' => self.emit(TokenKind::LBrace, start, self.pos, "{".into()),
            b'}' => self.emit(TokenKind::RBrace, start, self.pos, "}".into()),
            b'[' => self.emit(TokenKind::LBracket, start, self.pos, "[".into()),
            b']' => self.emit(TokenKind::RBracket, start, self.pos, "]".into()),

            // Punctuation
            b';' => self.emit(TokenKind::Semicolon, start, self.pos, ";".into()),
            b',' => self.emit(TokenKind::Comma, start, self.pos, ",".into()),
            b'~' => self.emit(TokenKind::Tilde, start, self.pos, "~".into()),
            b'@' => self.emit(TokenKind::At, start, self.pos, "@".into()),
            b'^' => self.emit(TokenKind::Caret, start, self.pos, "^".into()),
            b'&' => self.emit(TokenKind::Amp, start, self.pos, "&".into()),

            // Multi-char operators
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
                    self.emit(TokenKind::Error, start, self.pos, "!".into());
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
            b'|' => self.emit(TokenKind::Pipe, start, self.pos, "|".into()),

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

            // String literals
            b'"' => self.scan_string(start),

            // Char literals
            b'\'' => self.scan_char(start),

            // Numbers
            b'0'..=b'9' => self.scan_number(start),

            // Identifiers and keywords
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.scan_identifier(start),

            _ => {
                let text = String::from(ch as char);
                self.emit(TokenKind::Error, start, self.pos, text);
            }
        }
    }

    // --- String scanning ---

    fn scan_string(&mut self, start: usize) {
        let mut text = String::new();
        let mut has_interpolation = false;

        while !self.at_end() && self.peek() != b'"' {
            if self.peek() == b'{' {
                // Start of interpolation
                if !text.is_empty() || !has_interpolation {
                    self.emit(TokenKind::StringPart, start, self.pos, text.clone());
                    text.clear();
                }
                has_interpolation = true;
                let brace_start = self.pos;
                self.advance(); // consume {
                self.emit(TokenKind::InterpStart, brace_start, self.pos, "{".into());

                // Scan tokens inside interpolation until }
                while !self.at_end() && self.peek() != b'}' {
                    self.skip_whitespace_and_comments();
                    if !self.at_end() && self.peek() != b'}' {
                        self.scan_token();
                    }
                }
                if !self.at_end() {
                    let end_start = self.pos;
                    self.advance(); // consume }
                    self.emit(TokenKind::InterpEnd, end_start, self.pos, "}".into());
                }
                continue;
            }

            if self.peek() == b'\\' {
                self.advance(); // consume backslash
                if !self.at_end() {
                    let esc = self.advance();
                    match esc {
                        b'n' => text.push('\n'),
                        b't' => text.push('\t'),
                        b'r' => text.push('\r'),
                        b'\\' => text.push('\\'),
                        b'"' => text.push('"'),
                        b'0' => text.push('\0'),
                        _ => {
                            text.push('\\');
                            text.push(esc as char);
                        }
                    }
                }
                continue;
            }

            text.push(self.advance() as char);
        }

        if !self.at_end() {
            self.advance(); // consume closing "
        }

        if has_interpolation {
            // Emit the remaining part after last interpolation
            self.emit(TokenKind::StringPart, start, self.pos, text);
        } else {
            self.emit(TokenKind::String, start, self.pos, text);
        }
    }

    // --- Char scanning ---

    fn scan_char(&mut self, start: usize) {
        let ch = if self.peek() == b'\\' {
            self.advance(); // backslash
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
            self.advance() as char
        };

        if !self.at_end() && self.peek() == b'\'' {
            self.advance(); // closing '
        }

        self.emit(TokenKind::Char, start, self.pos, ch.to_string());
    }

    // --- Number scanning ---

    fn scan_number(&mut self, start: usize) {
        // Check for 0x, 0b, 0o prefixes
        if self.src.as_bytes()[start] == b'0' && !self.at_end() {
            match self.peek() {
                b'x' | b'X' => {
                    self.advance();
                    while !self.at_end() && (self.peek().is_ascii_hexdigit() || self.peek() == b'_') {
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

        // Decimal digits
        while !self.at_end() && (self.peek().is_ascii_digit() || self.peek() == b'_') {
            self.advance();
        }

        let mut is_float = false;

        // Check for decimal point (but not .. range)
        if !self.at_end() && self.peek() == b'.' && self.peek_at(1) != b'.' {
            if self.peek_at(1).is_ascii_digit() {
                is_float = true;
                self.advance(); // consume .
                while !self.at_end() && (self.peek().is_ascii_digit() || self.peek() == b'_') {
                    self.advance();
                }
            }
        }

        // Check for scientific notation
        if !self.at_end() && matches!(self.peek(), b'e' | b'E') {
            is_float = true;
            self.advance(); // consume e
            if !self.at_end() && matches!(self.peek(), b'+' | b'-') {
                self.advance();
            }
            while !self.at_end() && self.peek().is_ascii_digit() {
                self.advance();
            }
        }

        let text = self.src[start..self.pos].to_string();
        let kind = if is_float { TokenKind::Float } else { TokenKind::Integer };
        self.emit(kind, start, self.pos, text);
    }

    // --- Identifier/keyword scanning ---

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
```

**Step 4: Update lib.rs**

```rust
// compiler/crates/kryos-lexer/src/lib.rs
pub mod token;
pub mod lexer;

pub use token::*;
pub use lexer::*;
pub use kryos_errors::Span;
```

**Step 5: Run tests**

Run: `cd compiler && cargo test -p kryos-lexer`
Expected: All 20 tests pass

**Step 6: Commit**

```bash
git add compiler/crates/kryos-lexer/
git commit -m "feat(lexer): full tokenizer — string interp, escape sequences, hex/bin/oct, comments"
```

---

### Task 5: AST Node Definitions (kryos-ast)

Port all AST nodes from `kryos/compiler/ast_nodes.py`.

**Files:**
- Create: `compiler/crates/kryos-ast/src/types.rs`
- Create: `compiler/crates/kryos-ast/src/expr.rs`
- Create: `compiler/crates/kryos-ast/src/stmt.rs`
- Create: `compiler/crates/kryos-ast/src/decl.rs`
- Create: `compiler/crates/kryos-ast/src/visitor.rs`
- Modify: `compiler/crates/kryos-ast/src/lib.rs`

**Step 1: Define type AST nodes**

The full type node hierarchy matching Python's TypeNode, SimpleType, GenericType, ArrayType, FnType, OptionType, ReferenceType, TupleType:

```rust
// compiler/crates/kryos-ast/src/types.rs
use kryos_errors::Span;

/// Type annotation AST nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// Simple named type: `i32`, `bool`, `MyStruct`
    Simple { name: String, span: Span },
    /// Generic type: `Vec<i32>`, `Map<String, i32>`, `Result<T, E>`
    Generic { name: String, args: Vec<TypeExpr>, span: Span },
    /// Array type: `[i32; 10]` (fixed) or `[i32]` (slice)
    Array { element: Box<TypeExpr>, size: Option<u64>, span: Span },
    /// Tuple type: `(i32, String, bool)`
    Tuple { elements: Vec<TypeExpr>, span: Span },
    /// Function type: `fn(i32, i32) -> i32`
    Function { params: Vec<TypeExpr>, ret: Box<TypeExpr>, span: Span },
    /// Option shorthand: `?i32`
    Optional { inner: Box<TypeExpr>, span: Span },
    /// Reference: `&T` or `&mut T`
    Reference { inner: Box<TypeExpr>, mutable: bool, span: Span },
    /// Shared (ARC): `shared T`
    Shared { inner: Box<TypeExpr>, span: Span },
    /// Weak reference: `weak T`
    Weak { inner: Box<TypeExpr>, span: Span },
    /// Pointer (FFI): `*T` or `*mut T`
    Pointer { inner: Box<TypeExpr>, mutable: bool, span: Span },
    /// Inferred type (omitted annotation)
    Inferred { span: Span },
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            Self::Simple { span, .. } | Self::Generic { span, .. } |
            Self::Array { span, .. } | Self::Tuple { span, .. } |
            Self::Function { span, .. } | Self::Optional { span, .. } |
            Self::Reference { span, .. } | Self::Shared { span, .. } |
            Self::Weak { span, .. } | Self::Pointer { span, .. } |
            Self::Inferred { span } => *span,
        }
    }
}
```

**Step 2: Define expression nodes**

```rust
// compiler/crates/kryos-ast/src/expr.rs
use kryos_errors::Span;
use crate::types::TypeExpr;
use crate::stmt::Block;

/// Binary operator kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod, Pow,
    Eq, Neq, Lt, Gt, LtEq, GtEq,
    And, Or,
    BitAnd, BitOr, BitXor, Shl, Shr,
    Pipe,       // |> pipe operator
    MatMul,     // @ matrix multiply
}

/// Unary operator kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,    // -x
    Not,    // not x
    BitNot, // ~x
}

/// Function/closure parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeExpr>,
    pub default: Option<Box<Expr>>,
    pub span: Span,
}

/// Match arm for match expressions.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Box<Expr>>,
    pub body: Box<Expr>,
    pub span: Span,
}

/// Patterns for match/let destructuring.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard { span: Span },
    Ident { name: String, mutable: bool, span: Span },
    Literal { expr: Box<Expr>, span: Span },
    Tuple { elements: Vec<Pattern>, span: Span },
    Struct { name: String, fields: Vec<(String, Pattern)>, span: Span },
    Enum { name: String, variant: String, fields: Vec<Pattern>, span: Span },
    Or { patterns: Vec<Pattern>, span: Span },
}

/// Expression AST nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // --- Literals ---
    IntLiteral { value: i64, span: Span },
    FloatLiteral { value: f64, span: Span },
    StringLiteral { value: String, span: Span },
    InterpolatedString { parts: Vec<StringPart>, span: Span },
    CharLiteral { value: char, span: Span },
    BoolLiteral { value: bool, span: Span },
    NoneLiteral { span: Span },

    // --- References ---
    Identifier { name: String, span: Span },
    FieldAccess { object: Box<Expr>, field: String, span: Span },
    IndexAccess { object: Box<Expr>, index: Box<Expr>, span: Span },

    // --- Operations ---
    BinaryOp { op: BinOp, left: Box<Expr>, right: Box<Expr>, span: Span },
    UnaryOp { op: UnOp, operand: Box<Expr>, span: Span },

    // --- Calls ---
    FnCall { callee: Box<Expr>, args: Vec<Expr>, span: Span },
    MethodCall { object: Box<Expr>, method: String, args: Vec<Expr>, span: Span },

    // --- Constructors ---
    ArrayLiteral { elements: Vec<Expr>, span: Span },
    TupleLiteral { elements: Vec<Expr>, span: Span },
    MapLiteral { entries: Vec<(Expr, Expr)>, span: Span },
    StructLiteral { name: String, fields: Vec<(String, Expr)>, span: Span },

    // --- Closures ---
    Lambda { params: Vec<Param>, ret_ty: Option<TypeExpr>, body: Box<Expr>, span: Span },

    // --- Control flow expressions ---
    IfExpr { condition: Box<Expr>, then_branch: Block, else_branch: Option<Block>, span: Span },
    MatchExpr { subject: Box<Expr>, arms: Vec<MatchArm>, span: Span },
    RangeExpr { start: Option<Box<Expr>>, end: Option<Box<Expr>>, inclusive: bool, span: Span },

    // --- Pipe ---
    PipeExpr { left: Box<Expr>, right: Box<Expr>, span: Span },

    // --- Ownership ---
    SharedExpr { inner: Box<Expr>, span: Span },      // shared expr
    MoveExpr { inner: Box<Expr>, span: Span },         // move expr
    WeakExpr { inner: Box<Expr>, span: Span },         // weak expr

    // --- Special blocks ---
    ComptimeBlock { body: Block, span: Span },
    QuantumBlock { body: Block, span: Span },

    // --- Type cast ---
    Cast { expr: Box<Expr>, ty: TypeExpr, span: Span },  // expr as Type

    // --- Block expression ---
    Block { block: Block, span: Span },
}

/// Part of an interpolated string.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Literal(String),
    Expr(Box<Expr>),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::IntLiteral { span, .. } | Self::FloatLiteral { span, .. } |
            Self::StringLiteral { span, .. } | Self::InterpolatedString { span, .. } |
            Self::CharLiteral { span, .. } | Self::BoolLiteral { span, .. } |
            Self::NoneLiteral { span } | Self::Identifier { span, .. } |
            Self::FieldAccess { span, .. } | Self::IndexAccess { span, .. } |
            Self::BinaryOp { span, .. } | Self::UnaryOp { span, .. } |
            Self::FnCall { span, .. } | Self::MethodCall { span, .. } |
            Self::ArrayLiteral { span, .. } | Self::TupleLiteral { span, .. } |
            Self::MapLiteral { span, .. } | Self::StructLiteral { span, .. } |
            Self::Lambda { span, .. } | Self::IfExpr { span, .. } |
            Self::MatchExpr { span, .. } | Self::RangeExpr { span, .. } |
            Self::PipeExpr { span, .. } | Self::SharedExpr { span, .. } |
            Self::MoveExpr { span, .. } | Self::WeakExpr { span, .. } |
            Self::ComptimeBlock { span, .. } | Self::QuantumBlock { span, .. } |
            Self::Cast { span, .. } | Self::Block { span, .. } => *span,
        }
    }
}
```

**Step 3: Define statement nodes**

```rust
// compiler/crates/kryos-ast/src/stmt.rs
use kryos_errors::Span;
use crate::types::TypeExpr;
use crate::expr::{Expr, Pattern, Param};

/// A block of statements.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

/// Select branch for actor select statements.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectBranch {
    pub pattern: String,
    pub channel: Expr,
    pub body: Block,
    pub span: Span,
}

/// Statement AST nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `let [mut] name [: Type] = expr`
    Let {
        name: String,
        mutable: bool,
        ty: Option<TypeExpr>,
        value: Option<Expr>,
        pattern: Option<Pattern>,
        span: Span,
    },
    /// `name = expr` or `obj.field = expr` or `arr[i] = expr`
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    /// `return [expr]`
    Return { value: Option<Expr>, span: Span },
    /// `if cond { ... } [elif cond { ... }]* [else { ... }]`
    If {
        condition: Expr,
        then_block: Block,
        elif_clauses: Vec<(Expr, Block)>,
        else_block: Option<Block>,
        span: Span,
    },
    /// `for pattern in iterable { ... }`
    For {
        pattern: Pattern,
        iterable: Expr,
        body: Block,
        span: Span,
    },
    /// `while condition { ... }`
    While {
        condition: Expr,
        body: Block,
        span: Span,
    },
    /// `break`
    Break { span: Span },
    /// `continue`
    Continue { span: Span },
    /// Expression used as a statement
    Expr { expr: Expr, span: Span },
    /// `spawn expr`
    Spawn { expr: Expr, span: Span },
    /// `select { branch* }`
    Select { branches: Vec<SelectBranch>, span: Span },
    /// `try { ... } catch e { ... }`
    TryCatch {
        try_block: Block,
        catch_name: String,
        catch_block: Block,
        span: Span,
    },
    /// `throw expr`
    Throw { expr: Expr, span: Span },
}

/// Assignment operator variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,    // =
    AddAssign, // +=
    SubAssign, // -=
    MulAssign, // *=
    DivAssign, // /=
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Self::Let { span, .. } | Self::Assign { span, .. } |
            Self::Return { span, .. } | Self::If { span, .. } |
            Self::For { span, .. } | Self::While { span, .. } |
            Self::Break { span } | Self::Continue { span } |
            Self::Expr { span, .. } | Self::Spawn { span, .. } |
            Self::Select { span, .. } | Self::TryCatch { span, .. } |
            Self::Throw { span, .. } => *span,
        }
    }
}
```

**Step 4: Define declaration nodes**

```rust
// compiler/crates/kryos-ast/src/decl.rs
use kryos_errors::Span;
use crate::types::TypeExpr;
use crate::expr::{Expr, Param};
use crate::stmt::Block;

/// Annotation (e.g., @export, @capabilities(net, io), @budget(1000))
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<String>,
    pub span: Span,
}

/// Generic type parameter: `T`, `T: Ord`, `T: Display + Debug`
#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
    pub span: Span,
}

/// Struct field.
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub ty: TypeExpr,
    pub public: bool,
    pub default: Option<Expr>,
    pub span: Span,
}

/// Enum variant.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<TypeExpr>,
    pub span: Span,
}

/// Message handler in an actor declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageHandler {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_ty: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

/// Import path segment.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPath {
    pub segments: Vec<String>,
    pub alias: Option<String>,
    pub items: Vec<String>,  // for `use std::io::{File, BufReader}`
    pub span: Span,
}

/// Top-level declaration nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    /// Function declaration
    Function {
        name: String,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        ret_ty: Option<TypeExpr>,
        body: Block,
        public: bool,
        annotations: Vec<Annotation>,
        span: Span,
    },
    /// Struct declaration
    Struct {
        name: String,
        generics: Vec<GenericParam>,
        fields: Vec<StructField>,
        public: bool,
        annotations: Vec<Annotation>,
        span: Span,
    },
    /// Enum declaration
    Enum {
        name: String,
        generics: Vec<GenericParam>,
        variants: Vec<EnumVariant>,
        public: bool,
        annotations: Vec<Annotation>,
        span: Span,
    },
    /// Trait declaration
    Trait {
        name: String,
        generics: Vec<GenericParam>,
        methods: Vec<Decl>,  // Function decls (possibly without body)
        public: bool,
        span: Span,
    },
    /// Impl block
    Impl {
        target: String,
        trait_name: Option<String>,
        generics: Vec<GenericParam>,
        methods: Vec<Decl>,
        span: Span,
    },
    /// Actor declaration
    Actor {
        name: String,
        state_fields: Vec<StructField>,
        handlers: Vec<MessageHandler>,
        annotations: Vec<Annotation>,
        span: Span,
    },
    /// Type alias
    TypeAlias {
        name: String,
        generics: Vec<GenericParam>,
        ty: TypeExpr,
        public: bool,
        span: Span,
    },
    /// Import/use declaration
    Import { path: ImportPath, span: Span },
    /// Extern block (FFI declarations)
    Extern {
        abi: String,  // "C" for C ABI
        items: Vec<Decl>,
        span: Span,
    },
}

impl Decl {
    pub fn span(&self) -> Span {
        match self {
            Self::Function { span, .. } | Self::Struct { span, .. } |
            Self::Enum { span, .. } | Self::Trait { span, .. } |
            Self::Impl { span, .. } | Self::Actor { span, .. } |
            Self::TypeAlias { span, .. } | Self::Import { span, .. } |
            Self::Extern { span, .. } => *span,
        }
    }
}

/// A complete Kryos source module.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub name: String,
    pub declarations: Vec<Decl>,
    pub span: Span,
}
```

**Step 5: Define visitor trait**

```rust
// compiler/crates/kryos-ast/src/visitor.rs
use crate::types::TypeExpr;
use crate::expr::Expr;
use crate::stmt::Stmt;
use crate::decl::{Decl, Module};

/// Visitor trait for walking the AST.
pub trait AstVisitor {
    fn visit_module(&mut self, module: &Module) {
        for decl in &module.declarations {
            self.visit_decl(decl);
        }
    }

    fn visit_decl(&mut self, _decl: &Decl) {}
    fn visit_stmt(&mut self, _stmt: &Stmt) {}
    fn visit_expr(&mut self, _expr: &Expr) {}
    fn visit_type(&mut self, _ty: &TypeExpr) {}
}

/// Mutable visitor trait for transforming the AST.
pub trait AstMutVisitor {
    fn visit_module(&mut self, module: &mut Module) {
        for decl in &mut module.declarations {
            self.visit_decl(decl);
        }
    }

    fn visit_decl(&mut self, _decl: &mut Decl) {}
    fn visit_stmt(&mut self, _stmt: &mut Stmt) {}
    fn visit_expr(&mut self, _expr: &mut Expr) {}
    fn visit_type(&mut self, _ty: &mut TypeExpr) {}
}
```

**Step 6: Wire up lib.rs**

```rust
// compiler/crates/kryos-ast/src/lib.rs
pub mod types;
pub mod expr;
pub mod stmt;
pub mod decl;
pub mod visitor;

pub use kryos_errors::Span;
pub use types::*;
pub use expr::*;
pub use stmt::*;
pub use decl::*;
pub use visitor::*;
```

**Step 7: Verify compilation**

Run: `cd compiler && cargo check -p kryos-ast`
Expected: compiles with 0 errors

**Step 8: Commit**

```bash
git add compiler/crates/kryos-ast/
git commit -m "feat(ast): complete AST node definitions — types, exprs, stmts, decls, visitor"
```

---

### Task 6: Parser (kryos-parser)

Recursive descent + Pratt parser ported from Python.

**Files:**
- Create: `compiler/crates/kryos-parser/Cargo.toml`
- Create: `compiler/crates/kryos-parser/src/lib.rs`
- Create: `compiler/crates/kryos-parser/src/parser.rs`
- Create: `compiler/crates/kryos-parser/tests/parser.rs`

**Step 1: Create crate**

```toml
# compiler/crates/kryos-parser/Cargo.toml
[package]
name = "kryos-parser"
version.workspace = true
edition.workspace = true

[dependencies]
kryos-errors.workspace = true
kryos-ast.workspace = true
kryos-lexer.workspace = true

[dev-dependencies]
```

**Step 2: Write parser tests first**

```rust
// compiler/crates/kryos-parser/tests/parser.rs
use kryos_lexer::Lexer;
use kryos_parser::parse;
use kryos_ast::*;

fn parse_str(src: &str) -> Module {
    let tokens = Lexer::new(src, 0).tokenize();
    parse(tokens).expect("parse failed")
}

#[test]
fn parse_let_binding() {
    let module = parse_str("let x = 42");
    assert_eq!(module.declarations.len(), 0);
    // Top-level let is treated as a statement in a module — check the parser handles it
}

#[test]
fn parse_function() {
    let module = parse_str("fn add(a: i32, b: i32) -> i32 { return a + b }");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Function { name, params, ret_ty, .. } => {
            assert_eq!(name, "add");
            assert_eq!(params.len(), 2);
            assert!(ret_ty.is_some());
        }
        _ => panic!("expected function declaration"),
    }
}

#[test]
fn parse_struct() {
    let module = parse_str("struct Point { x: f64, y: f64 }");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Struct { name, fields, .. } => {
            assert_eq!(name, "Point");
            assert_eq!(fields.len(), 2);
        }
        _ => panic!("expected struct declaration"),
    }
}

#[test]
fn parse_enum() {
    let module = parse_str("enum Color { Red, Green, Blue }");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Enum { name, variants, .. } => {
            assert_eq!(name, "Color");
            assert_eq!(variants.len(), 3);
        }
        _ => panic!("expected enum declaration"),
    }
}

#[test]
fn parse_trait() {
    let module = parse_str("trait Display { fn to_string(self) -> str }");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Trait { name, methods, .. } => {
            assert_eq!(name, "Display");
            assert_eq!(methods.len(), 1);
        }
        _ => panic!("expected trait declaration"),
    }
}

#[test]
fn parse_impl_block() {
    let module = parse_str("impl Point { fn new(x: f64, y: f64) -> Point { return Point { x: x, y: y } } }");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Impl { target, methods, .. } => {
            assert_eq!(target, "Point");
            assert_eq!(methods.len(), 1);
        }
        _ => panic!("expected impl block"),
    }
}

#[test]
fn parse_if_else() {
    let module = parse_str("fn test() { if x > 0 { return 1 } else { return 0 } }");
    assert_eq!(module.declarations.len(), 1);
}

#[test]
fn parse_for_loop() {
    let module = parse_str("fn test() { for i in 0..10 { println(i) } }");
    assert_eq!(module.declarations.len(), 1);
}

#[test]
fn parse_match() {
    let module = parse_str("fn test(c: Color) -> str { match c { Color::Red => \"red\", Color::Blue => \"blue\" } }");
    assert_eq!(module.declarations.len(), 1);
}

#[test]
fn parse_use_import() {
    let module = parse_str("use std::io");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Import { path, .. } => {
            assert_eq!(path.segments, vec!["std", "io"]);
        }
        _ => panic!("expected import"),
    }
}

#[test]
fn parse_annotation() {
    let module = parse_str("@export\nfn main() {}");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Function { annotations, .. } => {
            assert_eq!(annotations.len(), 1);
            assert_eq!(annotations[0].name, "export");
        }
        _ => panic!("expected function"),
    }
}

#[test]
fn parse_generic_function() {
    let module = parse_str("fn identity<T>(x: T) -> T { return x }");
    match &module.declarations[0] {
        Decl::Function { generics, .. } => {
            assert_eq!(generics.len(), 1);
            assert_eq!(generics[0].name, "T");
        }
        _ => panic!("expected function"),
    }
}

#[test]
fn parse_actor() {
    let module = parse_str("actor Counter { count: i32 }");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Actor { name, .. } => {
            assert_eq!(name, "Counter");
        }
        _ => panic!("expected actor"),
    }
}

#[test]
fn parse_extern_block() {
    let module = parse_str("extern \"C\" { fn puts(s: *u8) -> i32 }");
    assert_eq!(module.declarations.len(), 1);
    match &module.declarations[0] {
        Decl::Extern { abi, items, .. } => {
            assert_eq!(abi, "C");
            assert_eq!(items.len(), 1);
        }
        _ => panic!("expected extern block"),
    }
}

#[test]
fn parse_shared_expr() {
    let module = parse_str("fn test() { let x = shared vec![1, 2, 3] }");
    assert_eq!(module.declarations.len(), 1);
}

#[test]
fn parse_select_stmt() {
    let module = parse_str("fn test() { select { recv ch1 => { println(msg) } } }");
    assert_eq!(module.declarations.len(), 1);
}

#[test]
fn parse_try_catch() {
    let module = parse_str("fn test() { try { risky() } catch e { println(e) } }");
    assert_eq!(module.declarations.len(), 1);
}
```

**Step 3: Implement the parser**

Create `compiler/crates/kryos-parser/src/parser.rs`. This is the largest file — a full recursive descent parser with Pratt expression parsing. The implementation must handle:

- All declaration types (fn, struct, enum, trait, impl, actor, type alias, use, extern)
- All statement types (let, assign, return, if/elif/else, for, while, break, continue, spawn, select, try/catch, throw)
- All expression types (literals, binary/unary ops, calls, method calls, field/index access, array/map/struct literals, lambdas, if/match exprs, range, pipe, shared/move/weak, comptime, quantum, cast)
- Pratt parsing for operator precedence
- Annotation parsing (@export, @capabilities(...), etc.)
- Generic parameter parsing (<T: Bound>)
- Error recovery (sync to next statement on error)
- Span tracking on every node

The parser reads from a `Vec<Token>` and produces an `ast::Module`.

```rust
// compiler/crates/kryos-parser/src/parser.rs
// [Full implementation — see Python parser.py for exact syntax rules]
// This file will be ~2000 lines.
```

**Note to implementer:** The full parser implementation is too large to include inline. Port the logic from `kryos/compiler/parser.py` method-by-method:
- `_parse_module()` → `parse_module()`
- `_parse_declaration()` → `parse_decl()`
- `_parse_fn_decl()` → `parse_fn_decl()`
- `_parse_struct_decl()` → `parse_struct_decl()`
- `_parse_expression()` with Pratt precedence → `parse_expr()` / `parse_expr_bp()`
- Each Python method maps 1:1 to a Rust method.

**Step 4: Wire up lib.rs**

```rust
// compiler/crates/kryos-parser/src/lib.rs
pub mod parser;
pub use parser::parse;
```

**Step 5: Run tests**

Run: `cd compiler && cargo test -p kryos-parser`
Expected: All 18 tests pass

**Step 6: Commit**

```bash
git add compiler/crates/kryos-parser/
git commit -m "feat(parser): recursive descent + Pratt parser — full Kryos syntax"
```

---

### Task 7: Type System (kryos-types)

Port from `kryos/compiler/types.py`.

**Files:**
- Create: `compiler/crates/kryos-types/Cargo.toml`
- Create: `compiler/crates/kryos-types/src/lib.rs`
- Create: `compiler/crates/kryos-types/src/ty.rs` — internal type representation
- Create: `compiler/crates/kryos-types/src/infer.rs` — Hindley-Milner type inference
- Create: `compiler/crates/kryos-types/src/check.rs` — type checking pass
- Create: `compiler/crates/kryos-types/tests/types.rs`

**Key features:**
- Internal `Type` enum (distinct from AST `TypeExpr`)
- Substitution map for type variables
- Unification algorithm
- Numeric literal inference (integer literals default to i32, float to f64)
- Monomorphization of generics
- Trait bound checking
- Type checking pass that walks the AST and produces a typed AST (annotations on each node)

**Tests to write first:**
- Infer `let x = 42` → x: i32
- Infer `let x = 3.14` → x: f64
- Infer `let x = "hello"` → x: str
- Type check `fn add(a: i32, b: i32) -> i32 { return a + b }` → valid
- Type error: `fn bad() -> i32 { return "hello" }` → error
- Generic instantiation: `fn id<T>(x: T) -> T` called with i32 → T = i32
- Trait bound: `fn sort<T: Ord>(items: [T])` — T must impl Ord
- ARC type: `shared x` → Shared<typeof(x)>

**Commit:** `feat(types): type system — inference, checking, generics, ARC types`

---

### Task 8: Ownership Analysis + ARC Insertion (kryos-ownership)

Port from `kryos/compiler/ownership.py`, extended with ARC.

**Files:**
- Create: `compiler/crates/kryos-ownership/Cargo.toml`
- Create: `compiler/crates/kryos-ownership/src/lib.rs`
- Create: `compiler/crates/kryos-ownership/src/analysis.rs`
- Create: `compiler/crates/kryos-ownership/src/arc.rs`
- Create: `compiler/crates/kryos-ownership/tests/ownership.rs`

**Key features:**
- Track ownership state per variable: Owned, Moved, Shared (ARC), Borrowed, BorrowedMut
- Detect use-after-move
- Detect double mutable borrow
- Identify values that need ARC wrapping:
  - Explicit `shared` keyword
  - Closures capturing mutable variables
  - Values sent to multiple actors
- Insert ARC retain/release annotations
- Copy types bypass ownership (primitives, `@copy` structs)
- Cycle detection for shared references

**Tests to write first:**
- Use-after-move detected
- Shared wrapping applied
- Copy types can be used after assignment
- Closure captures flagged for ARC
- Cycle detection triggers error

**Commit:** `feat(ownership): ownership analysis + ARC insertion — move tracking, shared detection`

---

### Task 9: Capability System (kryos-capabilities)

Port from `kryos/compiler/capabilities.py`.

**Files:**
- Create: `compiler/crates/kryos-capabilities/Cargo.toml`
- Create: `compiler/crates/kryos-capabilities/src/lib.rs`
- Create: `compiler/crates/kryos-capabilities/tests/capabilities.rs`

**Key features:**
- Parse `@capabilities(net, io, ffi)` annotations
- Validate capability usage — net operations require `net` capability
- Attenuation — child scopes cannot exceed parent capabilities
- Immutability at runtime — capabilities cannot be modified after compile time
- Budget/sandbox annotation checking

**Tests:**
- Function using net without capability → error
- Attenuation: child exceeds parent → error
- Valid capability chain → pass
- Self-heal cannot escalate capabilities

**Commit:** `feat(capabilities): compile-time capability checking — attenuation, immutability`

---

### Task 10: MIR (Mid-Level IR) (kryos-mir)

**Files:**
- Create: `compiler/crates/kryos-mir/Cargo.toml`
- Create: `compiler/crates/kryos-mir/src/lib.rs`
- Create: `compiler/crates/kryos-mir/src/ir.rs` — MIR data structures
- Create: `compiler/crates/kryos-mir/src/lower.rs` — AST → MIR lowering
- Create: `compiler/crates/kryos-mir/tests/mir.rs`

**Key features:**
- Control flow graph representation (basic blocks, terminators)
- Explicit drop points
- Explicit ARC retain/release operations
- Desugared — no syntactic sugar, no implicit conversions
- SSA-like variable numbering
- MIR instructions: Assign, Load, Store, Call, Return, Branch, Switch, Drop, ArcRetain, ArcRelease, Alloc, Dealloc

**Tests:**
- Simple function lowers to basic blocks
- If/else produces branch + phi
- Loop produces back-edge
- ARC retain/release inserted at correct points
- Drop calls at scope exits

**Commit:** `feat(mir): mid-level IR — CFG, explicit drops, ARC ops, SSA lowering`

---

### Task 11: Cranelift Backend (kryos-codegen-cranelift)

**Files:**
- Create: `compiler/crates/kryos-codegen-cranelift/Cargo.toml`
- Create: `compiler/crates/kryos-codegen-cranelift/src/lib.rs`
- Create: `compiler/crates/kryos-codegen-cranelift/src/codegen.rs`
- Create: `compiler/crates/kryos-codegen-cranelift/src/jit.rs` — JIT for `kryos run` and REPL
- Create: `compiler/crates/kryos-codegen-cranelift/tests/codegen.rs`

**Dependencies:**
```toml
[dependencies]
kryos-mir.workspace = true
kryos-errors.workspace = true
cranelift-codegen.workspace = true
cranelift-frontend.workspace = true
cranelift-jit.workspace = true
cranelift-module.workspace = true
cranelift-native.workspace = true
```

**Key features:**
- Lower MIR to Cranelift IR
- AOT compilation: MIR → Cranelift → object file
- JIT compilation: MIR → Cranelift → executable memory → function pointer
- Handle ARC runtime calls (extern functions)
- Handle channel/actor runtime calls
- C ABI function calls (for FFI)
- Support x86_64 and aarch64

**Tests:**
- Compile `fn add(a: i32, b: i32) -> i32 { return a + b }` → JIT execute → returns correct value
- Compile function with if/else → correct branch
- Compile function with loop → correct iteration
- ARC alloc/retain/release calls generated
- Float arithmetic correct
- String operations via runtime calls

**Commit:** `feat(codegen-cranelift): Cranelift backend — AOT + JIT, ARC runtime, C ABI`

---

### Task 12: LLVM Backend (kryos-codegen-llvm)

**Files:**
- Create: `compiler/crates/kryos-codegen-llvm/Cargo.toml`
- Create: `compiler/crates/kryos-codegen-llvm/src/lib.rs`
- Create: `compiler/crates/kryos-codegen-llvm/src/codegen.rs`
- Create: `compiler/crates/kryos-codegen-llvm/tests/codegen.rs`

**Dependencies:**
```toml
[dependencies]
kryos-mir.workspace = true
kryos-errors.workspace = true
inkwell.workspace = true
```

**Key features:**
- Lower MIR to LLVM IR via inkwell
- Optimization levels O0-O3
- LTO support
- WASM target (wasm32-unknown-unknown)
- Cross-compilation target selection
- ARC runtime function declarations
- Debug info generation (DWARF)

**Tests:**
- Compile simple function → verify LLVM IR output
- O0 vs O3 produces different IR
- WASM target produces wasm32 triple
- ARC runtime calls present in IR

**Commit:** `feat(codegen-llvm): LLVM backend via inkwell — O0-O3, LTO, WASM, debug info`

---

### Task 13: Runtime Library (kryos-rt)

**Files:**
- Create: `compiler/crates/kryos-rt/Cargo.toml`
- Create: `compiler/crates/kryos-rt/src/lib.rs`
- Create: `compiler/crates/kryos-rt/src/arc.rs`
- Create: `compiler/crates/kryos-rt/src/panic.rs`
- Create: `compiler/crates/kryos-rt/src/channel.rs`
- Create: `compiler/crates/kryos-rt/src/actor.rs`
- Create: `compiler/crates/kryos-rt/tests/runtime.rs`

**Key features:**
- `kryos_arc_alloc(size, drop_fn) -> *mut u8` — allocate ARC object
- `kryos_arc_retain(ptr)` — atomic increment
- `kryos_arc_release(ptr)` — atomic decrement, drop + dealloc at zero
- `kryos_panic(msg_ptr, msg_len)` — panic with message, stack trace
- `kryos_chan_new() -> handle` — create MPMC channel
- `kryos_chan_send(handle, data_ptr, data_len)` — send data
- `kryos_chan_recv(handle) -> (data_ptr, data_len)` — receive data
- `kryos_chan_select(handles, count) -> index` — select on multiple channels
- All functions exported with `#[no_mangle] pub extern "C"` for linking

**Tests:**
- ARC alloc/retain/release cycle — verify drop called at refcount 0
- Channel send/recv roundtrip
- Select on multiple channels
- Panic produces readable message

**Commit:** `feat(rt): runtime library — ARC, channels, actors, panic handling`

---

### Task 14: Linker Integration (kryos-linker)

**Files:**
- Create: `compiler/crates/kryos-linker/Cargo.toml`
- Create: `compiler/crates/kryos-linker/src/lib.rs`
- Create: `compiler/crates/kryos-linker/tests/linker.rs`

**Key features:**
- Detect system linker (cc on Unix, link.exe on Windows, wasm-ld for WASM)
- Link object files + kryos-rt + kryos-stdlib-native into final binary
- Handle static/dynamic linking
- Cross-compilation target support

**Commit:** `feat(linker): system linker integration — Unix/Windows/WASM linking`

---

### Task 15: Native Stdlib Layer (kryos-stdlib-native)

**Files:**
- Create: `compiler/crates/kryos-stdlib-native/Cargo.toml`
- Create: `compiler/crates/kryos-stdlib-native/src/lib.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/io.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/net.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/crypto.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/process.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/term.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/datetime.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/fs.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/sync.rs`
- Create: `compiler/crates/kryos-stdlib-native/src/regex.rs`

**Key features:**
- All functions `#[no_mangle] pub extern "C"` for FFI from compiled Kryos
- io: `kryos_file_open`, `kryos_file_read`, `kryos_file_write`, `kryos_file_close`, `kryos_stdin_read`, `kryos_stdout_write`, `kryos_stderr_write`
- net: `kryos_tcp_connect`, `kryos_tcp_bind`, `kryos_tcp_listen`, `kryos_tcp_accept`, `kryos_tcp_send`, `kryos_tcp_recv`, `kryos_udp_bind`, `kryos_udp_send`, `kryos_udp_recv`, `kryos_dns_resolve`
- crypto: wraps `ring` — `kryos_sha256`, `kryos_sha512`, `kryos_hmac`, `kryos_aes_encrypt`, `kryos_aes_decrypt`, `kryos_random_bytes`
- process: `kryos_process_spawn`, `kryos_process_wait`, `kryos_env_get`, `kryos_env_set`
- term: wraps `crossterm` — `kryos_term_raw_mode`, `kryos_term_size`, `kryos_term_color`, `kryos_term_cursor`
- datetime: `kryos_time_now`, `kryos_time_format`, `kryos_time_parse`
- fs: `kryos_path_join`, `kryos_path_exists`, `kryos_dir_walk`, `kryos_temp_file`
- sync: `kryos_mutex_new`, `kryos_mutex_lock`, `kryos_mutex_unlock`, `kryos_rwlock_new`
- regex: wraps `regex` crate — `kryos_regex_new`, `kryos_regex_match`, `kryos_regex_find_all`

**Commit:** `feat(stdlib-native): syscall FFI layer — io, net, crypto, process, term, datetime, fs, sync, regex`

---

### Task 16: Compiler Driver (kryos-driver)

**Files:**
- Create: `compiler/crates/kryos-driver/Cargo.toml`
- Create: `compiler/crates/kryos-driver/src/lib.rs`
- Create: `compiler/crates/kryos-driver/tests/driver.rs`

**Key features:**
- Orchestrate full pipeline: source → lex → parse → type check → ownership → capabilities → MIR → codegen → link
- Read `kryos.toml` project config
- Resolve module imports (file paths)
- Select backend (Cranelift or LLVM) based on build mode
- Incremental compilation (hash files, skip unchanged)
- Parallel per-module compilation
- Error aggregation and reporting

**Commit:** `feat(driver): compiler driver — pipeline orchestration, incremental compilation`

---

### Task 17: CLI Frontend (kryos-cli)

**Files:**
- Create: `compiler/crates/kryos-cli/Cargo.toml`
- Create: `compiler/crates/kryos-cli/src/main.rs`
- Create: `compiler/crates/kryos-cli/src/commands/mod.rs`
- Create: `compiler/crates/kryos-cli/src/commands/build.rs`
- Create: `compiler/crates/kryos-cli/src/commands/run.rs`
- Create: `compiler/crates/kryos-cli/src/commands/repl.rs`
- Create: `compiler/crates/kryos-cli/src/commands/test.rs`
- Create: `compiler/crates/kryos-cli/src/commands/bench.rs`
- Create: `compiler/crates/kryos-cli/src/commands/bindgen.rs`
- Create: `compiler/crates/kryos-cli/src/commands/fmt.rs`
- Create: `compiler/crates/kryos-cli/src/commands/check.rs`
- Create: `compiler/crates/kryos-cli/src/commands/doc.rs`
- Create: `compiler/crates/kryos-cli/src/commands/pkg.rs`
- Create: `compiler/crates/kryos-cli/src/commands/lsp.rs`

**Commands:**
- `kryos build [--release] [--target <triple>] [file|dir]`
- `kryos run <file.kry> [-- args...]`
- `kryos repl`
- `kryos test [--filter <pattern>]`
- `kryos bench`
- `kryos bindgen <header.h> [-o bindings.kry]`
- `kryos fmt [files...]`
- `kryos check [file|dir]`
- `kryos doc [--open]`
- `kryos pkg init|add|remove|update|lock`
- `kryos lsp`
- `kryos version`

**Commit:** `feat(cli): kryos CLI — build, run, repl, test, bench, bindgen, fmt, check, doc, pkg, lsp`

---

### Task 18: Bindgen (kryos-bindgen)

**Files:**
- Create: `compiler/crates/kryos-bindgen/Cargo.toml`
- Create: `compiler/crates/kryos-bindgen/src/lib.rs`
- Create: `compiler/crates/kryos-bindgen/src/parser.rs` — C header parser (subset)
- Create: `compiler/crates/kryos-bindgen/src/generator.rs` — Kryos extern generator
- Create: `compiler/crates/kryos-bindgen/tests/bindgen.rs`

**Key features:**
- Parse C header subset: function declarations, struct definitions, enum constants, typedefs, #define constants
- Generate Kryos `extern "C" { ... }` blocks with correct type mappings
- Type mapping: int→i32, long→i64, char*→*u8, void*→*u8, size_t→usize, struct→@repr(C) struct

**Tests:**
- `int puts(const char* s)` → `extern "C" { fn puts(s: *u8) -> i32 }`
- `struct timeval { long tv_sec; long tv_usec; }` → Kryos struct
- `#define EXIT_SUCCESS 0` → `let EXIT_SUCCESS: i32 = 0`

**Commit:** `feat(bindgen): C header → Kryos extern declarations`

---

### Task 19: Package Manager (kryos-package)

**Files:**
- Create: `compiler/crates/kryos-package/Cargo.toml`
- Create: `compiler/crates/kryos-package/src/lib.rs`
- Create: `compiler/crates/kryos-package/src/manifest.rs` — kryos.toml parsing
- Create: `compiler/crates/kryos-package/src/resolve.rs` — dependency resolution
- Create: `compiler/crates/kryos-package/src/fetch.rs` — git-based fetching
- Create: `compiler/crates/kryos-package/tests/package.rs`

**Key features:**
- Parse `kryos.toml` manifest (name, version, dependencies, capabilities)
- Git-based dependency fetching (`github:user/pkg@^1.0.0`)
- Semver resolution with ^, ~, = ranges
- Lock file generation (`kryos.lock`)
- Dependency tree validation
- `kryos pkg init` — create new project
- `kryos pkg add <dep>` — add dependency
- `kryos pkg update` — update dependencies
- `kryos pkg lock` — regenerate lock file

**Commit:** `feat(package): package manager — kryos.toml, git deps, semver resolution, lock file`

---

### Task 20: LSP Server (kryos-lsp)

**Files:**
- Create: `compiler/crates/kryos-lsp/Cargo.toml`
- Create: `compiler/crates/kryos-lsp/src/lib.rs`
- Create: `compiler/crates/kryos-lsp/src/server.rs`
- Create: `compiler/crates/kryos-lsp/src/diagnostics.rs`
- Create: `compiler/crates/kryos-lsp/src/completion.rs`
- Create: `compiler/crates/kryos-lsp/src/hover.rs`
- Create: `compiler/crates/kryos-lsp/src/goto_def.rs`

**Key features:**
- JSON-RPC over stdin/stdout
- Real-time diagnostics (errors/warnings as you type)
- Go-to-definition
- Find references
- Hover information (types, docs)
- Context-aware completion
- Rename symbol
- Format on save (delegates to kryos fmt)

**Commit:** `feat(lsp): language server — diagnostics, completion, hover, goto-def, rename`

---

## Phase 2: Kryos Standard Library

### Task 21: Pure Kryos Stdlib — Core Modules

Write in Kryos, compiled by the Rust bootstrap compiler.

**Files:**
- Create: `stdlib/std/math.kry`
- Create: `stdlib/std/string.kry`
- Create: `stdlib/std/fmt.kry`
- Create: `stdlib/std/iter.kry`
- Create: `stdlib/std/collections.kry`
- Create: `stdlib/std/map.kry`
- Create: `stdlib/std/set.kry`
- Create: `stdlib/std/test.kry`
- Create: `stdlib/std/chan.kry`
- Create: `stdlib/std/config.kry`

Each module written in Kryos with `@test` functions that validate correctness.

**Commit:** `feat(stdlib): pure Kryos stdlib — math, string, fmt, iter, collections, map, set, test, chan, config`

---

### Task 22: FFI-Backed Stdlib — System Modules

Kryos wrappers around kryos-stdlib-native.

**Files:**
- Create: `stdlib/std/io.kry`
- Create: `stdlib/std/net.kry`
- Create: `stdlib/std/crypto.kry`
- Create: `stdlib/std/process.kry`
- Create: `stdlib/std/term.kry`
- Create: `stdlib/std/datetime.kry`
- Create: `stdlib/std/fs.kry`
- Create: `stdlib/std/sync.kry`
- Create: `stdlib/std/regex.kry`
- Create: `stdlib/std/json.kry`
- Create: `stdlib/std/server.kry`
- Create: `stdlib/std/db.kry`

Each module declares `extern "C"` bindings to kryos-stdlib-native and wraps them in idiomatic Kryos APIs.

**Commit:** `feat(stdlib): FFI-backed stdlib — io, net, crypto, process, term, datetime, fs, sync, regex, json, server, db`

---

## Phase 3: Toolchain Completeness

### Task 23: Formatter (kryos fmt)

**Files:**
- Create: `compiler/crates/kryos-cli/src/commands/fmt_impl.rs`

Opinionated, zero-config formatter. Re-parses source, pretty-prints with canonical style. Handles indentation, line length, trailing commas, brace positioning.

**Commit:** `feat(fmt): opinionated code formatter — zero-config canonical style`

---

### Task 24: Documentation Generator (kryos doc)

**Files:**
- Create: `compiler/crates/kryos-cli/src/commands/doc_impl.rs`

Extracts `///` doc comments from AST, generates HTML documentation with type signatures, examples, cross-references.

**Commit:** `feat(doc): documentation generator — HTML output from doc comments`

---

### Task 25: Test Runner (kryos test)

**Files:**
- Create: `compiler/crates/kryos-cli/src/commands/test_impl.rs`

Finds all `@test` annotated functions, compiles and runs them, reports results with pass/fail/skip counts and timing.

**Commit:** `feat(test): built-in test runner — @test annotation, filtering, timing`

---

### Task 26: CI + Integration Test Suite

**Files:**
- Create: `.github/workflows/compiler-ci.yml`
- Create: `tests/integration/` — integration test programs

Full CI pipeline: build compiler, run unit tests, run integration tests, cross-compile check.

**Commit:** `feat(ci): compiler CI — unit tests, integration tests, cross-compile verification`

---

## Phase 4: Self-Hosting

### Task 27: Kryos Compiler in Kryos — Lexer

**Files:**
- Create: `self-host/compiler/lexer.kry`

Rewrite kryos-lexer in Kryos. Same logic, same tests. Compiled by the Rust bootstrap compiler.

**Commit:** `feat(self-host): lexer rewritten in Kryos`

---

### Task 28: Kryos Compiler in Kryos — Parser

**Files:**
- Create: `self-host/compiler/parser.kry`
- Create: `self-host/compiler/ast.kry`

**Commit:** `feat(self-host): parser + AST rewritten in Kryos`

---

### Task 29: Kryos Compiler in Kryos — Type System + Ownership

**Files:**
- Create: `self-host/compiler/types.kry`
- Create: `self-host/compiler/ownership.kry`
- Create: `self-host/compiler/capabilities.kry`

**Commit:** `feat(self-host): type system + ownership + capabilities rewritten in Kryos`

---

### Task 30: Kryos Compiler in Kryos — MIR + Codegen

**Files:**
- Create: `self-host/compiler/mir.kry`
- Create: `self-host/compiler/codegen_cranelift.kry`
- Create: `self-host/compiler/codegen_llvm.kry`

**Commit:** `feat(self-host): MIR + codegen backends rewritten in Kryos`

---

### Task 31: Kryos Compiler in Kryos — Driver + CLI

**Files:**
- Create: `self-host/compiler/driver.kry`
- Create: `self-host/compiler/cli.kry`

**Commit:** `feat(self-host): driver + CLI rewritten in Kryos`

---

### Task 32: Self-Host Validation

1. Compile the self-hosted compiler using the Rust bootstrap
2. Run the self-hosted compiler on itself (self-compilation)
3. Run the full test suite using the self-compiled compiler
4. Compare outputs of Rust-compiled vs self-compiled test programs
5. Freeze the Rust compiler as bootstrap binary

**Commit:** `feat(self-host): Kryos compiler compiles itself — self-hosting achieved`

---

### Task 33: Python Removal

**Files:**
- Delete: `kryos/` (entire Python compiler)
- Delete: `kryos_cli.py`
- Delete: `setup.py`
- Delete: all Python test files
- Modify: `.github/workflows/ci.yml` — remove Python jobs
- Modify: `README.md` — update installation instructions

**Commit:** `feat: remove Python bootstrap — Kryos is now fully self-hosted`

---

## Summary

| Phase | Tasks | Description |
|-------|-------|-------------|
| 1: Compiler Foundation | 1-20 | Full Rust compiler with dual backend, all analysis passes, full toolchain |
| 2: Standard Library | 21-22 | 22 stdlib modules in Kryos + Rust FFI layer |
| 3: Toolchain | 23-26 | Formatter, doc gen, test runner, CI |
| 4: Self-Hosting | 27-33 | Rewrite compiler in Kryos, validate, remove Python |

**Total: 33 tasks**
