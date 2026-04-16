#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    // Lexer first to get token stream
    let lexer = kryos_lexer::Lexer::new(s, 0);
    let tokens = lexer.tokenize();
    // Parser must not panic on any input - only return errors
    let _ = kryos_parser::parse(tokens);
});
