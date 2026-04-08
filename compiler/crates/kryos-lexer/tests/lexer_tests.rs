use kryos_lexer::{Lexer, Token, TokenKind};

/// Helper: tokenize a string and return all tokens (including Eof).
fn lex(src: &str) -> Vec<Token> {
    Lexer::new(src, 0).tokenize()
}

/// Helper: collect just the (kind, text) pairs, excluding Eof.
fn lex_kinds(src: &str) -> Vec<(TokenKind, String)> {
    lex(src)
        .into_iter()
        .filter(|t| t.kind != TokenKind::Eof)
        .map(|t| (t.kind, t.text))
        .collect()
}

// ==========================================================================
// Basic tokens — keywords
// ==========================================================================

#[test]
fn keywords_are_recognized() {
    let keywords = vec![
        ("fn", TokenKind::Fn),
        ("let", TokenKind::Let),
        ("mut", TokenKind::Mut),
        ("struct", TokenKind::Struct),
        ("if", TokenKind::If),
        ("else", TokenKind::Else),
        ("elif", TokenKind::Elif),
        ("match", TokenKind::Match),
        ("return", TokenKind::Return),
        ("for", TokenKind::For),
        ("while", TokenKind::While),
        ("in", TokenKind::In),
        ("break", TokenKind::Break),
        ("continue", TokenKind::Continue),
        ("enum", TokenKind::Enum),
        ("impl", TokenKind::Impl),
        ("trait", TokenKind::Trait),
        ("pub", TokenKind::Pub),
        ("use", TokenKind::Use),
        ("as", TokenKind::As),
        ("mod", TokenKind::Mod),
        ("type", TokenKind::Type),
        ("true", TokenKind::True),
        ("false", TokenKind::False),
        ("none", TokenKind::None),
        ("and", TokenKind::And),
        ("or", TokenKind::Or),
        ("not", TokenKind::Not),
        ("try", TokenKind::Try),
        ("catch", TokenKind::Catch),
        ("throw", TokenKind::Throw),
        ("actor", TokenKind::Actor),
        ("spawn", TokenKind::Spawn),
        ("select", TokenKind::Select),
        ("send", TokenKind::Send),
        ("recv", TokenKind::Recv),
        ("parallel", TokenKind::Parallel),
        ("quantum", TokenKind::Quantum),
        ("comptime", TokenKind::Comptime),
        ("shared", TokenKind::Shared),
        ("weak", TokenKind::Weak),
        ("move", TokenKind::Move),
    ];

    for (src, expected_kind) in keywords {
        let tokens = lex_kinds(src);
        assert_eq!(
            tokens.len(),
            1,
            "Expected exactly 1 token for keyword '{}', got {:?}",
            src,
            tokens
        );
        assert_eq!(tokens[0].0, expected_kind, "Wrong kind for keyword '{}'", src);
        assert_eq!(tokens[0].1, src, "Wrong text for keyword '{}'", src);
    }
}

// ==========================================================================
// Basic tokens — operators
// ==========================================================================

#[test]
fn operators_single_and_compound() {
    let operators = vec![
        ("+", TokenKind::Plus),
        ("-", TokenKind::Minus),
        ("*", TokenKind::Star),
        ("/", TokenKind::Slash),
        ("%", TokenKind::Percent),
        ("**", TokenKind::Power),
        ("==", TokenKind::EqEq),
        ("!=", TokenKind::BangEq),
        ("<", TokenKind::Lt),
        (">", TokenKind::Gt),
        ("<=", TokenKind::LtEq),
        (">=", TokenKind::GtEq),
        ("=", TokenKind::Eq),
        ("+=", TokenKind::PlusEq),
        ("-=", TokenKind::MinusEq),
        ("*=", TokenKind::StarEq),
        ("/=", TokenKind::SlashEq),
        ("&", TokenKind::Amp),
        ("|", TokenKind::Pipe),
        ("^", TokenKind::Caret),
        ("~", TokenKind::Tilde),
        ("<<", TokenKind::Shl),
        (">>", TokenKind::Shr),
        ("@", TokenKind::At),
    ];

    for (src, expected_kind) in operators {
        let tokens = lex_kinds(src);
        assert_eq!(
            tokens.len(),
            1,
            "Expected exactly 1 token for operator '{}', got {:?}",
            src,
            tokens
        );
        assert_eq!(tokens[0].0, expected_kind, "Wrong kind for operator '{}'", src);
        assert_eq!(tokens[0].1, src, "Wrong text for operator '{}'", src);
    }
}

// ==========================================================================
// Basic tokens — punctuation
// ==========================================================================

#[test]
fn punctuation_tokens() {
    let puncts = vec![
        ("{", TokenKind::LBrace),
        ("}", TokenKind::RBrace),
        ("(", TokenKind::LParen),
        (")", TokenKind::RParen),
        ("[", TokenKind::LBracket),
        ("]", TokenKind::RBracket),
        (":", TokenKind::Colon),
        ("::", TokenKind::ColonColon),
        (";", TokenKind::Semicolon),
        (",", TokenKind::Comma),
        (".", TokenKind::Dot),
        ("..", TokenKind::DotDot),
        ("..=", TokenKind::DotDotEq),
        ("->", TokenKind::Arrow),
        ("=>", TokenKind::FatArrow),
    ];

    for (src, expected_kind) in puncts {
        let tokens = lex_kinds(src);
        assert_eq!(
            tokens.len(),
            1,
            "Expected exactly 1 token for punctuation '{}', got {:?}",
            src,
            tokens
        );
        assert_eq!(tokens[0].0, expected_kind, "Wrong kind for '{}'", src);
        assert_eq!(tokens[0].1, src, "Wrong text for '{}'", src);
    }
}

// ==========================================================================
// Basic tokens — identifiers
// ==========================================================================

#[test]
fn identifiers_and_type_idents() {
    // Regular identifiers
    let regular = vec!["foo", "bar_baz", "_private", "x1", "camelCase"];
    for src in regular {
        let tokens = lex_kinds(src);
        assert_eq!(tokens.len(), 1, "Expected 1 token for ident '{}'", src);
        assert_eq!(tokens[0].0, TokenKind::Ident, "'{}' should be Ident", src);
        assert_eq!(tokens[0].1, src);
    }

    // Builtin type identifiers
    let builtins = vec!["i32", "f64", "bool", "str", "Vec", "Option", "Result"];
    for src in builtins {
        let tokens = lex_kinds(src);
        assert_eq!(tokens.len(), 1, "Expected 1 token for type ident '{}'", src);
        assert_eq!(tokens[0].0, TokenKind::TypeIdent, "'{}' should be TypeIdent", src);
        assert_eq!(tokens[0].1, src);
    }
}

// ==========================================================================
// Basic tokens — empty input
// ==========================================================================

#[test]
fn empty_input_produces_only_eof() {
    let tokens = lex("");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Eof);
}

// ==========================================================================
// Numeric literals — integers
// ==========================================================================

#[test]
fn integer_literals() {
    let cases = vec![
        ("42", "42"),
        ("0", "0"),
        ("1234567890", "1234567890"),
    ];

    for (src, expected_text) in cases {
        let tokens = lex_kinds(src);
        assert_eq!(tokens.len(), 1, "Expected 1 token for '{}'", src);
        assert_eq!(tokens[0].0, TokenKind::Integer, "'{}' should be Integer", src);
        assert_eq!(tokens[0].1, expected_text);
    }
}

#[test]
fn float_literals() {
    let cases = vec![
        ("3.14", "3.14"),
        ("0.5", "0.5"),
        ("1.0", "1.0"),
        ("1e10", "1e10"),
        ("2.5e3", "2.5e3"),
        ("1E-5", "1E-5"),
    ];

    for (src, expected_text) in cases {
        let tokens = lex_kinds(src);
        assert_eq!(tokens.len(), 1, "Expected 1 token for '{}'", src);
        assert_eq!(tokens[0].0, TokenKind::Float, "'{}' should be Float", src);
        assert_eq!(tokens[0].1, expected_text);
    }
}

#[test]
fn large_integer_literal() {
    let src = "999999999999999999";
    let tokens = lex_kinds(src);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Integer);
    assert_eq!(tokens[0].1, src);
}

#[test]
fn hex_binary_octal_integers() {
    let cases = vec![
        ("0xFF", TokenKind::Integer, "0xFF"),
        ("0b1010", TokenKind::Integer, "0b1010"),
        ("0o77", TokenKind::Integer, "0o77"),
        ("0x1A_2B", TokenKind::Integer, "0x1A_2B"),
    ];

    for (src, expected_kind, expected_text) in cases {
        let tokens = lex_kinds(src);
        assert_eq!(tokens.len(), 1, "Expected 1 token for '{}'", src);
        assert_eq!(tokens[0].0, expected_kind, "Wrong kind for '{}'", src);
        assert_eq!(tokens[0].1, expected_text);
    }
}

#[test]
fn underscore_separators_in_numbers() {
    let tokens = lex_kinds("1_000_000");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Integer);
    assert_eq!(tokens[0].1, "1_000_000");

    let tokens = lex_kinds("3.14_15");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Float);
    assert_eq!(tokens[0].1, "3.14_15");
}

// ==========================================================================
// String handling
// ==========================================================================

#[test]
fn simple_string_literal() {
    let tokens = lex_kinds(r#""hello""#);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::String);
    assert_eq!(tokens[0].1, "hello");
}

#[test]
fn string_with_escape_sequences() {
    let tokens = lex_kinds(r#""line\nnew\ttab\\slash\"quote""#);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::String);
    assert_eq!(tokens[0].1, "line\nnew\ttab\\slash\"quote");
}

#[test]
fn string_interpolation() {
    // "hello {name}!" should produce:
    // StringPart("hello ") InterpStart("{") Ident("name") InterpEnd("}") StringPart("!")
    let tokens = lex_kinds(r#""hello {name}!""#);

    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.0).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::StringPart,  // "hello "
            TokenKind::InterpStart, // {
            TokenKind::Ident,       // name
            TokenKind::InterpEnd,   // }
            TokenKind::StringPart,  // "!"
        ],
        "Interpolation token sequence mismatch: {:?}",
        tokens
    );

    assert_eq!(tokens[0].1, "hello ");
    assert_eq!(tokens[2].1, "name");
    assert_eq!(tokens[4].1, "!");
}

#[test]
fn empty_string() {
    let tokens = lex_kinds(r#""""#);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::String);
    assert_eq!(tokens[0].1, "");
}

#[test]
fn multi_word_string() {
    let tokens = lex_kinds(r#""hello world foo bar""#);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::String);
    assert_eq!(tokens[0].1, "hello world foo bar");
}

// ==========================================================================
// Char literals
// ==========================================================================

#[test]
fn char_literal() {
    let tokens = lex_kinds("'a'");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Char);
    assert_eq!(tokens[0].1, "a");
}

#[test]
fn char_literal_escape() {
    let tokens = lex_kinds(r"'\n'");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Char);
    assert_eq!(tokens[0].1, "\n");
}

// ==========================================================================
// Comments
// ==========================================================================

#[test]
fn line_comments_are_skipped() {
    let tokens = lex_kinds("// this is a comment");
    assert_eq!(tokens.len(), 0, "Line-comment-only input should produce no tokens");
}

#[test]
fn comment_at_end_of_line() {
    let tokens = lex_kinds("let x // a variable");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].0, TokenKind::Let);
    assert_eq!(tokens[1].0, TokenKind::Ident);
    assert_eq!(tokens[1].1, "x");
}

#[test]
fn block_comment_skipped() {
    let tokens = lex_kinds("/* block comment */ let x");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].0, TokenKind::Let);
    assert_eq!(tokens[1].0, TokenKind::Ident);
    assert_eq!(tokens[1].1, "x");
}

#[test]
fn nested_block_comment() {
    let tokens = lex_kinds("/* outer /* inner */ still comment */ 42");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Integer);
    assert_eq!(tokens[0].1, "42");
}

// ==========================================================================
// Edge cases
// ==========================================================================

#[test]
fn unterminated_string_does_not_panic() {
    // The lexer should not crash on an unterminated string.
    // It produces whatever partial tokens it can.
    let tokens = lex(r#""unterminated"#);
    // Should still produce at least one token (the partial string) plus Eof.
    assert!(tokens.len() >= 2, "Expected at least string + Eof, got {:?}", tokens);
    let last = tokens.last().unwrap();
    assert_eq!(last.kind, TokenKind::Eof);
}

#[test]
fn unknown_character_produces_error_token() {
    let tokens = lex_kinds("$");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Error);
    assert_eq!(tokens[0].1, "$");
}

#[test]
fn multiple_tokens_on_one_line() {
    let tokens = lex_kinds("let x = 42");
    assert_eq!(tokens.len(), 4);
    assert_eq!(tokens[0], (TokenKind::Let, "let".to_string()));
    assert_eq!(tokens[1], (TokenKind::Ident, "x".to_string()));
    assert_eq!(tokens[2], (TokenKind::Eq, "=".to_string()));
    assert_eq!(tokens[3], (TokenKind::Integer, "42".to_string()));
}

#[test]
fn whitespace_is_not_tokenized() {
    let tokens = lex_kinds("   \t\n\r\n   ");
    assert_eq!(tokens.len(), 0, "Whitespace-only input should produce no tokens");
}

#[test]
fn ambiguous_operators_lex_correctly() {
    // >= should be a single GtEq, not Gt + Eq
    let tokens = lex_kinds(">=");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::GtEq);

    // == should be a single EqEq, not Eq + Eq
    let tokens = lex_kinds("==");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::EqEq);

    // > = (with space) should be Gt then Eq
    let tokens = lex_kinds("> =");
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].0, TokenKind::Gt);
    assert_eq!(tokens[1].0, TokenKind::Eq);

    // -> should be Arrow, not Minus + Gt
    let tokens = lex_kinds("->");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Arrow);

    // => should be FatArrow, not Eq + Gt
    let tokens = lex_kinds("=>");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::FatArrow);

    // .. should be DotDot, not Dot + Dot
    let tokens = lex_kinds("..");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::DotDot);

    // ..= should be DotDotEq, not DotDot + Eq
    let tokens = lex_kinds("..=");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::DotDotEq);

    // :: should be ColonColon, not Colon + Colon
    let tokens = lex_kinds("::");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::ColonColon);
}

// ==========================================================================
// Span correctness
// ==========================================================================

#[test]
fn spans_are_correct() {
    let tokens = lex("let x = 42");
    // "let" at 0..3
    assert_eq!(tokens[0].span.start, 0);
    assert_eq!(tokens[0].span.end, 3);
    // "x" at 4..5
    assert_eq!(tokens[1].span.start, 4);
    assert_eq!(tokens[1].span.end, 5);
    // "=" at 6..7
    assert_eq!(tokens[2].span.start, 6);
    assert_eq!(tokens[2].span.end, 7);
    // "42" at 8..10
    assert_eq!(tokens[3].span.start, 8);
    assert_eq!(tokens[3].span.end, 10);
}

// ==========================================================================
// Integration — small complete function
// ==========================================================================

#[test]
fn lex_complete_function() {
    let src = r#"fn add(a: i32, b: i32) -> i32 {
    return a + b
}"#;
    let tokens = lex_kinds(src);
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.0).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::Fn,          // fn
            TokenKind::Ident,       // add
            TokenKind::LParen,      // (
            TokenKind::Ident,       // a
            TokenKind::Colon,       // :
            TokenKind::TypeIdent,   // i32
            TokenKind::Comma,       // ,
            TokenKind::Ident,       // b
            TokenKind::Colon,       // :
            TokenKind::TypeIdent,   // i32
            TokenKind::RParen,      // )
            TokenKind::Arrow,       // ->
            TokenKind::TypeIdent,   // i32
            TokenKind::LBrace,      // {
            TokenKind::Return,      // return
            TokenKind::Ident,       // a
            TokenKind::Plus,        // +
            TokenKind::Ident,       // b
            TokenKind::RBrace,      // }
        ]
    );
}

// ==========================================================================
// Integration — struct definition
// ==========================================================================

#[test]
fn lex_struct_definition() {
    let src = r#"pub struct Point {
    x: f64,
    y: f64,
}"#;
    let tokens = lex_kinds(src);
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.0).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::Pub,        // pub
            TokenKind::Struct,     // struct
            TokenKind::Ident,      // Point
            TokenKind::LBrace,     // {
            TokenKind::Ident,      // x
            TokenKind::Colon,      // :
            TokenKind::TypeIdent,  // f64
            TokenKind::Comma,      // ,
            TokenKind::Ident,      // y
            TokenKind::Colon,      // :
            TokenKind::TypeIdent,  // f64
            TokenKind::Comma,      // ,
            TokenKind::RBrace,     // }
        ]
    );
}

// ==========================================================================
// Integration — match expression
// ==========================================================================

#[test]
fn lex_match_expression() {
    let src = r#"match x {
    0 => "zero",
    _ => "other",
}"#;
    let tokens = lex_kinds(src);
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.0).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::Match,      // match
            TokenKind::Ident,      // x
            TokenKind::LBrace,     // {
            TokenKind::Integer,    // 0
            TokenKind::FatArrow,   // =>
            TokenKind::String,     // "zero"
            TokenKind::Comma,      // ,
            TokenKind::Ident,      // _
            TokenKind::FatArrow,   // =>
            TokenKind::String,     // "other"
            TokenKind::Comma,      // ,
            TokenKind::RBrace,     // }
        ]
    );
}

// ==========================================================================
// Bang alone is an error token
// ==========================================================================

#[test]
fn bang_alone_is_error() {
    let tokens = lex_kinds("!");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, TokenKind::Error);
    assert_eq!(tokens[0].1, "!");
}

// ==========================================================================
// Multiple string interpolations
// ==========================================================================

#[test]
fn multiple_interpolations_in_one_string() {
    let tokens = lex_kinds(r#""x={a} y={b}""#);
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.0).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::StringPart,  // "x="
            TokenKind::InterpStart, // {
            TokenKind::Ident,       // a
            TokenKind::InterpEnd,   // }
            TokenKind::StringPart,  // " y="
            TokenKind::InterpStart, // {
            TokenKind::Ident,       // b
            TokenKind::InterpEnd,   // }
            TokenKind::StringPart,  // ""
        ]
    );
    assert_eq!(tokens[0].1, "x=");
    assert_eq!(tokens[2].1, "a");
    assert_eq!(tokens[4].1, " y=");
    assert_eq!(tokens[6].1, "b");
}

// ==========================================================================
// File ID is propagated
// ==========================================================================

#[test]
fn file_id_is_set_on_tokens() {
    let tokens = Lexer::new("let x", 7).tokenize();
    for token in &tokens {
        assert_eq!(token.span.file_id, 7, "file_id should be 7 for all tokens");
    }
}
