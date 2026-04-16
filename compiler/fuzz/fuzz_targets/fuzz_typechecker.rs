#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    // Lexer first to get token stream
    let lexer = kryos_lexer::Lexer::new(s, 0);
    let tokens = lexer.tokenize();
    // Parser must succeed to proceed to type checker
    let Ok(module) = kryos_parser::parse(tokens) else { return };
    // Type checker must not panic on valid-parse output
    let _ = kryos_types::type_check(&module);
});
