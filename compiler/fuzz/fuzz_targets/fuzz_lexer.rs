#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    // Must not panic - just tokenize
    let lexer = kryos_lexer::Lexer::new(s, 0);
    let _ = lexer.tokenize();
});
