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
    Let, Mut, Fn, Return, If, Else, Elif,
    For, While, In, Break, Continue,
    Struct, Enum, Impl, Trait,
    Pub, Use, Extern, As, Mod, Type,
    Actor, Spawn, Select, Send, Recv, Ask, Chan,
    Parallel, Quantum, Comptime,
    Match, And, Or, Not,
    Try, Catch, Throw,
    Shared, Weak, Move,

    // --- Operators ---
    Plus, Minus, Star, Slash, Percent, Power,
    At,
    EqEq, BangEq, Lt, Gt, LtEq, GtEq,
    Eq, PlusEq, MinusEq, StarEq, SlashEq,
    Amp, Pipe, Caret, Tilde, Shl, Shr,

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
    LParen, RParen,
    LBrace, RBrace,
    LBracket, RBracket,

    // --- Special ---
    Newline,
    Eof,
    Error,
}

impl TokenKind {
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

pub fn is_builtin_type(name: &str) -> bool {
    matches!(name,
        "i8" | "i16" | "i32" | "i64" | "i128" |
        "u8" | "u16" | "u32" | "u64" | "u128" |
        "f32" | "f64" |
        "bool" | "str" | "char" | "usize" | "isize" |
        "Tensor" | "Vec" | "Map" | "Set" | "Option" | "Result" | "Secret" | "Qubit" | "Qureg"
    )
}

pub fn attribute_lookup(word: &str) -> bool {
    matches!(word,
        "capabilities" | "compute" | "export" | "layout" |
        "real_time" | "no_std" | "zero_copy" | "target" |
        "differentiable" | "test" | "bench" | "actor" |
        "budget" | "sandbox" | "repr" | "copy" | "allocator"
    )
}
