//! `kryos explain ERRXXXX` — print a long-form explanation for an error code.
//!
//! Mirrors `rustc --explain`. The explanation catalog lives in
//! `kryos-errors/src/explain.rs`.

use kryos_errors::explain;

/// Print the long-form explanation for `code`, or list all available codes
/// when `code == "--list"` / `"list"`.
pub fn execute(code: &str) -> Result<(), String> {
    let arg = code.trim();
    if arg.is_empty() {
        return Err(
            "no error code given. Usage: kryos explain E0100 (or `kryos explain --list`)"
                .to_string(),
        );
    }

    if arg == "--list" || arg == "-l" || arg == "list" {
        println!("Error codes with long-form explanations:");
        println!();
        for (code, summary) in explain::list() {
            println!("  {}  {}", code, summary);
        }
        println!();
        println!("Run `kryos explain <CODE>` to see the full article for one.");
        return Ok(());
    }

    match explain::explain(arg) {
        Some(text) => {
            print!("{}", text);
            if !text.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        None => Err(format!(
            "no long-form explanation for `{}`. Run `kryos explain --list` to see available codes.",
            arg
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_known_code() {
        assert!(execute("E0100").is_ok());
    }

    #[test]
    fn execute_known_code_lowercase() {
        assert!(execute("e0100").is_ok());
    }

    #[test]
    fn execute_unknown_code() {
        assert!(execute("E9999").is_err());
    }

    #[test]
    fn execute_empty() {
        assert!(execute("").is_err());
    }

    #[test]
    fn execute_list() {
        assert!(execute("--list").is_ok());
    }

    #[test]
    fn list_has_all_explained_codes() {
        // Every entry returned by list() must have an explain() body.
        for (code, _) in explain::list() {
            assert!(
                explain::explain(code).is_some(),
                "code {} in list() has no explain() body",
                code
            );
        }
    }
}
