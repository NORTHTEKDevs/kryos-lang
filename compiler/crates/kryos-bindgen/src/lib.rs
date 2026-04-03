//! `kryos-bindgen` — generates Kryos `extern` declarations from C header files.
//!
//! This crate provides a minimal C header parser and a Kryos source text
//! generator. It handles the practical subset of C headers needed for FFI:
//! function declarations, structs, enums, typedefs, and simple `#define`
//! constants.
//!
//! # Example
//!
//! ```
//! use kryos_bindgen::generate_bindings;
//!
//! let header = r#"
//!     int puts(const char* s);
//!     struct timeval { long tv_sec; long tv_usec; };
//!     #define EXIT_SUCCESS 0
//! "#;
//!
//! let kryos_source = generate_bindings(header).unwrap();
//! assert!(kryos_source.contains("fn puts(s: *u8) -> i32"));
//! ```

pub mod c_parser;
pub mod generator;

pub use c_parser::{parse_c_header, CField, CItem, CParam, CType};
pub use generator::{generate, map_type};

/// Parse a C header and generate Kryos source text with extern declarations.
///
/// Returns the generated Kryos source text on success, or a list of parse
/// error messages on failure.
pub fn generate_bindings(header_source: &str) -> Result<String, Vec<String>> {
    let items = c_parser::parse_c_header(header_source)?;
    Ok(generator::generate(&items))
}
