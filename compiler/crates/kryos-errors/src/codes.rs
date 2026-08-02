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
pub const E0009: &str = "E0009"; // syntax error (general / uncategorized)
pub const E0010: &str = "E0010"; // program nesting too deep / expression too complex

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
pub const E0110: &str = "E0110"; // type error (general / uncategorized)
pub const E0111: &str = "E0111"; // integer literal out of range for declared type
pub const E0112: &str = "E0112"; // non-exhaustive match

// Resolution errors (E02xx)
pub const E0200: &str = "E0200"; // module path / import could not be resolved
pub const E0201: &str = "E0201"; // qualified call (`Mod::fn`) refers to a name imported from a different module
pub const E0202: &str = "E0202"; // qualified call (`Mod::fn`) names a symbol never imported
pub const E0203: &str = "E0203"; // import of a private/internal module member
pub const E0204: &str = "E0204"; // module has no export by that name
pub const E0205: &str = "E0205"; // duplicate name imported from multiple modules

// Ownership errors (E03xx)
pub const E0300: &str = "E0300"; // use of moved value
pub const E0301: &str = "E0301"; // use of uninitialized value
pub const E0302: &str = "E0302"; // assignment to immutable variable
pub const E0303: &str = "E0303"; // use of partially moved value

// Ownership warnings (W03xx)
pub const W0300: &str = "W0300"; // conditional move (moved on one branch only)

// Provenance / lineage warnings (W04xx)
pub const W0400: &str = "W0400"; // tracked value discarded (provenance/lineage lost)

// Capability-scope warnings (W05xx)
pub const W0500: &str = "W0500"; // deny!() block names no recognized capability (no effect)

// Capability errors (E05xx)
pub const E0500: &str = "E0500"; // unsafe operation outside `unsafe` context
pub const E0501: &str = "E0501"; // capability import violation
pub const E0502: &str = "E0502"; // missing required capability
pub const E0503: &str = "E0503"; // capability attenuation violation
pub const E0504: &str = "E0504"; // capability escalation
pub const E0505: &str = "E0505"; // builtin capability violation
pub const E0506: &str = "E0506"; // FFI capability violation
pub const E0507: &str = "E0507"; // capability propagation violation
