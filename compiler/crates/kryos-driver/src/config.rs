//! Build configuration for the Kryos compiler driver.

use kryos_capabilities::CapabilityMode;
use std::path::PathBuf;

/// Which compilation backend / optimization level to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildMode {
    /// Cranelift backend, no optimizations — fast compile times.
    #[default]
    Debug,
    /// LLVM backend, O2 optimizations — optimized output.
    Release,
}

/// What the compiler should produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputType {
    /// Linked executable binary.
    #[default]
    Binary,
    /// Static or shared library.
    Library,
    /// Object file (.o).
    Object,
    /// LLVM IR text (.ll).
    LlvmIr,
    /// Dump MIR for debugging.
    Mir,
}

/// Configuration for a single compilation session.
///
/// The `input` and `output` fields use `String` rather than `PathBuf` so that
/// the CLI can pass them through directly from clap arguments.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// Source file or project root path.
    pub input: String,
    /// Output path. If `None`, the driver derives it from the input name.
    pub output: Option<String>,
    /// Debug or Release mode.
    pub mode: BuildMode,
    /// What kind of artifact to produce.
    pub output_type: OutputType,
    /// Target triple override (e.g. `x86_64-unknown-linux-gnu`).
    /// If `None`, the host target is used.
    pub target: Option<String>,
    /// Capabilities that are allowed for this build.
    pub capabilities: Vec<String>,
    /// Print verbose compiler output.
    pub verbose: bool,
    /// Skip ownership analysis (needed for self-host bootstrap).
    pub skip_ownership: bool,
    /// Link-Time Optimization. Enables `-flto=thin` on the per-object
    /// clang invocation and on the link step so the runtime helpers
    /// (which are marked `#[inline]`) get inlined into user code.
    /// Default: false. Release builds turn this on by default in the CLI.
    pub lto: bool,
    /// Emit DWARF debug info (`-g`).
    pub debug_info: bool,
    /// Use the cross-build artifact cache to short-circuit repeated
    /// invocations on the same `(source, target, mode, output_type)` tuple.
    /// Default: false. Opt-in so existing call sites are unaffected.
    pub use_cache: bool,
    /// Apply `kryos_mir::async_lower::apply_split_at_awaits` so async
    /// functions become real suspendable state machines at codegen time.
    /// Default: true. Can be disabled via `KRYOS_DISABLE_AWAIT_SPLIT=1` for
    /// debugging or to fall back to the pre-split synchronous lowering.
    pub split_async_awaits: bool,
    /// Deny capability-gated builtins in unannotated functions. When true,
    /// every function is checked as if it had an explicit `@capabilities(...)`
    /// annotation (the empty set unless declared), so a missing annotation
    /// no longer hides a `file_write` or `http_get` call.
    ///
    /// Wired to the `--strict-capabilities` flag on `kryos build` and
    /// `kryos check`. Default: `false` — the self-host bootstrap and existing
    /// projects depend on opt-in behaviour, so this MUST remain off for any
    /// default config the compiler itself produces.
    pub strict_capabilities: bool,
    /// Capability enforcement mode used when `strict_capabilities` is not set.
    /// `Permissive` = only annotated functions enforce (historical default);
    /// `Inferred` = deny-by-default at the boundary with interior inference.
    /// `--strict-capabilities` upgrades whatever this is to `Strict`.
    pub capability_mode: CapabilityMode,
}

impl BuildConfig {
    /// Create a build config for a single source file with default settings.
    pub fn for_file(path: impl Into<String>) -> Self {
        Self {
            input: path.into(),
            output: None,
            mode: BuildMode::default(),
            output_type: OutputType::default(),
            target: None,
            capabilities: Vec::new(),
            verbose: false,
            skip_ownership: false,
            lto: false,
            debug_info: false,
            use_cache: false,
            split_async_awaits: default_split_async_awaits(),
            strict_capabilities: false,
            capability_mode: CapabilityMode::Permissive,
        }
    }

    /// Create a build config for a project directory with default settings.
    pub fn for_project(dir: impl Into<String>) -> Self {
        Self {
            input: dir.into(),
            output: None,
            mode: BuildMode::default(),
            output_type: OutputType::default(),
            target: None,
            capabilities: Vec::new(),
            verbose: false,
            skip_ownership: false,
            lto: false,
            debug_info: false,
            use_cache: false,
            split_async_awaits: default_split_async_awaits(),
            strict_capabilities: false,
            capability_mode: CapabilityMode::Permissive,
        }
    }

    /// Return the input as a `PathBuf`.
    pub fn input_path(&self) -> PathBuf {
        PathBuf::from(&self.input)
    }

    /// Derive the default output path from the input path and output type.
    pub fn derive_output_path(&self) -> PathBuf {
        if let Some(ref out) = self.output {
            return PathBuf::from(out);
        }

        let input_path = PathBuf::from(&self.input);
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("out");

        match self.output_type {
            OutputType::Binary => {
                if cfg!(windows) {
                    PathBuf::from(format!("{stem}.exe"))
                } else {
                    PathBuf::from(stem)
                }
            }
            OutputType::Library => PathBuf::from(format!("lib{stem}.a")),
            OutputType::Object => PathBuf::from(format!("{stem}.o")),
            OutputType::LlvmIr => PathBuf::from(format!("{stem}.ll")),
            OutputType::Mir => PathBuf::from(format!("{stem}.mir")),
        }
    }

    /// Return the effective target triple string.
    #[doc(hidden)]
    pub fn _split_async_awaits_default() -> bool {
        default_split_async_awaits()
    }

    /// Return the effective target triple string.
    pub fn effective_target(&self) -> String {
        self.target
            .clone()
            .unwrap_or_else(|| kryos_linker::Target::host().triple_string())
    }
}

/// Default for [`BuildConfig::split_async_awaits`]. ON unless the
/// `KRYOS_DISABLE_AWAIT_SPLIT` environment variable is set to a truthy
/// value (anything non-empty other than `0`/`false`/`no`/`off`).
fn default_split_async_awaits() -> bool {
    match std::env::var("KRYOS_DISABLE_AWAIT_SPLIT").ok().as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("no") | Some("off") => true,
        _ => false,
    }
}
