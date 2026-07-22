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
    /// Network access (TCP, UDP, DNS). Coarse: grants every `net:*` sub-cap.
    Net,
    /// Network — HTTP(S) client/server only (`net:http`).
    NetHttp,
    /// Network — raw TCP / sockets / TLS (`net:tcp`).
    NetTcp,
    /// File I/O (read, write, seek, stat). Coarse: grants every `fs:*` sub-cap.
    /// Spelled `io` (legacy) or `fs`; both map here for back-compat.
    Io,
    /// Filesystem — read / stat only (`fs:read`).
    FsRead,
    /// Filesystem — write / create / mutate (`fs:write`).
    FsWrite,
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
    ///
    /// Sub-capabilities use a `family:scope` form (`net:http`, `fs:write`). The
    /// coarse families remain valid (`net`, `io`/`fs`) and grant all their
    /// sub-caps via [`Capability::satisfies`]. `io` and `fs` are aliases for the
    /// coarse filesystem capability (back-compat: `io` is the legacy spelling).
    pub fn from_str(s: &str) -> Option<Self> {
        // Capability names are single tokens, but the annotation tokenizer pads
        // the `:` sub-scope separator with spaces (`net : http`). Strip all
        // internal whitespace so both `net:http` and `net : http` parse.
        let normalized: String = s
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        match normalized.as_str() {
            "net" => Some(Self::Net),
            "net:http" => Some(Self::NetHttp),
            "net:tcp" => Some(Self::NetTcp),
            "io" | "fs" => Some(Self::Io),
            "fs:read" => Some(Self::FsRead),
            "fs:write" => Some(Self::FsWrite),
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

    /// All concrete coarse capabilities (excludes `All` and the `*:scope`
    /// sub-capabilities, which are always implied by their coarse family).
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

    /// Whether holding `self` grants (is sufficient for) a `required` capability.
    ///
    /// Containment rules:
    /// - `All` satisfies everything.
    /// - An exact match satisfies.
    /// - A coarse family satisfies each of its sub-capabilities
    ///   (`net` ⊇ `net:http`/`net:tcp`, `io`/`fs` ⊇ `fs:read`/`fs:write`).
    /// - A sub-capability does NOT satisfy a sibling sub-cap or the coarse
    ///   family (`fs:read` does not grant `fs:write`, nor full `io`).
    pub fn satisfies(&self, required: &Capability) -> bool {
        use Capability::*;
        if *self == All || self == required {
            return true;
        }
        matches!(
            (self, required),
            (Net, NetHttp) | (Net, NetTcp) | (Io, FsRead) | (Io, FsWrite)
        )
    }

    /// Whether `self` and `other` share any access — symmetric containment.
    ///
    /// Used by the manifest `--deny` gate: denying a coarse family (`net`)
    /// must trip a function declaring only a sub-cap (`net:http`), and denying
    /// a sub-cap (`net:tcp`) must trip a function declaring the coarse family.
    pub fn overlaps(&self, other: &Capability) -> bool {
        self.satisfies(other) || other.satisfies(self)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Net => "net",
            Self::NetHttp => "net:http",
            Self::NetTcp => "net:tcp",
            Self::Io => "io",
            Self::FsRead => "fs:read",
            Self::FsWrite => "fs:write",
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

    /// Check whether a specific capability is granted by exact match.
    ///
    /// Returns `true` if the set contains `cap` or contains `All`. This is the
    /// historical exact-membership test; for sub-capability containment
    /// (a coarse declaration granting a required sub-cap) use
    /// [`CapabilitySet::satisfies_required`].
    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&Capability::All) || self.capabilities.contains(&cap)
    }

    /// Check whether this set grants a `required` capability, honouring
    /// sub-capability containment (e.g. a declared coarse `net` satisfies a
    /// required `net:http`; `all` satisfies everything).
    pub fn satisfies_required(&self, required: &Capability) -> bool {
        self.capabilities.iter().any(|c| c.satisfies(required))
    }

    /// Whether any declared capability overlaps `cap` (symmetric containment).
    /// Used by the manifest `--deny` gate so denying `net` also trips `net:http`.
    pub fn overlaps_capability(&self, cap: Capability) -> bool {
        self.capabilities.iter().any(|c| c.overlaps(&cap))
    }

    /// Check whether `self` is a subset of `other`, honouring sub-capability
    /// containment: every capability in `self` must be satisfied by some
    /// capability in `other`. A coarse `net` in `other` therefore covers a
    /// `net:http` in `self`; `all` in `other` covers everything.
    pub fn is_subset_of(&self, other: &CapabilitySet) -> bool {
        self.capabilities.iter().all(|c| other.satisfies_required(c))
    }

    /// Compute the union of two capability sets.
    pub fn union(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            capabilities: self
                .capabilities
                .union(&other.capabilities)
                .copied()
                .collect(),
        }
    }

    /// Narrow the set by removing every capability that overlaps any of the
    /// `denied` capabilities. Used by `deny!(...)` blocks: denying coarse `net`
    /// removes `net`, `net:http`, and `net:tcp`; denying a sub-cap `net:http`
    /// removes `net:http` (and, conservatively, a coarse `net` that overlaps it).
    /// `all` is removed if anything is denied, since `all` overlaps everything.
    pub fn without(&self, denied: &[Capability]) -> CapabilitySet {
        CapabilitySet {
            capabilities: self
                .capabilities
                .iter()
                .filter(|c| !denied.iter().any(|d| c.overlaps(d)))
                .copied()
                .collect(),
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
        let mut excess: Vec<Capability> = self
            .capabilities
            .iter()
            .filter(|c| !other.satisfies_required(c))
            .copied()
            .collect();
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
        // Filesystem — read / stat (fs:read)
        "file_read" | "read_file" => Some(Capability::FsRead),
        // `file_exists` is a documented global builtin (stat/existence probe) and
        // leaks the presence of secrets/config/other-user files; it belongs with
        // the other fs:read stat ops, which it was missing from (gate-enumeration gap).
        "path_exists" | "file_exists" | "is_file" | "is_dir" | "file_size" | "list_dir"
        | "walk_dir" => Some(Capability::FsRead),

        // Filesystem — write / create / mutate (fs:write). `append_file` (creates
        // and appends) and `buf_write_to_file` (writes a byte buffer to a path)
        // are real write authority that were UNGATED — a deny-by-default escape.
        "file_write" | "write_file" | "append_file" | "buf_write_to_file" | "create_dir"
        | "remove_file" | "remove_dir" | "copy_file" | "rename_file" => Some(Capability::FsWrite),

        // Process — running OTHER programs (exec/spawn) and reading/writing the
        // environment (which can exfiltrate secrets) are real authority.
        "env_get" | "env_set" | "exec" | "spawn_process" => Some(Capability::Process),

        // `exit` / `abort` terminate the CURRENT process. That is control flow,
        // not authority: it cannot read data, reach other systems, or escalate
        // -- it just stops. Gating it made strict-capabilities impractical
        // (every test assert-helper and CLI error path calls exit). Ambient,
        // like Rust's process::exit and WASI's proc_exit.
        "exit" | "abort" => None,

        // Network — HTTP(S) client/server (net:http). `http_request` /
        // `https_get` are real callable builtins that were UNGATED.
        // `fetch_text` is a convenience wrapper that calls `https_get` on native —
        // identical outbound-HTTP authority under a different bare name the gate
        // never checked (SSRF/exfiltration escape). Gated with the rest of net:http.
        "http_get" | "http_post" | "http_request" | "https_get" | "fetch_text"
        | "http2_get" | "http2_post" | "http2_request" => Some(Capability::NetHttp),

        // Network — raw TCP / sockets / TLS / unix-domain sockets (net:tcp).
        // The non-blocking variants (tcp_bind/set_nonblocking/try_accept/
        // try_recv) are network authority too and were UNGATED.
        // `tcp_close` closes an OS socket fd — real net:tcp authority that was
        // ungated even though its siblings `tls_close`/`uds_close` were gated.
        "tcp_connect" | "tcp_listen" | "tcp_accept" | "tcp_send" | "tcp_recv" | "tcp_close"
        | "tcp_bind" | "tcp_set_nonblocking" | "tcp_try_accept" | "tcp_try_recv"
        | "tls_server_config" | "tls_accept" | "tls_send" | "tls_recv" | "tls_close"
        | "uds_connect" | "uds_bind" | "uds_accept" | "uds_send" | "uds_recv"
        | "uds_close" => Some(Capability::NetTcp),

        // Network — postgres wire + websocket framing helpers stay coarse `net`
        // (pg straddles connect+query; ws_* are pure byte (de)framing used by
        // net code). A coarse `net` declaration covers all of the above.
        "pg_connect" | "pg_exec" | "pg_query" | "pg_close"
        | "ws_accept_key" | "ws_encode_text" | "ws_encode_binary" | "ws_encode_close"
        | "ws_encode_ping" | "ws_encode_pong" | "ws_unmask" | "ws_read_frame" => Some(Capability::Net),

        // Terminal
        "term_clear" | "term_raw_mode" | "term_size" => Some(Capability::Term),

        // Crypto — hashing, key derivation, signing, key generation, and CSPRNG
        // bytes. All require `crypto`: signing/keygen (ed25519_*) is authority
        // over key material; hashing is gated for consistency + audit (you want
        // to KNOW when code does crypto). Previously only sha256/sha512/
        // random_bytes/hmac_sha256 were gated -- ed25519_sign/generate/public/
        // verify, pbkdf2, and the sha1 helpers were UNGATED, so an unannotated
        // function could sign or mint keys without declaring `crypto`.
        // (base64/hex encoders are pure data transforms, NOT crypto -- ungated.)
        "sha256" | "sha512" | "sha1_hex" | "sha1_base64" | "hmac_sha256"
        | "pbkdf2_sha256" | "random_bytes" | "ed25519_generate" | "ed25519_public"
        | "ed25519_sign" | "ed25519_verify" => Some(Capability::Crypto),

        // Time — reading the wall clock and pausing your OWN execution are
        // self-scoped: they cannot read your data, reach other systems, or
        // take unauthorized action, so they are ambient (not gated). An
        // agent that stalls is bounded by `@budget`, not a compile-time cap.
        // The `Capability::Time` variant is kept for a future deterministic-
        // execution / reproducible-clock mode, where it would re-gate these.
        "time_now" | "time_millis" | "sleep" => None,

        // println, print, eprintln, to_string, len, push, etc. — NO capability needed
        _ => None,
    }
}

/// The capability a RAW NATIVE runtime symbol requires, for an `extern { fn
/// kryos_<sym> }` call (`sym` is the name with the `kryos_` prefix already
/// stripped). The native symbol vocabulary is LARGER and spelled differently
/// from the language-builtin vocabulary [`required_capability_for_builtin`]
/// checks: natives carry a `_ks` (kryos-string ABI) suffix or a `builtin_`
/// prefix, and some use a different verb order (`dir_create` vs the builtin
/// `create_dir`, `file_remove` vs `remove_file`, `process_exec` vs `exec`).
/// Because a program can hand-declare `extern { fn kryos_process_exec_simple }`
/// and call it (marshalling args via the ungated `str_to_ptr`/`len`
/// primitives), mapping ONLY the builtin names left every authority-bearing
/// native ambient -- a full bypass of deny-by-default (arbitrary process
/// exec / fs write / outbound HTTP from a zero-capability `main`). This maps
/// the natives to the SAME authority as the builtin they back.
pub fn required_capability_for_native_symbol(sym: &str) -> Option<Capability> {
    // 1. Exact language-builtin spelling (file_read, create_dir, exec, ...).
    if let Some(c) = required_capability_for_builtin(sym) {
        return Some(c);
    }
    // 2. Normalize the ABI-variant spellings and retry the builtin table: strip
    //    a `_ks` suffix and/or a `builtin_` prefix (http_request_ks -> http_request,
    //    https_get_ks -> https_get, tcp_connect_ks -> tcp_connect, builtin_http_get
    //    -> http_get, builtin_file_write -> file_write).
    let norm = sym.strip_suffix("_ks").unwrap_or(sym);
    let norm = norm.strip_prefix("builtin_").unwrap_or(norm);
    if norm != sym {
        if let Some(c) = required_capability_for_builtin(norm) {
            return Some(c);
        }
    }
    // 3. Native symbols whose name differs from the builtin (verb order /
    //    distinct spelling) and therefore never matched the table. These are the
    //    raw runtime entry points registered in the JIT (kryos-codegen-cranelift
    //    jit.rs) behind the documented fs/process/net builtins.
    match norm {
        // process — spawn/run other programs
        "process_exec" | "process_exec_simple" => Some(Capability::Process),
        // fs:write — create/remove/delete on disk
        "dir_create" | "dir_remove" | "file_remove" | "fs_delete" | "file_append" => {
            Some(Capability::FsWrite)
        }
        // fs:read — enumerate/stat on disk
        "dir_list" | "dir_walk" | "fs_exists" => Some(Capability::FsRead),
        // NOTE: `file_open` is NEUTRAL and stays ambient. Opening a file handle
        // (with a read/write/append mode arg) carries no authority by itself --
        // the actual read is `kryos_file_read` (fs:read) and the actual write is
        // `kryos_file_write` (fs:write), both gated above. Mapping file_open to
        // fs:read made std::fs::write_file / append_file (which open with a
        // WRITE mode) demand fs:read, so a caller declaring fs:write could not
        // use them at all (they were uncallable under every declaration,
        // including a shipped example).
        // net:http — the outbound-HTTP natives whose bare name isn't in the table
        "https_get" | "http_request" | "http2_get" | "http2_post" | "http2_request" => {
            Some(Capability::NetHttp)
        }
        // net:tcp — raw sockets / TLS / unix-domain sockets closing helpers whose
        // bare native name isn't in the table (their connect/send/recv siblings
        // normalize into the table above).
        "socket_close" | "tls_server_config" | "tls_accept" | "uds_accept" | "uds_close" => {
            Some(Capability::NetTcp)
        }
        // Genuine ambient plumbing (allocators, pointer/string helpers, stdout/
        // stderr/stdin, process_exit/argc/argv, file_close, socket handles) —
        // no authority, stays ungated.
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
        "io" | "fs" => Some(Capability::Io),
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
    fn builtin_filesystem_functions_require_fs_subcaps() {
        // Read / stat ops require fs:read.
        assert_eq!(
            required_capability_for_builtin("file_read"),
            Some(Capability::FsRead)
        );
        assert_eq!(
            required_capability_for_builtin("read_file"),
            Some(Capability::FsRead)
        );
        assert_eq!(
            required_capability_for_builtin("path_exists"),
            Some(Capability::FsRead)
        );
        assert_eq!(
            required_capability_for_builtin("list_dir"),
            Some(Capability::FsRead)
        );
        assert_eq!(
            required_capability_for_builtin("walk_dir"),
            Some(Capability::FsRead)
        );
        // Write / mutate ops require fs:write.
        assert_eq!(
            required_capability_for_builtin("file_write"),
            Some(Capability::FsWrite)
        );
        assert_eq!(
            required_capability_for_builtin("write_file"),
            Some(Capability::FsWrite)
        );
        assert_eq!(
            required_capability_for_builtin("create_dir"),
            Some(Capability::FsWrite)
        );
        assert_eq!(
            required_capability_for_builtin("remove_file"),
            Some(Capability::FsWrite)
        );
    }

    #[test]
    fn builtin_net_functions_require_net_subcaps() {
        // HTTP ops require net:http.
        assert_eq!(
            required_capability_for_builtin("http_get"),
            Some(Capability::NetHttp)
        );
        assert_eq!(
            required_capability_for_builtin("http_post"),
            Some(Capability::NetHttp)
        );
        // Raw socket ops require net:tcp.
        assert_eq!(
            required_capability_for_builtin("tcp_connect"),
            Some(Capability::NetTcp)
        );
        assert_eq!(
            required_capability_for_builtin("tcp_listen"),
            Some(Capability::NetTcp)
        );
    }

    #[test]
    fn parse_subcapability_names() {
        assert_eq!(Capability::from_str("net:http"), Some(Capability::NetHttp));
        assert_eq!(Capability::from_str("NET:HTTP"), Some(Capability::NetHttp));
        assert_eq!(Capability::from_str("net:tcp"), Some(Capability::NetTcp));
        assert_eq!(Capability::from_str("fs:read"), Some(Capability::FsRead));
        assert_eq!(Capability::from_str("fs:write"), Some(Capability::FsWrite));
        // Coarse aliases: both `io` (legacy) and `fs` map to the coarse fs cap.
        assert_eq!(Capability::from_str("io"), Some(Capability::Io));
        assert_eq!(Capability::from_str("fs"), Some(Capability::Io));
        // Unknown scope is rejected.
        assert_eq!(Capability::from_str("net:bogus"), None);
        assert_eq!(Capability::from_str("fs:append"), None);
    }

    #[test]
    fn subcapability_display() {
        assert_eq!(Capability::NetHttp.to_string(), "net:http");
        assert_eq!(Capability::NetTcp.to_string(), "net:tcp");
        assert_eq!(Capability::FsRead.to_string(), "fs:read");
        assert_eq!(Capability::FsWrite.to_string(), "fs:write");
        // Coarse fs still displays as `io` (back-compat with existing manifests).
        assert_eq!(Capability::Io.to_string(), "io");
    }

    #[test]
    fn coarse_satisfies_subcaps_but_not_vice_versa() {
        // Coarse grants its sub-caps.
        assert!(Capability::Net.satisfies(&Capability::NetHttp));
        assert!(Capability::Net.satisfies(&Capability::NetTcp));
        assert!(Capability::Io.satisfies(&Capability::FsRead));
        assert!(Capability::Io.satisfies(&Capability::FsWrite));
        // All grants everything.
        assert!(Capability::All.satisfies(&Capability::FsWrite));
        assert!(Capability::All.satisfies(&Capability::NetTcp));
        // Sub-cap does not grant sibling, nor the coarse family.
        assert!(!Capability::FsRead.satisfies(&Capability::FsWrite));
        assert!(!Capability::FsWrite.satisfies(&Capability::FsRead));
        assert!(!Capability::NetHttp.satisfies(&Capability::NetTcp));
        assert!(!Capability::NetHttp.satisfies(&Capability::Net));
        assert!(!Capability::FsRead.satisfies(&Capability::Io));
        // Exact match satisfies.
        assert!(Capability::FsWrite.satisfies(&Capability::FsWrite));
        // Different families never satisfy.
        assert!(!Capability::Net.satisfies(&Capability::FsRead));
    }

    #[test]
    fn capset_satisfies_required_containment() {
        let io = CapabilitySet::from_annotations(&[make_annotation("capabilities", vec!["io"])]);
        assert!(io.satisfies_required(&Capability::FsWrite));
        assert!(io.satisfies_required(&Capability::FsRead));

        let ro =
            CapabilitySet::from_annotations(&[make_annotation("capabilities", vec!["fs:read"])]);
        assert!(ro.satisfies_required(&Capability::FsRead));
        assert!(!ro.satisfies_required(&Capability::FsWrite));

        let net = CapabilitySet::from_annotations(&[make_annotation("capabilities", vec!["net"])]);
        assert!(net.satisfies_required(&Capability::NetHttp));
        assert!(net.satisfies_required(&Capability::NetTcp));
    }

    #[test]
    fn deny_overlap_is_symmetric_within_family() {
        // Declaring net:http overlaps a coarse `net` deny.
        let http =
            CapabilitySet::from_annotations(&[make_annotation("capabilities", vec!["net:http"])]);
        assert!(http.overlaps_capability(Capability::Net));
        // Declaring coarse net overlaps a `net:tcp` deny.
        let net = CapabilitySet::from_annotations(&[make_annotation("capabilities", vec!["net"])]);
        assert!(net.overlaps_capability(Capability::NetTcp));
        // But net:http does not overlap an unrelated fs deny.
        assert!(!http.overlaps_capability(Capability::Io));
        // ...and net:http does not overlap a net:tcp deny (disjoint siblings).
        assert!(!http.overlaps_capability(Capability::NetTcp));
    }

    #[test]
    fn builtin_process_functions_require_process() {
        assert_eq!(
            required_capability_for_builtin("env_get"),
            Some(Capability::Process)
        );
        assert_eq!(
            required_capability_for_builtin("env_set"),
            Some(Capability::Process)
        );
        // exit/abort terminate the current process only -- ambient control
        // flow, not a gated authority (see required_capability_for_builtin).
        assert_eq!(required_capability_for_builtin("exit"), None);
        assert_eq!(required_capability_for_builtin("abort"), None);
        assert_eq!(
            required_capability_for_builtin("exec"),
            Some(Capability::Process)
        );
        assert_eq!(
            required_capability_for_builtin("spawn_process"),
            Some(Capability::Process)
        );
    }

    #[test]
    fn builtin_term_functions_require_term() {
        assert_eq!(
            required_capability_for_builtin("term_clear"),
            Some(Capability::Term)
        );
        assert_eq!(
            required_capability_for_builtin("term_raw_mode"),
            Some(Capability::Term)
        );
        assert_eq!(
            required_capability_for_builtin("term_size"),
            Some(Capability::Term)
        );
    }

    #[test]
    fn builtin_crypto_functions_require_crypto() {
        assert_eq!(
            required_capability_for_builtin("sha256"),
            Some(Capability::Crypto)
        );
        assert_eq!(
            required_capability_for_builtin("sha512"),
            Some(Capability::Crypto)
        );
        assert_eq!(
            required_capability_for_builtin("random_bytes"),
            Some(Capability::Crypto)
        );
        assert_eq!(
            required_capability_for_builtin("hmac_sha256"),
            Some(Capability::Crypto)
        );
    }

    #[test]
    fn builtin_time_functions_are_ambient() {
        // Clock reads and self-pause are self-scoped -> ambient (see the
        // Time arm in required_capability_for_builtin). The Time variant is
        // retained for a future deterministic-clock mode.
        assert_eq!(required_capability_for_builtin("time_now"), None);
        assert_eq!(required_capability_for_builtin("time_millis"), None);
        assert_eq!(required_capability_for_builtin("sleep"), None);
    }

    #[test]
    fn previously_ungated_authority_builtins_are_gated() {
        // Regression: these 5 authority builtins are dispatched by codegen but
        // were MISSING from the gate, so unannotated code could call them under
        // deny-by-default (inferred) mode -- a capability escape. (launch-hardening
        // 2026-07-13: fetch_text=SSRF/exfil, append_file/buf_write_to_file=fs:write,
        // file_exists=fs:read probe, tcp_close=net:tcp.)
        assert_eq!(
            required_capability_for_builtin("fetch_text"),
            Some(Capability::NetHttp)
        );
        assert_eq!(
            required_capability_for_builtin("append_file"),
            Some(Capability::FsWrite)
        );
        assert_eq!(
            required_capability_for_builtin("buf_write_to_file"),
            Some(Capability::FsWrite)
        );
        assert_eq!(
            required_capability_for_builtin("file_exists"),
            Some(Capability::FsRead)
        );
        assert_eq!(
            required_capability_for_builtin("tcp_close"),
            Some(Capability::NetTcp)
        );
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
