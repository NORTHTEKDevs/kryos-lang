//! Build configuration for the Kryos compiler driver.

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
    pub fn effective_target(&self) -> String {
        self.target
            .clone()
            .unwrap_or_else(|| kryos_linker::Target::host().triple_string())
    }
}
