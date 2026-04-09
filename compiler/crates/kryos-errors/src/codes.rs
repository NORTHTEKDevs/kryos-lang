//! Standard error codes for Kryos compiler diagnostics.
//!
//! Codes are grouped by category:
//! - E00xx: Parse errors
//! - E01xx: Type errors
//! - E02xx: Resolution errors
//! - E03xx: Ownership errors

// Parse errors (E00xx)
pub const E0001: &str = "E0001"; // unexpected token
pub const E0002: &str = "E0002"; // expected identifier
pub const E0003: &str = "E0003"; // expected expression
pub const E0004: &str = "E0004"; // expected type

// Type errors (E01xx)
pub const E0100: &str = "E0100"; // type mismatch
pub const E0101: &str = "E0101"; // unknown type
pub const E0102: &str = "E0102"; // undefined variable
pub const E0103: &str = "E0103"; // unknown struct
pub const E0104: &str = "E0104"; // wrong number of arguments
pub const E0105: &str = "E0105"; // unknown trait
pub const E0106: &str = "E0106"; // no such field
pub const E0107: &str = "E0107"; // no such method
pub const E0108: &str = "E0108"; // missing fields in struct literal
pub const E0109: &str = "E0109"; // Self used outside of impl/trait

// Ownership errors (E03xx)
pub const E0300: &str = "E0300"; // use of moved value
pub const E0301: &str = "E0301"; // use of uninitialized value
pub const E0302: &str = "E0302"; // assignment to immutable variable
