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
        /// Filter test names by substring
        #[arg(long)]
        filter: Option<String>,
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

        /// Output directory for markdown files (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
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
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Build {
            path,
            release,
            target,
            output,
            emit_mir,
            emit_llvm,
            verbose,
            skip_ownership,
        } => commands::build::execute(
            &path, release, target.as_deref(), output.as_deref(), emit_mir, emit_llvm, verbose,
            skip_ownership,
        ),

        Commands::Run { file, args } => commands::run::execute(&file, &args),

        Commands::Check { path, skip_ownership } => commands::check::execute(&path, skip_ownership),

        Commands::Repl => commands::repl::execute(),

        Commands::Test { filter } => commands::test_cmd::execute(filter.as_deref()),

        Commands::Fmt { files, check } => commands::fmt::execute(&files, check),

        Commands::Doc { files, output } => {
            commands::doc::execute(&files, output.as_deref())
        }

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
        },

        Commands::Lsp => commands::lsp::execute(),

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
    use clap::Parser;
    use super::Cli;

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
            "kryos", "build", "src/main.kry",
            "--release", "--target", "x86_64-unknown-linux-gnu",
            "-o", "out/main", "--emit-mir", "--verbose",
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
            super::Commands::Test { filter } => assert_eq!(filter.as_deref(), Some("math")),
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
            Cli::try_parse_from(["kryos", "pkg", "add", "github:kryos-lang/serde@^1.0.0"])
                .unwrap();
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
