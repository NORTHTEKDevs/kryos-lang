use std::fmt;

use kryos_errors::Span;

/// Every token kind the Kryos lexer produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // --- Literals ---
    Integer,
    Float,
    String,
    StringPart,
    InterpStart,
    InterpEnd,
    Char,
    True,
    False,
    None,

    // --- Identifiers ---
    Ident,
    TypeIdent,

    // --- Keywords ---
    Let,
    Mut,
    Fn,
    Return,
    If,
    Else,
    Elif,
    For,
    While,
    Loop,
    In,
    Break,
    Continue,
    Struct,
    Enum,
    Impl,
    Trait,
    Dyn,
    Pub,
    Use,
    Extern,
    As,
    Mod,
    Type,
    Actor,
    Spawn,
    Select,
    Send,
    Recv,
    Chan,
    Parallel,
    Quantum,
    Comptime,
    Unsafe,
    Match,
    And,
    Or,
    Not,
    Try,
    Catch,
    Throw,
    Async,
    Await,
    Shared,
    Weak,
    Move,

    // --- Operators ---
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Power,
    At,
    EqEq,
    BangEq,
    Bang,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    Amp,
    AmpAmp,
    Pipe,
    PipePipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    Hash,
    Question,

    // --- Punctuation ---
    Arrow,
    FatArrow,
    ColonColon,
    DotDot,
    DotDotEq,
    Dot,
    Colon,
    Semicolon,
    Comma,

    // --- Grouping ---
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    // --- Special ---
    DocComment,
    Newline,
    Eof,
    Error,
}

impl TokenKind {
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::Let
                | Self::Mut
                | Self::Fn
                | Self::Return
                | Self::If
                | Self::Else
                | Self::Elif
                | Self::For
                | Self::While
                | Self::Loop
                | Self::In
                | Self::Break
                | Self::Continue
                | Self::Struct
                | Self::Enum
                | Self::Impl
                | Self::Trait
                | Self::Dyn
                | Self::Pub
                | Self::Use
                | Self::Extern
                | Self::As
                | Self::Mod
                | Self::Type
                | Self::Actor
                | Self::Spawn
                | Self::Select
                | Self::Send
                | Self::Recv
                | Self::Chan
                | Self::Parallel
                | Self::Quantum
                | Self::Comptime
                | Self::Unsafe
                | Self::Match
                | Self::And
                | Self::Or
                | Self::Not
                | Self::Try
                | Self::Catch
                | Self::Throw
                | Self::Async
                | Self::Await
                | Self::Shared
                | Self::Weak
                | Self::Move
                | Self::True
                | Self::False
                | Self::None
        )
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            // Literals
            Self::Integer => "integer literal",
            Self::Float => "float literal",
            Self::String => "string literal",
            Self::StringPart => "string interpolation part",
            Self::InterpStart => "interpolation start",
            Self::InterpEnd => "interpolation end",
            Self::Char => "character literal",
            Self::True => "'true'",
            Self::False => "'false'",
            Self::None => "'none'",

            // Identifiers
            Self::Ident => "identifier",
            Self::TypeIdent => "type identifier",

            // Keywords
            Self::Let => "'let'",
            Self::Mut => "'mut'",
            Self::Fn => "'fn'",
            Self::Return => "'return'",
            Self::If => "'if'",
            Self::Else => "'else'",
            Self::Elif => "'elif'",
            Self::For => "'for'",
            Self::While => "'while'",
            Self::Loop => "'loop'",
            Self::In => "'in'",
            Self::Break => "'break'",
            Self::Continue => "'continue'",
            Self::Struct => "'struct'",
            Self::Enum => "'enum'",
            Self::Impl => "'impl'",
            Self::Trait => "'trait'",
            Self::Dyn => "'dyn'",
            Self::Pub => "'pub'",
            Self::Use => "'use'",
            Self::Extern => "'extern'",
            Self::As => "'as'",
            Self::Mod => "'mod'",
            Self::Type => "'type'",
            Self::Actor => "'actor'",
            Self::Spawn => "'spawn'",
            Self::Select => "'select'",
            Self::Send => "'send'",
            Self::Recv => "'recv'",
            Self::Chan => "'chan'",
            Self::Parallel => "'parallel'",
            Self::Quantum => "'quantum'",
            Self::Comptime => "'comptime'",
            Self::Unsafe => "'unsafe'",
            Self::Match => "'match'",
            Self::And => "'and'",
            Self::Or => "'or'",
            Self::Not => "'not'",
            Self::Try => "'try'",
            Self::Catch => "'catch'",
            Self::Throw => "'throw'",
            Self::Async => "'async'",
            Self::Await => "'await'",
            Self::Shared => "'shared'",
            Self::Weak => "'weak'",
            Self::Move => "'move'",

            // Operators
            Self::Plus => "'+'",
            Self::Minus => "'-'",
            Self::Star => "'*'",
            Self::Slash => "'/'",
            Self::Percent => "'%'",
            Self::Power => "'**'",
            Self::At => "'@'",
            Self::EqEq => "'=='",
            Self::BangEq => "'!='",
            Self::Bang => "'!'",
            Self::Lt => "'<'",
            Self::Gt => "'>'",
            Self::LtEq => "'<='",
            Self::GtEq => "'>='",
            Self::Eq => "'='",
            Self::PlusEq => "'+='",
            Self::MinusEq => "'-='",
            Self::StarEq => "'*='",
            Self::SlashEq => "'/='",
            Self::PercentEq => "'%='",
            Self::Amp => "'&'",
            Self::AmpAmp => "'&&'",
            Self::Pipe => "'|'",
            Self::PipePipe => "'||'",
            Self::Caret => "'^'",
            Self::Tilde => "'~'",
            Self::Shl => "'<<'",
            Self::Shr => "'>>'",
            Self::Hash => "'#'",
            Self::Question => "'?'",

            // Punctuation
            Self::Arrow => "'->'",
            Self::FatArrow => "'=>'",
            Self::ColonColon => "'::'",
            Self::DotDot => "'..'",
            Self::DotDotEq => "'..='",
            Self::Dot => "'.'",
            Self::Colon => "':'",
            Self::Semicolon => "';'",
            Self::Comma => "','",

            // Grouping
            Self::LParen => "'('",
            Self::RParen => "')'",
            Self::LBrace => "'{'",
            Self::RBrace => "'}'",
            Self::LBracket => "'['",
            Self::RBracket => "']'",

            // Special
            Self::DocComment => "doc comment",
            Self::Newline => "newline",
            Self::Eof => "end of file",
            Self::Error => "error",
        };
        f.write_str(s)
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
        Self {
            kind,
            span,
            text: text.into(),
        }
    }

    pub fn dummy(kind: TokenKind) -> Self {
        Self {
            kind,
            span: Span::DUMMY,
            text: String::new(),
        }
    }
}

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
        "loop" => TokenKind::Loop,
        "in" => TokenKind::In,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "impl" => TokenKind::Impl,
        "trait" => TokenKind::Trait,
        "dyn" => TokenKind::Dyn,
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
        "chan" => TokenKind::Chan,
        "parallel" => TokenKind::Parallel,
        "quantum" => TokenKind::Quantum,
        "comptime" => TokenKind::Comptime,
        "unsafe" => TokenKind::Unsafe,
        "match" => TokenKind::Match,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "try" => TokenKind::Try,
        "catch" => TokenKind::Catch,
        "throw" => TokenKind::Throw,
        "async" => TokenKind::Async,
        "await" => TokenKind::Await,
        "shared" => TokenKind::Shared,
        "weak" => TokenKind::Weak,
        "move" => TokenKind::Move,
        _ => return Option::None,
    })
}

pub fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "f32"
            | "f64"
            | "bool"
            | "str"
            | "char"
            | "usize"
            | "isize"
            | "Tensor"
            | "Vec"
            | "Map"
            | "Set"
            | "Option"
            | "Result"
            | "Secret"
            | "Qubit"
            | "Qureg"
    )
}

pub fn attribute_lookup(word: &str) -> bool {
    matches!(
        word,
        "capabilities"
            | "compute"
            | "export"
            | "layout"
            | "real_time"
            | "no_std"
            | "zero_copy"
            | "target"
            | "differentiable"
            | "test"
            | "bench"
            | "actor"
            | "budget"
            | "sandbox"
            | "repr"
            | "copy"
            | "allocator"
    )
}
