//! `kryos` — the Kryos programming language compiler CLI.
//!
//! Entry point that parses arguments with `clap` and delegates to
//! the appropriate command module.

#![allow(clippy::too_many_arguments)]

mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "kryos",
    version,
    about = "The Kryos programming language compiler",
    long_about = "Kryos is a capability-safe, ownership-aware language with\n\
                  Cranelift (debug) and LLVM (release) backends."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile a Kryos project or file
    Build {
        /// Source file or project directory
        #[arg(default_value = ".")]
        path: String,

        /// Build in release mode (LLVM backend, optimizations)
        #[arg(long)]
        release: bool,

        /// Override the codegen backend. Values: cranelift, llvm, wasm.
        /// If unset, uses cranelift for debug and llvm for release.
        #[arg(long, value_name = "NAME")]
        backend: Option<String>,

        /// Target triple (e.g. x86_64-unknown-linux-gnu)
        #[arg(long)]
        target: Option<String>,

        /// Output path
        #[arg(short, long)]
        output: Option<String>,

        /// Emit MIR instead of binary
        #[arg(long)]
        emit_mir: bool,

        /// Emit LLVM IR instead of binary
        #[arg(long)]
        emit_llvm: bool,

        /// Print verbose compiler internals
        #[arg(short, long)]
        verbose: bool,

        /// Skip ownership analysis (for self-host bootstrap)
        #[arg(long)]
        skip_ownership: bool,

        /// Enable Link-Time Optimization (cross-module inlining).
        /// Implied by --release. Disable with --no-lto.
        #[arg(long)]
        lto: bool,

        /// Force-disable LTO even in release builds.
        #[arg(long)]
        no_lto: bool,

        /// Emit DWARF debug info for gdb/lldb source-level debugging.
        #[arg(short = 'g', long)]
        debug_info: bool,

        /// Enable the cross-build artifact cache. Identical source +
        /// (target, mode, output_type, compiler version) tuples reuse a
        /// previously produced binary without rerunning the pipeline. The
        /// cache lives under `$KRYOS_CACHE_DIR` (or `$XDG_CACHE_HOME/kryos`,
        /// `$HOME/.cache/kryos`). Disable with --no-cache.
        #[arg(long)]
        cache: bool,

        /// Force-disable the cross-build artifact cache.
        #[arg(long)]
        no_cache: bool,
    },

    /// Compile and run a Kryos file
    Run {
        /// Source file to run
        file: String,

        /// Arguments to pass to the program
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Type-check without compiling
    Check {
        /// Source file or project directory
        #[arg(default_value = ".")]
        path: String,

        /// Skip ownership analysis (for self-host bootstrap)
        #[arg(long)]
        skip_ownership: bool,
    },

    /// Interactive REPL
    Repl,

    /// Run tests in the current project
    Test {
        /// Filter test names by substring (positional). If `--exact` is set,
        /// only tests whose name exactly equals this value are run.
        #[arg(value_name = "FILTER")]
        filter_pos: Option<String>,

        /// Filter test names by substring (kept for backwards compatibility;
        /// equivalent to the positional FILTER argument).
        #[arg(long, value_name = "PATTERN")]
        filter: Option<String>,

        /// Match the filter exactly (not as a substring).
        #[arg(long)]
        exact: bool,

        /// Show output captured from passing tests (cargo-test parity).
        #[arg(long = "nocapture")]
        nocapture: bool,

        /// Report format. `pretty` (default) prints a human-readable colored
        /// summary; `json` prints one JSON event per line for CI consumption.
        #[arg(long, value_name = "FORMAT", default_value = "pretty")]
        format: String,

        /// List discovered test names and exit, one per line.
        #[arg(long)]
        list: bool,

        /// Path to a `.kry` file or directory to search for tests. When omitted,
        /// `kryos test` looks for a `tests/` directory and falls back to `.`.
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
    },

    /// Watch a file and auto-rebuild on changes
    Watch {
        /// File to watch.
        file: String,

        /// Poll interval in milliseconds (default 250).
        #[arg(long, value_name = "MS")]
        interval: Option<u64>,

        /// Action on change: "run" (default) or "check".
        #[arg(long, value_name = "ACTION", default_value = "run")]
        run: String,
    },

    /// Remove build artifacts (target/, *.exe, *.pdb, *.o, *.ll, *.wasm, kryos.lock)
    Clean {
        /// Project directory (default: cwd).
        #[arg(value_name = "PATH")]
        path: Option<String>,

        /// Print what would be removed without removing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Run a Kryos program with verbose function-entry/exit tracing
    Trace {
        /// File to execute via the JIT.
        file: String,

        /// Optional args passed to the program (after `--`).
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run a Kryos program with call-count profiling
    Profile {
        /// File to execute.
        file: String,

        /// Optional args passed to the program (after `--`).
        #[arg(last = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Generate a new Kryos project from a template
    New {
        /// Project name (also the directory name).
        name: String,

        /// Template: cli (default), http, lib, agent.
        #[arg(long, value_name = "TEMPLATE", default_value = "cli")]
        template: String,

        /// Parent directory in which to create the project. Default: cwd.
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
    },

    /// Lint Kryos source for stylistic + correctness issues
    Lint {
        /// Path to lint (file or directory). Default: current directory.
        #[arg(value_name = "PATH")]
        path: Option<String>,

        /// Output format: pretty (default) or json.
        #[arg(long, value_name = "FORMAT", default_value = "pretty")]
        format: String,

        /// Only enable these lint codes (comma-separated, e.g. L001,L003).
        #[arg(long, value_delimiter = ',')]
        enable: Vec<String>,

        /// Disable these lint codes (comma-separated).
        #[arg(long, value_delimiter = ',')]
        disable: Vec<String>,

        /// Exit non-zero if any lint diagnostic is emitted (CI mode).
        #[arg(long)]
        strict: bool,
    },

    /// Audit the project: capability usage, extern surface, secret patterns
    Audit {
        /// Path to audit. Default: current directory.
        #[arg(value_name = "PATH")]
        path: Option<String>,

        /// Output format: pretty (default) or json.
        #[arg(long, value_name = "FORMAT", default_value = "pretty")]
        format: String,
    },

    /// Run `@bench`-annotated micro-benchmarks
    Bench {
        /// Filter bench names by substring (positional).
        #[arg(value_name = "FILTER")]
        filter_pos: Option<String>,

        /// Match the filter exactly (not as a substring).
        #[arg(long)]
        exact: bool,

        /// Warmup iterations before each measurement run.
        #[arg(long, value_name = "N")]
        warmup: Option<usize>,

        /// Measurement iterations per bench function.
        #[arg(long, value_name = "N")]
        measure: Option<usize>,

        /// Path to a `.kry` file or directory. Defaults to `benches/`,
        /// falling back to `tests/`, then the current directory.
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
    },

    /// Format source files
    Fmt {
        /// Files to format (default: all .kry files in project)
        files: Vec<String>,

        /// Check formatting without modifying files
        #[arg(long)]
        check: bool,
    },

    /// Generate documentation from source files
    Doc {
        /// Files to generate documentation for (default: all .kry files)
        files: Vec<String>,

        /// Output directory for rendered files (default: stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Emit HTML instead of markdown
        #[arg(long)]
        html: bool,
    },

    /// Generate Kryos bindings from C header files
    Bindgen {
        /// C header file to process
        header: String,

        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Package management
    Pkg {
        #[command(subcommand)]
        action: PkgAction,
    },

    /// Start the language server (LSP)
    Lsp,

    /// Show a long-form explanation for an error code (like `rustc --explain`).
    ///
    /// Pass an error code (e.g. `E0100`) to read its article, or `--list`
    /// to see every code that has an explanation.
    Explain {
        /// Error code (e.g. E0100). Omit to list every code with an explanation.
        #[arg(default_value = "list")]
        code: String,

        /// List all error codes that have a long-form explanation.
        #[arg(long, short)]
        list: bool,
    },

    /// Print detailed version and build information
    Version,
}

#[derive(Subcommand)]
enum PkgAction {
    /// Initialize a new Kryos project
    Init {
        /// Project name (default: current directory name)
        name: Option<String>,
    },

    /// Add a dependency to kryos.toml
    Add {
        /// Dependency specifier (e.g. github:kryos-lang/serde@^1.0.0)
        dependency: String,
    },

    /// Remove a dependency from kryos.toml
    Remove {
        /// Dependency name to remove
        dependency: String,
    },

    /// Update dependencies to latest compatible versions
    Update,

    /// Resolve and fetch all dependencies
    Install,

    /// Regenerate the lock file
    Lock,

    /// Package and publish to the registry
    Publish,

    /// Search the registry for packages
    Search {
        /// Search query (substring match)
        query: String,
    },

    /// Show package info from the registry
    Info {
        /// Package name
        name: String,
    },

    /// Sync the registry index
    Sync,

    /// List locked packages with newer versions available
    Outdated,

    /// List first-party / local packages discovered under `packages/` (or a
    /// custom directory).
    ListLocal {
        /// Root directory to scan. Default: `./packages`.
        #[arg(long, value_name = "PATH")]
        root: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Build {
            path,
            release,
            backend,
            target,
            output,
            emit_mir,
            emit_llvm,
            verbose,
            skip_ownership,
            lto,
            no_lto,
            debug_info,
            cache,
            no_cache,
        } => {
            // --no-lto wins over both --lto and the release-implied LTO default.
            let effective_lto = if no_lto { false } else { lto };
            // --no-cache wins over --cache and the KRYOS_CACHE env var.
            // Otherwise: explicit --cache flag, or KRYOS_CACHE=1, opts in.
            let env_opt_in = std::env::var("KRYOS_CACHE")
                .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
                .unwrap_or(false);
            let effective_cache = if no_cache { false } else { cache || env_opt_in };
            commands::build::execute(
                &path,
                release,
                backend.as_deref(),
                target.as_deref(),
                output.as_deref(),
                emit_mir,
                emit_llvm,
                verbose,
                skip_ownership,
                effective_lto,
                debug_info,
                effective_cache,
            )
        }

        Commands::Run { file, args } => commands::run::execute(&file, &args),

        Commands::Check {
            path,
            skip_ownership,
        } => commands::check::execute(&path, skip_ownership),

        Commands::Repl => commands::repl::execute(),

        Commands::Test {
            filter_pos,
            filter,
            exact,
            nocapture,
            format,
            list,
            path,
        } => {
            // Positional FILTER takes precedence over --filter; otherwise fall back.
            // Special case: if filter_pos points to an existing path on disk and
            // --path was not given, treat it as the path. This lets users run
            // `kryos test path/to/file.kry` or `kryos test path/to/dir/` directly.
            let (chosen_path, chosen_filter) = match (filter_pos, path) {
                (Some(p), None) if std::path::Path::new(&p).exists() => (Some(p), filter),
                (fp, p) => (p, fp.or(filter)),
            };
            match format.as_str() {
                "pretty" | "json" => {
                    let fmt = if format == "json" {
                        commands::test_cmd::OutputFormat::Json
                    } else {
                        commands::test_cmd::OutputFormat::Pretty
                    };
                    commands::test_cmd::execute(commands::test_cmd::TestOptions {
                        filter: chosen_filter,
                        exact,
                        nocapture,
                        format: fmt,
                        list,
                        path: chosen_path,
                    })
                }
                other => Err(format!(
                    "unknown --format value '{}': expected 'pretty' or 'json'",
                    other
                )),
            }
        }

        Commands::Watch { file, interval, run } => {
            commands::watch_cmd::execute(commands::watch_cmd::WatchOptions {
                file,
                interval_ms: interval,
                run,
            })
        }

        Commands::Clean { path, dry_run } => {
            commands::clean_cmd::execute(commands::clean_cmd::CleanOptions {
                path,
                dry_run,
            })
        }

        Commands::Trace { file, args } => commands::trace_cmd::execute(&file, &args),

        Commands::Profile { file, args } => commands::profile_cmd::execute(&file, &args),

        Commands::New { name, template, path } => {
            commands::new_cmd::execute(commands::new_cmd::NewOptions {
                name,
                template,
                path,
            })
        }

        Commands::Lint { path, format, enable, disable, strict } => {
            commands::lint_cmd::execute(commands::lint_cmd::LintOptions {
                path,
                format,
                enable,
                disable,
                strict,
            })
        }

        Commands::Audit { path, format } => {
            commands::audit_cmd::execute(commands::audit_cmd::AuditOptions {
                path,
                format,
            })
        }

        Commands::Bench {
            filter_pos,
            exact,
            warmup,
            measure,
            path,
        } => {
            let (chosen_path, chosen_filter) = match (filter_pos, path) {
                (Some(p), None) if std::path::Path::new(&p).exists() => (Some(p), None),
                (fp, p) => (p, fp),
            };
            commands::bench_cmd::execute(commands::bench_cmd::BenchCliOptions {
                filter: chosen_filter,
                exact,
                warmup,
                measure,
                path: chosen_path,
            })
        }

        Commands::Fmt { files, check } => commands::fmt::execute(&files, check),

        Commands::Doc {
            files,
            output,
            html,
        } => commands::doc::execute(&files, output.as_deref(), html),

        Commands::Bindgen { header, output } => {
            commands::bindgen::execute(&header, output.as_deref())
        }

        Commands::Pkg { action } => match action {
            PkgAction::Init { name } => commands::pkg::init(name.as_deref()),
            PkgAction::Add { dependency } => commands::pkg::add(&dependency),
            PkgAction::Remove { dependency } => commands::pkg::remove(&dependency),
            PkgAction::Update => commands::pkg::update(),
            PkgAction::Install => commands::pkg::install(),
            PkgAction::Lock => commands::pkg::lock(),
            PkgAction::Publish => commands::pkg::publish(),
            PkgAction::Search { query } => commands::pkg::search(&query),
            PkgAction::Info { name } => commands::pkg::info(&name),
            PkgAction::Sync => commands::pkg::sync(),
            PkgAction::Outdated => commands::pkg::outdated(),
            PkgAction::ListLocal { root } => commands::pkg::list_local(root.as_deref()),
        },

        Commands::Lsp => commands::lsp::execute(),

        Commands::Explain { code, list } => {
            if list {
                commands::explain::execute("list")
            } else {
                commands::explain::execute(&code)
            }
        }

        Commands::Version => {
            commands::version::execute();
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    /// Verify that clap parsing works for the build command with defaults.
    #[test]
    fn parse_build_default() {
        let cli = Cli::try_parse_from(["kryos", "build"]).unwrap();
        match cli.command {
            super::Commands::Build { path, release, .. } => {
                assert_eq!(path, ".");
                assert!(!release);
            }
            _ => panic!("expected Build command"),
        }
    }

    /// Verify build with all flags.
    #[test]
    fn parse_build_all_flags() {
        let cli = Cli::try_parse_from([
            "kryos",
            "build",
            "src/main.kry",
            "--release",
            "--target",
            "x86_64-unknown-linux-gnu",
            "-o",
            "out/main",
            "--emit-mir",
            "--verbose",
        ])
        .unwrap();
        match cli.command {
            super::Commands::Build {
                path,
                release,
                target,
                output,
                emit_mir,
                emit_llvm,
                verbose,
                ..
            } => {
                assert_eq!(path, "src/main.kry");
                assert!(release);
                assert_eq!(target.as_deref(), Some("x86_64-unknown-linux-gnu"));
                assert_eq!(output.as_deref(), Some("out/main"));
                assert!(emit_mir);
                assert!(!emit_llvm);
                assert!(verbose);
            }
            _ => panic!("expected Build command"),
        }
    }

    #[test]
    fn parse_run() {
        let cli = Cli::try_parse_from(["kryos", "run", "hello.kry", "--", "arg1", "arg2"]).unwrap();
        match cli.command {
            super::Commands::Run { file, args } => {
                assert_eq!(file, "hello.kry");
                assert_eq!(args, vec!["arg1", "arg2"]);
            }
            _ => panic!("expected Run command"),
        }
    }

    #[test]
    fn parse_check() {
        let cli = Cli::try_parse_from(["kryos", "check"]).unwrap();
        match cli.command {
            super::Commands::Check { path, .. } => assert_eq!(path, "."),
            _ => panic!("expected Check command"),
        }
    }

    #[test]
    fn parse_repl() {
        let cli = Cli::try_parse_from(["kryos", "repl"]).unwrap();
        assert!(matches!(cli.command, super::Commands::Repl));
    }

    #[test]
    fn parse_test_with_filter() {
        let cli = Cli::try_parse_from(["kryos", "test", "--filter", "math"]).unwrap();
        match cli.command {
            super::Commands::Test {
                filter_pos,
                filter,
                exact,
                nocapture,
                format,
                list,
                path: _,
            } => {
                assert_eq!(filter.as_deref(), Some("math"));
                assert!(filter_pos.is_none());
                assert!(!exact);
                assert!(!nocapture);
                assert_eq!(format, "pretty");
                assert!(!list);
            }
            _ => panic!("expected Test command"),
        }
    }

    #[test]
    fn parse_test_positional_filter() {
        let cli = Cli::try_parse_from(["kryos", "test", "math"]).unwrap();
        match cli.command {
            super::Commands::Test { filter_pos, .. } => {
                assert_eq!(filter_pos.as_deref(), Some("math"))
            }
            _ => panic!("expected Test command"),
        }
    }

    #[test]
    fn parse_test_nocapture_exact_json() {
        let cli = Cli::try_parse_from([
            "kryos",
            "test",
            "my_test",
            "--exact",
            "--nocapture",
            "--format",
            "json",
        ])
        .unwrap();
        match cli.command {
            super::Commands::Test {
                filter_pos,
                exact,
                nocapture,
                format,
                ..
            } => {
                assert_eq!(filter_pos.as_deref(), Some("my_test"));
                assert!(exact);
                assert!(nocapture);
                assert_eq!(format, "json");
            }
            _ => panic!("expected Test command"),
        }
    }

    #[test]
    fn parse_test_list() {
        let cli = Cli::try_parse_from(["kryos", "test", "--list"]).unwrap();
        match cli.command {
            super::Commands::Test { list, .. } => assert!(list),
            _ => panic!("expected Test command"),
        }
    }

    #[test]
    fn parse_fmt() {
        let cli = Cli::try_parse_from(["kryos", "fmt", "a.kry", "b.kry"]).unwrap();
        match cli.command {
            super::Commands::Fmt { files, check } => {
                assert_eq!(files, vec!["a.kry", "b.kry"]);
                assert!(!check);
            }
            _ => panic!("expected Fmt command"),
        }
    }

    #[test]
    fn parse_fmt_check() {
        let cli = Cli::try_parse_from(["kryos", "fmt", "--check"]).unwrap();
        match cli.command {
            super::Commands::Fmt { files, check } => {
                assert!(files.is_empty());
                assert!(check);
            }
            _ => panic!("expected Fmt command"),
        }
    }

    #[test]
    fn parse_bindgen() {
        let cli = Cli::try_parse_from(["kryos", "bindgen", "stdio.h", "-o", "stdio.kry"]).unwrap();
        match cli.command {
            super::Commands::Bindgen { header, output } => {
                assert_eq!(header, "stdio.h");
                assert_eq!(output.as_deref(), Some("stdio.kry"));
            }
            _ => panic!("expected Bindgen command"),
        }
    }

    #[test]
    fn parse_pkg_init() {
        let cli = Cli::try_parse_from(["kryos", "pkg", "init", "my-project"]).unwrap();
        match cli.command {
            super::Commands::Pkg {
                action: super::PkgAction::Init { name },
            } => assert_eq!(name.as_deref(), Some("my-project")),
            _ => panic!("expected Pkg Init"),
        }
    }

    #[test]
    fn parse_pkg_add() {
        let cli =
            Cli::try_parse_from(["kryos", "pkg", "add", "github:kryos-lang/serde@^1.0.0"]).unwrap();
        match cli.command {
            super::Commands::Pkg {
                action: super::PkgAction::Add { dependency },
            } => assert_eq!(dependency, "github:kryos-lang/serde@^1.0.0"),
            _ => panic!("expected Pkg Add"),
        }
    }

    #[test]
    fn parse_lsp() {
        let cli = Cli::try_parse_from(["kryos", "lsp"]).unwrap();
        assert!(matches!(cli.command, super::Commands::Lsp));
    }

    #[test]
    fn parse_version() {
        let cli = Cli::try_parse_from(["kryos", "version"]).unwrap();
        assert!(matches!(cli.command, super::Commands::Version));
    }
}
