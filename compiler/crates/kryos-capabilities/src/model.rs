//! Capability model — types and sets for compile-time capability enforcement.

use std::collections::HashSet;
use std::fmt;

use kryos_ast::Annotation;

/// All capabilities recognized by Kryos.
///
/// Capabilities are compile-time markers that restrict what a function, actor,
/// or scope is allowed to do. They cannot be widened at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Network access (TCP, UDP, DNS).
    Net,
    /// File I/O (read, write, seek, stat).
    Io,
    /// Foreign function interface — calling into C, Rust, etc.
    Ffi,
    /// Heavy computation (GPU dispatch, SIMD intrinsics).
    Compute,
    /// Cryptographic operations (hashing, signing, encrypting).
    Crypto,
    /// Process spawning (exec, fork).
    Process,
    /// Environment variable access.
    Env,
    /// Terminal control (raw mode, cursor, colors).
    Term,
    /// Database access (query, transaction).
    Db,
    /// System clock access (wall time, monotonic).
    Time,
    /// Special: grants every capability. Dangerous — should be audited.
    All,
}

impl Capability {
    /// Parse a capability name (case-insensitive) from an annotation argument.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "net" => Some(Self::Net),
            "io" => Some(Self::Io),
            "ffi" => Some(Self::Ffi),
            "compute" => Some(Self::Compute),
            "crypto" => Some(Self::Crypto),
            "process" => Some(Self::Process),
            "env" => Some(Self::Env),
            "term" => Some(Self::Term),
            "db" => Some(Self::Db),
            "time" => Some(Self::Time),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    /// All concrete capabilities (excludes `All`).
    pub fn all_concrete() -> &'static [Capability] {
        &[
            Self::Net,
            Self::Io,
            Self::Ffi,
            Self::Compute,
            Self::Crypto,
            Self::Process,
            Self::Env,
            Self::Term,
            Self::Db,
            Self::Time,
        ]
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Net => "net",
            Self::Io => "io",
            Self::Ffi => "ffi",
            Self::Compute => "compute",
            Self::Crypto => "crypto",
            Self::Process => "process",
            Self::Env => "env",
            Self::Term => "term",
            Self::Db => "db",
            Self::Time => "time",
            Self::All => "all",
        };
        write!(f, "{}", name)
    }
}

/// A set of capabilities for a scope.
///
/// If the set contains `All`, every capability check succeeds.
/// Otherwise, only explicitly granted capabilities are available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySet {
    capabilities: HashSet<Capability>,
}

impl CapabilitySet {
    /// An empty capability set — no capabilities granted.
    pub fn empty() -> Self {
        Self {
            capabilities: HashSet::new(),
        }
    }

    /// Build a capability set from `@capabilities(...)` annotations.
    ///
    /// Scans the annotation list for annotations named `"capabilities"` and
    /// parses each argument as a capability name. Unknown names are silently
    /// ignored (the checker reports them as diagnostics separately).
    pub fn from_annotations(annotations: &[Annotation]) -> Self {
        let mut caps = HashSet::new();
        for ann in annotations {
            if ann.name == "capabilities" {
                for arg in &ann.args {
                    if let Some(cap) = Capability::from_str(arg) {
                        caps.insert(cap);
                    }
                }
            }
        }
        Self { capabilities: caps }
    }

    /// Check whether a specific capability is granted.
    ///
    /// Returns `true` if the set contains `cap` or contains `All`.
    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&Capability::All) || self.capabilities.contains(&cap)
    }

    /// Check whether `self` is a subset of `other`.
    ///
    /// If `self` contains `All`, then `other` must also contain `All`.
    /// Otherwise each capability in `self` must be present in `other`
    /// (or `other` must contain `All`).
    pub fn is_subset_of(&self, other: &CapabilitySet) -> bool {
        if other.capabilities.contains(&Capability::All) {
            return true;
        }
        for cap in &self.capabilities {
            if *cap == Capability::All {
                // self has All but other doesn't — not a subset.
                return false;
            }
            if !other.capabilities.contains(cap) {
                return false;
            }
        }
        true
    }

    /// Compute the union of two capability sets.
    pub fn union(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            capabilities: self.capabilities.union(&other.capabilities).copied().collect(),
        }
    }

    /// Iterate over the capabilities in this set.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.capabilities.iter()
    }

    /// Returns `true` if no capabilities are granted.
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    /// Number of capabilities in the set.
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// Insert a single capability.
    pub fn insert(&mut self, cap: Capability) {
        self.capabilities.insert(cap);
    }

    /// The excess capabilities that `self` has over `other`.
    ///
    /// Returns the set of capabilities present in `self` but not granted by `other`.
    pub fn excess_over(&self, other: &CapabilitySet) -> Vec<Capability> {
        if other.capabilities.contains(&Capability::All) {
            return Vec::new();
        }
        let mut excess = Vec::new();
        for cap in &self.capabilities {
            if *cap == Capability::All && !other.capabilities.contains(&Capability::All) {
                excess.push(*cap);
            } else if *cap != Capability::All && !other.capabilities.contains(cap) {
                excess.push(*cap);
            }
        }
        excess.sort_by_key(|c| format!("{c}"));
        excess
    }
}

/// Self-heal actions that are prohibited because they would escalate capabilities.
pub const PROHIBITED_SELF_HEAL_ACTIONS: &[&str] = &[
    "add_capability",
    "widen_sandbox",
    "increase_budget",
    "remove_spawn_limit",
    "modify_annotations",
    "escalate_tier",
];

/// Check if a function/method name is a prohibited self-heal escalation.
pub fn is_escalation_action(name: &str) -> bool {
    PROHIBITED_SELF_HEAL_ACTIONS.contains(&name)
}

/// Map a bare builtin function name to its required capability.
///
/// This catches calls like `file_write("out.txt", data)` where the caller
/// doesn't use a qualified `std.io.write_file(...)` path.
///
/// Returns `None` for functions that don't require any capability
/// (e.g. `println`, `print`, `len`, `push`, `to_string`).
pub fn required_capability_for_builtin(name: &str) -> Option<Capability> {
    match name {
        // File I/O
        "file_read" | "file_write" | "read_file" | "write_file" => Some(Capability::Io),

        // Filesystem operations
        "path_exists" | "is_file" | "is_dir" | "create_dir" | "remove_file"
        | "remove_dir" | "copy_file" | "rename_file" | "file_size"
        | "list_dir" | "walk_dir" => Some(Capability::Io),

        // Process
        "env_get" | "env_set" | "exit" | "exec" | "spawn_process" => Some(Capability::Process),

        // Network
        "http_get" | "http_post" | "tcp_connect" | "tcp_listen"
        | "tcp_accept" | "tcp_send" | "tcp_recv" => Some(Capability::Net),

        // Terminal
        "term_clear" | "term_raw_mode" | "term_size" => Some(Capability::Term),

        // Crypto
        "sha256" | "sha512" | "random_bytes" | "hmac_sha256" => Some(Capability::Crypto),

        // Time
        "time_now" | "time_millis" | "sleep" => Some(Capability::Time),

        // println, print, eprintln, to_string, len, push, etc. — NO capability needed
        _ => None,
    }
}

/// Map a stdlib module path prefix to its required capability.
///
/// For example, `std::net` requires `Net`, `std::io` requires `Io`, etc.
pub fn required_capability_for_path(segments: &[String]) -> Option<Capability> {
    if segments.len() < 2 || segments[0] != "std" {
        return None;
    }
    match segments[1].as_str() {
        "net" => Some(Capability::Net),
        "io" => Some(Capability::Io),
        "ffi" => Some(Capability::Ffi),
        "compute" => Some(Capability::Compute),
        "crypto" => Some(Capability::Crypto),
        "process" => Some(Capability::Process),
        "env" => Some(Capability::Env),
        "term" => Some(Capability::Term),
        "db" => Some(Capability::Db),
        "time" => Some(Capability::Time),
        _ => None,
    }
}

/// Budget annotation value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub limit: u64,
}

impl Budget {
    /// Parse a `@budget(N)` annotation. Returns `None` if not a budget annotation
    /// or if the argument is not a valid integer.
    pub fn from_annotations(annotations: &[Annotation]) -> Option<Self> {
        for ann in annotations {
            if ann.name == "budget" {
                if let Some(arg) = ann.args.first() {
                    if let Ok(limit) = arg.parse::<u64>() {
                        return Some(Budget { limit });
                    }
                }
            }
        }
        None
    }
}

/// Sandbox annotation presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sandbox {
    pub enabled: bool,
}

impl Sandbox {
    /// Check for `@sandbox` annotation presence.
    pub fn from_annotations(annotations: &[Annotation]) -> Self {
        let enabled = annotations.iter().any(|a| a.name == "sandbox");
        Sandbox { enabled }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kryos_errors::Span;

    fn make_annotation(name: &str, args: Vec<&str>) -> Annotation {
        Annotation {
            name: name.to_string(),
            args: args.into_iter().map(|s| s.to_string()).collect(),
            span: Span::DUMMY,
        }
    }

    #[test]
    fn parse_capability_names() {
        assert_eq!(Capability::from_str("net"), Some(Capability::Net));
        assert_eq!(Capability::from_str("IO"), Some(Capability::Io));
        assert_eq!(Capability::from_str("Ffi"), Some(Capability::Ffi));
        assert_eq!(Capability::from_str("unknown"), None);
    }

    #[test]
    fn empty_set_has_nothing() {
        let set = CapabilitySet::empty();
        assert!(!set.has(Capability::Net));
        assert!(!set.has(Capability::Io));
        assert!(set.is_empty());
    }

    #[test]
    fn all_grants_everything() {
        let mut set = CapabilitySet::empty();
        set.insert(Capability::All);
        for cap in Capability::all_concrete() {
            assert!(set.has(*cap));
        }
    }

    #[test]
    fn from_annotations_basic() {
        let anns = vec![make_annotation("capabilities", vec!["net", "io"])];
        let set = CapabilitySet::from_annotations(&anns);
        assert!(set.has(Capability::Net));
        assert!(set.has(Capability::Io));
        assert!(!set.has(Capability::Ffi));
    }

    #[test]
    fn subset_checks() {
        let parent = {
            let anns = vec![make_annotation("capabilities", vec!["net", "io", "time"])];
            CapabilitySet::from_annotations(&anns)
        };
        let child = {
            let anns = vec![make_annotation("capabilities", vec!["net"])];
            CapabilitySet::from_annotations(&anns)
        };
        assert!(child.is_subset_of(&parent));
        assert!(!parent.is_subset_of(&child));
    }

    #[test]
    fn all_is_superset_of_everything() {
        let all = {
            let anns = vec![make_annotation("capabilities", vec!["all"])];
            CapabilitySet::from_annotations(&anns)
        };
        let some = {
            let anns = vec![make_annotation("capabilities", vec!["net", "io"])];
            CapabilitySet::from_annotations(&anns)
        };
        assert!(some.is_subset_of(&all));
        assert!(!all.is_subset_of(&some));
    }

    #[test]
    fn excess_over_reports_extras() {
        let parent = {
            let anns = vec![make_annotation("capabilities", vec!["net"])];
            CapabilitySet::from_annotations(&anns)
        };
        let child = {
            let anns = vec![make_annotation("capabilities", vec!["net", "io"])];
            CapabilitySet::from_annotations(&anns)
        };
        let excess = child.excess_over(&parent);
        assert_eq!(excess, vec![Capability::Io]);
    }

    #[test]
    fn budget_from_annotations() {
        let anns = vec![make_annotation("budget", vec!["1000"])];
        let budget = Budget::from_annotations(&anns);
        assert_eq!(budget, Some(Budget { limit: 1000 }));
    }

    #[test]
    fn sandbox_from_annotations() {
        let anns = vec![make_annotation("sandbox", vec![])];
        let sandbox = Sandbox::from_annotations(&anns);
        assert!(sandbox.enabled);

        let no_sandbox = Sandbox::from_annotations(&[]);
        assert!(!no_sandbox.enabled);
    }

    #[test]
    fn escalation_detection() {
        assert!(is_escalation_action("add_capability"));
        assert!(is_escalation_action("widen_sandbox"));
        assert!(is_escalation_action("escalate_tier"));
        assert!(!is_escalation_action("handle_error"));
        assert!(!is_escalation_action("process_data"));
    }

    #[test]
    fn required_capability_mapping() {
        let path = vec!["std".into(), "net".into(), "TcpStream".into()];
        assert_eq!(required_capability_for_path(&path), Some(Capability::Net));

        let path = vec!["std".into(), "io".into(), "File".into()];
        assert_eq!(required_capability_for_path(&path), Some(Capability::Io));

        let path = vec!["mylib".into(), "net".into()];
        assert_eq!(required_capability_for_path(&path), None);
    }

    #[test]
    fn builtin_io_functions_require_io() {
        assert_eq!(required_capability_for_builtin("file_read"), Some(Capability::Io));
        assert_eq!(required_capability_for_builtin("file_write"), Some(Capability::Io));
        assert_eq!(required_capability_for_builtin("read_file"), Some(Capability::Io));
        assert_eq!(required_capability_for_builtin("write_file"), Some(Capability::Io));
        assert_eq!(required_capability_for_builtin("path_exists"), Some(Capability::Io));
        assert_eq!(required_capability_for_builtin("list_dir"), Some(Capability::Io));
        assert_eq!(required_capability_for_builtin("walk_dir"), Some(Capability::Io));
    }

    #[test]
    fn builtin_net_functions_require_net() {
        assert_eq!(required_capability_for_builtin("http_get"), Some(Capability::Net));
        assert_eq!(required_capability_for_builtin("http_post"), Some(Capability::Net));
        assert_eq!(required_capability_for_builtin("tcp_connect"), Some(Capability::Net));
        assert_eq!(required_capability_for_builtin("tcp_listen"), Some(Capability::Net));
    }

    #[test]
    fn builtin_process_functions_require_process() {
        assert_eq!(required_capability_for_builtin("env_get"), Some(Capability::Process));
        assert_eq!(required_capability_for_builtin("env_set"), Some(Capability::Process));
        assert_eq!(required_capability_for_builtin("exit"), Some(Capability::Process));
        assert_eq!(required_capability_for_builtin("exec"), Some(Capability::Process));
        assert_eq!(required_capability_for_builtin("spawn_process"), Some(Capability::Process));
    }

    #[test]
    fn builtin_term_functions_require_term() {
        assert_eq!(required_capability_for_builtin("term_clear"), Some(Capability::Term));
        assert_eq!(required_capability_for_builtin("term_raw_mode"), Some(Capability::Term));
        assert_eq!(required_capability_for_builtin("term_size"), Some(Capability::Term));
    }

    #[test]
    fn builtin_crypto_functions_require_crypto() {
        assert_eq!(required_capability_for_builtin("sha256"), Some(Capability::Crypto));
        assert_eq!(required_capability_for_builtin("sha512"), Some(Capability::Crypto));
        assert_eq!(required_capability_for_builtin("random_bytes"), Some(Capability::Crypto));
        assert_eq!(required_capability_for_builtin("hmac_sha256"), Some(Capability::Crypto));
    }

    #[test]
    fn builtin_time_functions_require_time() {
        assert_eq!(required_capability_for_builtin("time_now"), Some(Capability::Time));
        assert_eq!(required_capability_for_builtin("time_millis"), Some(Capability::Time));
        assert_eq!(required_capability_for_builtin("sleep"), Some(Capability::Time));
    }

    #[test]
    fn safe_builtins_require_no_capability() {
        assert_eq!(required_capability_for_builtin("println"), None);
        assert_eq!(required_capability_for_builtin("print"), None);
        assert_eq!(required_capability_for_builtin("eprintln"), None);
        assert_eq!(required_capability_for_builtin("to_string"), None);
        assert_eq!(required_capability_for_builtin("len"), None);
        assert_eq!(required_capability_for_builtin("push"), None);
        assert_eq!(required_capability_for_builtin("my_custom_func"), None);
    }
}
