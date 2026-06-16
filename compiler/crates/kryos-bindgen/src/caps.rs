//! Capability inference for generated C bindings.
//!
//! `bindgen` / `@cImport` produce typed bindings but say nothing about what the
//! bound functions actually *do* -- every imported symbol is equally trusted.
//! This module adds a heuristic classifier over the parsed C declarations and
//! emits two reviewable artifacts:
//!
//! 1. A `bindings.caps.toml` mapping each extern function to a **suggested**
//!    capability plus a confidence note.
//! 2. A thin Kryos wrapper-stub module that places the suggested
//!    `@capabilities(...)` annotation on each wrapper fn.
//!
//! Every tag produced here is a **GUESS**. It is framed as
//! *suggested -> human-confirmed -> compiler-enforced*, NEVER authoritative: a
//! wrong tag is worse than no tag. Sound static analysis of the C side is
//! impossible from a header, so the developer reviews the toml, corrects any
//! misclassification via the `[overrides]` table (an override always wins over
//! the heuristic), and only then promotes the confirmed capability onto the
//! wrapper -- at which point the Kryos compiler enforces it transitively across
//! the call graph.

use std::collections::BTreeMap;

use crate::c_parser::{CItem, CParam};
use crate::generator::map_type;

/// A capability a bound function might require.
///
/// `compute` is the least-privileged default (pure math / string work plus the
/// fallback for anything the heuristic cannot place).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Net,
    Io,
    Process,
    Compute,
}

impl Capability {
    /// The lowercase token used in `@capabilities(...)` and in the toml.
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Net => "net",
            Capability::Io => "io",
            Capability::Process => "process",
            Capability::Compute => "compute",
        }
    }

    /// Parse a capability token (as written in the toml). Unknown tokens fail.
    pub fn parse(s: &str) -> Option<Capability> {
        match s.trim() {
            "net" => Some(Capability::Net),
            "io" => Some(Capability::Io),
            "process" => Some(Capability::Process),
            "compute" => Some(Capability::Compute),
            _ => None,
        }
    }
}

/// Where a capability tag came from, and therefore how much to trust it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Matched a known symbol / parameter naming pattern.
    Heuristic,
    /// No pattern matched; defaulted to the least-privileged `compute` guess.
    LowConfidence,
    /// Taken from a human-confirmed `[overrides]` entry in bindings.caps.toml.
    Override,
}

impl Provenance {
    /// A human-readable confidence / provenance note. Every heuristic note ends
    /// in "verify" -- nothing here is authoritative until a human confirms it.
    pub fn note(self) -> &'static str {
        match self {
            Provenance::Heuristic => "heuristic, verify",
            Provenance::LowConfidence => {
                "no naming pattern matched; defaulted to compute (low confidence), verify"
            }
            Provenance::Override => "overridden via bindings.caps.toml (human-confirmed)",
        }
    }
}

/// A suggested capability tag for a single extern function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapTag {
    pub symbol: String,
    pub capability: Capability,
    pub provenance: Provenance,
}

impl CapTag {
    /// The confidence / provenance note for this tag.
    pub fn note(&self) -> &'static str {
        self.provenance.note()
    }
}

/// Classify every function in a parsed header into a suggested capability tag.
///
/// Non-function items (structs, enums, typedefs, defines) carry no capability
/// and are skipped.
pub fn classify(items: &[CItem]) -> Vec<CapTag> {
    let mut tags = Vec::new();
    for item in items {
        if let CItem::Function { name, params, .. } = item {
            let (capability, provenance) = classify_symbol(name, params);
            tags.push(CapTag {
                symbol: name.clone(),
                capability,
                provenance,
            });
        }
    }
    tags
}

/// Classify one symbol from its name and (as a weak secondary signal) its
/// parameter names. Always a guess.
fn classify_symbol(name: &str, params: &[CParam]) -> (Capability, Provenance) {
    if let Some(cap) = match_name(name) {
        return (cap, Provenance::Heuristic);
    }
    // Secondary signal: a parameter named like a known pattern (e.g. `socket`).
    for p in params {
        if let Some(pname) = &p.name {
            if let Some(cap) = match_name(pname) {
                return (cap, Provenance::Heuristic);
            }
        }
    }
    // Unknown -- default to the least-privileged guess, flagged low-confidence.
    (Capability::Compute, Provenance::LowConfidence)
}

/// Match a single identifier against the capability naming-pattern table.
///
/// Patterns (checked in order net -> io -> process -> pure):
/// - net:     `socket` `bind` `listen` `accept` `send` `recv`, `*_open`
///            `*_connect` `*_socket`
/// - io:      `fopen` `fread` `fwrite` `open` `close` `read` `write`,
///            `*_read` `*_write`
/// - process: `system` `fork` `popen`, `*_exec` `*_spawn`
/// - compute: known-pure math / string functions (`strlen`, `strcmp`, `sin`,
///            `cos`, `abs`, `memcpy`, ...)
fn match_name(n: &str) -> Option<Capability> {
    if matches!(n, "socket" | "bind" | "listen" | "accept" | "send" | "recv")
        || n.ends_with("_open")
        || n.ends_with("_connect")
        || n.ends_with("_socket")
    {
        return Some(Capability::Net);
    }
    if matches!(
        n,
        "fopen" | "fread" | "fwrite" | "open" | "close" | "read" | "write"
    ) || n.ends_with("_read")
        || n.ends_with("_write")
    {
        return Some(Capability::Io);
    }
    if matches!(n, "system" | "fork" | "popen") || n.ends_with("_exec") || n.ends_with("_spawn") {
        return Some(Capability::Process);
    }
    if is_known_pure(n) {
        return Some(Capability::Compute);
    }
    None
}

/// Known-pure math / string functions that need no ambient authority.
fn is_known_pure(n: &str) -> bool {
    matches!(
        n,
        "strlen"
            | "strcmp"
            | "strncmp"
            | "strcpy"
            | "strncpy"
            | "strcat"
            | "strncat"
            | "strchr"
            | "strrchr"
            | "strstr"
            | "memcpy"
            | "memmove"
            | "memset"
            | "memcmp"
            | "memchr"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "sqrt"
            | "cbrt"
            | "pow"
            | "exp"
            | "log"
            | "log2"
            | "log10"
            | "abs"
            | "labs"
            | "fabs"
            | "floor"
            | "ceil"
            | "round"
            | "trunc"
            | "fmod"
    )
}

/// Apply human-confirmed overrides on top of heuristic tags. An override always
/// wins, and its provenance is updated to [`Provenance::Override`].
pub fn apply_overrides(tags: &mut [CapTag], overrides: &BTreeMap<String, Capability>) {
    for tag in tags.iter_mut() {
        if let Some(&cap) = overrides.get(&tag.symbol) {
            tag.capability = cap;
            tag.provenance = Provenance::Override;
        }
    }
}

/// Parse the `[overrides]` table out of a `bindings.caps.toml` file.
///
/// Only the `[overrides]` section is read; all other sections are ignored.
/// Entries with an unknown capability value are skipped. This is a deliberately
/// small, dependency-free reader for the narrow shape this crate emits.
pub fn parse_overrides(toml: &str) -> BTreeMap<String, Capability> {
    let mut map = BTreeMap::new();
    let mut in_overrides = false;
    for line in toml.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_overrides = line == "[overrides]";
            continue;
        }
        if !in_overrides {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = unquote(k);
            let val = unquote(v);
            if !key.is_empty() {
                if let Some(cap) = Capability::parse(&val) {
                    map.insert(key, cap);
                }
            }
        }
    }
    map
}

/// Strip a trailing `# ...` comment that is outside any quoted string.
fn strip_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Trim whitespace and an optional pair of surrounding double quotes.
fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('"').unwrap_or(s);
    let s = s.strip_suffix('"').unwrap_or(s);
    s.to_string()
}

const CAPS_TOML_HEADER: &str = "\
# ============================================================================
# bindings.caps.toml  --  SUGGESTED capability tags for generated C bindings
# ============================================================================
#
# Generated by `kryos bindgen`. Every tag below is a HEURISTIC GUESS inferred
# from C symbol / parameter naming patterns. It is NOT authoritative -- a wrong
# tag is worse than no tag.
#
# Workflow:
#   1. Review each [suggestions.*] entry below.
#   2. Correct any wrong guess by adding the symbol to the [overrides] table.
#   3. Promote the confirmed capability onto the wrapper fn's @capabilities(...)
#      annotation. The Kryos compiler then enforces it transitively across the
#      whole call graph -- human-confirmed, then compiler-enforced.
#
# Capabilities: net | io | process | compute
";

const CAPS_TOML_OVERRIDES_HEADER: &str = "
# ----------------------------------------------------------------------------
# Human-confirmed overrides. An entry here ALWAYS wins over the heuristic above.
# Format:  symbol = \"net\"      (one of: net | io | process | compute)
# ----------------------------------------------------------------------------
[overrides]
";

/// Emit a reviewable `bindings.caps.toml`.
///
/// The `[suggestions.*]` tables hold the raw heuristic guesses (passed in via
/// `tags`); the `[overrides]` table re-emits any human-confirmed overrides so
/// the file round-trips. Nothing here is authoritative.
pub fn generate_caps_toml(tags: &[CapTag], overrides: &BTreeMap<String, Capability>) -> String {
    let mut out = String::new();
    out.push_str(CAPS_TOML_HEADER);
    for tag in tags {
        out.push('\n');
        out.push_str(&format!("[suggestions.{}]\n", tag.symbol));
        out.push_str(&format!("capability = \"{}\"\n", tag.capability.as_str()));
        out.push_str(&format!("confidence = \"{}\"\n", tag.note()));
    }
    out.push_str(CAPS_TOML_OVERRIDES_HEADER);
    for (sym, cap) in overrides {
        out.push_str(&format!("{} = \"{}\"\n", sym, cap.as_str()));
    }
    out
}

const WRAPPER_HEADER: &str = "\
// Generated by kryos bindgen -- capability-annotated wrapper stubs.
//
// Each @capabilities(...) tag below is a SUGGESTED guess (heuristic, verify),
// NOT authoritative. Review bindings.caps.toml, confirm or override each tag,
// then implement the stub bodies. Once confirmed, the Kryos compiler enforces
// these capabilities transitively across the call graph.
";

/// Generate a thin Kryos wrapper module of capability-annotated stubs.
///
/// Each wrapper fn carries the suggested (or overridden) `@capabilities(...)`
/// tag for its symbol. Bodies are stubs only -- the developer fills in the real
/// call after confirming the capability. `tags` supplies the effective
/// capability for each function (run [`apply_overrides`] first if you want
/// overrides to take effect).
pub fn generate_wrappers(items: &[CItem], tags: &[CapTag]) -> String {
    let cap_of: BTreeMap<&str, &CapTag> = tags.iter().map(|t| (t.symbol.as_str(), t)).collect();

    let mut out = String::new();
    out.push_str(WRAPPER_HEADER);

    for item in items {
        if let CItem::Function {
            name,
            params,
            ret_ty,
            ..
        } = item
        {
            let tag = match cap_of.get(name.as_str()) {
                Some(t) => *t,
                None => continue,
            };
            out.push('\n');
            out.push_str(&format!(
                "// `{}` -- SUGGESTED @capabilities({}) ({})\n",
                name,
                tag.capability.as_str(),
                tag.note()
            ));
            out.push_str(&format!("@capabilities({})\n", tag.capability.as_str()));
            out.push_str(&format!("fn {}({})", name, wrapper_params(params)));
            let ret = map_type(ret_ty);
            if ret != "()" {
                out.push_str(&format!(" -> {ret}"));
            }
            out.push_str(" {\n");
            out.push_str("    // stub: confirm the suggested capability above, then call the\n");
            out.push_str("    // matching extern declaration from the generated bindings module.\n");
            out.push_str(&format!(
                "    panic(\"kryos-bindgen stub: {} wrapper is not implemented\")\n",
                name
            ));
            out.push_str("}\n");
        }
    }

    out
}

/// Build a wrapper parameter list, synthesizing names for unnamed C params.
fn wrapper_params(params: &[CParam]) -> String {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let ty = map_type(&p.ty);
            let name = match &p.name {
                Some(n) => n.clone(),
                None => format!("arg{i}"),
            };
            format!("{name}: {ty}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_net_patterns() {
        assert_eq!(match_name("socket"), Some(Capability::Net));
        assert_eq!(match_name("recv"), Some(Capability::Net));
        assert_eq!(match_name("tcp_connect"), Some(Capability::Net));
        assert_eq!(match_name("device_open"), Some(Capability::Net));
    }

    #[test]
    fn matches_io_patterns() {
        assert_eq!(match_name("fread"), Some(Capability::Io));
        assert_eq!(match_name("open"), Some(Capability::Io));
        assert_eq!(match_name("buffer_write"), Some(Capability::Io));
        // bare `open` is io; only the `_open` suffix is net.
        assert_eq!(match_name("fopen"), Some(Capability::Io));
    }

    #[test]
    fn matches_process_patterns() {
        assert_eq!(match_name("system"), Some(Capability::Process));
        assert_eq!(match_name("popen"), Some(Capability::Process));
        assert_eq!(match_name("proc_spawn"), Some(Capability::Process));
        assert_eq!(match_name("lua_exec"), Some(Capability::Process));
    }

    #[test]
    fn matches_pure_compute() {
        assert_eq!(match_name("strlen"), Some(Capability::Compute));
        assert_eq!(match_name("memcpy"), Some(Capability::Compute));
        assert_eq!(match_name("sin"), Some(Capability::Compute));
    }

    #[test]
    fn unknown_is_none() {
        assert_eq!(match_name("frobnicate"), None);
    }

    #[test]
    fn capability_round_trips() {
        for cap in [
            Capability::Net,
            Capability::Io,
            Capability::Process,
            Capability::Compute,
        ] {
            assert_eq!(Capability::parse(cap.as_str()), Some(cap));
        }
        assert_eq!(Capability::parse("bogus"), None);
    }
}
