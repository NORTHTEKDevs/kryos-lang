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
pub mod caps;
pub mod generator;

pub use c_parser::{parse_c_header, CField, CItem, CParam, CType};
pub use caps::{
    apply_overrides, classify, generate_caps_toml, generate_wrappers, parse_overrides, CapTag,
    Capability, Provenance,
};
pub use generator::{generate, map_type};

/// Parse a C header and generate Kryos source text with extern declarations.
///
/// Returns the generated Kryos source text on success, or a list of parse
/// error messages on failure.
pub fn generate_bindings(header_source: &str) -> Result<String, Vec<String>> {
    let items = c_parser::parse_c_header(header_source)?;
    Ok(generator::generate(&items))
}

/// Capability-aware artifacts produced from a C header.
pub struct CapsBindings {
    /// One effective capability tag per extern function (overrides applied).
    pub tags: Vec<caps::CapTag>,
    /// Reviewable `bindings.caps.toml` text.
    pub caps_toml: String,
    /// Capability-annotated Kryos wrapper-stub module text.
    pub wrappers: String,
}

/// Parse a C header and produce capability-aware artifacts: the suggested
/// `bindings.caps.toml` text and the wrapper-stub module text.
///
/// If `existing_toml` is supplied, its `[overrides]` table is read back so a
/// human-confirmed override wins over the heuristic. Every tag remains a
/// suggestion until a human confirms it; this only proposes, never decides.
pub fn generate_caps_bindings(
    header_source: &str,
    existing_toml: Option<&str>,
) -> Result<CapsBindings, Vec<String>> {
    let items = c_parser::parse_c_header(header_source)?;
    let heuristic = caps::classify(&items);
    let overrides = existing_toml
        .map(caps::parse_overrides)
        .unwrap_or_default();

    let mut effective = heuristic.clone();
    caps::apply_overrides(&mut effective, &overrides);

    // Suggestions show the raw heuristic guess; overrides are kept separate so
    // the file round-trips and the "suggested vs human-confirmed" split is clear.
    let caps_toml = caps::generate_caps_toml(&heuristic, &overrides);
    let wrappers = caps::generate_wrappers(&items, &effective);

    Ok(CapsBindings {
        tags: effective,
        caps_toml,
        wrappers,
    })
}
