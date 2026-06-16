//! `kryos repl` — interactive read-eval-print loop.
//!
//! Line editing, Up/Down history recall, and Tab completion are provided by
//! `rustyline`. Persistent history is kept in `~/.kryos_history` using the
//! same plain-line format as before: it is loaded on startup (and seeded into
//! rustyline so the arrow keys recall prior sessions) and each accepted input
//! line is appended immediately.

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};

/// Kryos keywords + common builtins, offered as Tab completions at the prompt.
const COMPLETION_WORDS: &[&str] = &[
    // keywords
    "fn", "let", "mut", "if", "elif", "else", "while", "for", "loop", "match",
    "struct", "enum", "impl", "trait", "return", "break", "continue", "in",
    "use", "pub", "const", "type", "extern", "actor", "throw", "try", "catch",
    "true", "false", "and", "or", "not", "as",
    // option/result constructors
    "Some", "None", "Ok", "Err", "Option", "Result",
    // common builtins
    "println", "print", "len", "to_string", "push", "pop", "contains",
    "parse_int", "parse_float", "args", "env_get", "file_read", "file_write",
    "file_exists", "create_dir", "substr", "char_code", "sqrt", "pow", "sin",
    "cos", "abs", "min", "max", "exit",
    // REPL meta-commands
    ":help", ":quit", ":type", ":clear", ":reset", ":history", ":history-clear",
];

/// rustyline `Helper` providing Kryos keyword/builtin Tab completion.
/// Hinting, highlighting, and validation use the trait defaults.
struct KryosHelper;

impl Completer for KryosHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        // Find the start of the identifier (or `:command`) under the cursor.
        let start = line[..pos]
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == ':'))
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        let prefix = &line[start..pos];
        if prefix.is_empty() {
            return Ok((start, Vec::new()));
        }
        let candidates = COMPLETION_WORDS
            .iter()
            .filter(|w| w.starts_with(prefix))
            .map(|w| Pair {
                display: (*w).to_string(),
                replacement: (*w).to_string(),
            })
            .collect();
        Ok((start, candidates))
    }
}

impl Hinter for KryosHelper {
    type Hint = String;
}
impl Highlighter for KryosHelper {}
impl Validator for KryosHelper {}
impl Helper for KryosHelper {}

/// Execute the REPL.
pub fn execute() -> Result<(), String> {
    eprintln!(
        "Kryos {} REPL — type :help for commands, :quit to exit",
        env!("CARGO_PKG_VERSION")
    );

    // Persistent history: load from ~/.kryos_history on startup, append each
    // accepted input line during the session. Lines starting with `:` (REPL
    // meta-commands) are recorded too — useful context when re-reading.
    let history_path = history_file_path();
    let mut session_history: Vec<String> = read_history(&history_path);

    let mut rl: Editor<KryosHelper, DefaultHistory> =
        Editor::new().map_err(|e| e.to_string())?;
    rl.set_helper(Some(KryosHelper));
    // Seed rustyline's in-memory history so Up/Down recalls prior sessions.
    for entry in &session_history {
        let _ = rl.add_history_entry(entry.as_str());
    }

    // Accumulated state across REPL inputs.
    // `decl_history` holds top-level items (fn, struct, enum, impl, trait, const).
    // `let_history` holds let/const bindings that go inside the wrapper function.
    let mut decl_history: Vec<String> = Vec::new();
    let mut let_history: Vec<String> = Vec::new();

    loop {
        let mut line = match rl.readline("kryos> ") {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                eprintln!("interrupted");
                break;
            }
            Err(ReadlineError::Eof) => {
                eprintln!();
                break;
            }
            Err(e) => return Err(e.to_string()),
        };

        // Multi-line input: keep reading while brackets are unclosed.
        while bracket_depth(line.trim_end()) > 0 {
            match rl.readline(".... ") {
                Ok(more) => {
                    line.push('\n');
                    line.push_str(&more);
                }
                Err(_) => break,
            }
        }

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        // Record every accepted input in the persistent history (on-disk file
        // in the original plain-line format, the :history list, and rustyline's
        // in-memory history for arrow-key recall).
        session_history.push(trimmed.clone());
        append_history(&history_path, &trimmed);
        let _ = rl.add_history_entry(trimmed.as_str());

        match trimmed.as_str() {
            ":quit" | ":q" | ":exit" => break,
            ":help" | ":h" => {
                println!("Commands:");
                println!("  :help, :h       Show this help");
                println!("  :quit, :q       Exit the REPL");
                println!("  :type <expr>    Show the type of an expression");
                println!("  :clear          Clear the screen");
                println!("  :reset          Clear accumulated definitions");
                println!("  :history        Show the persistent input history");
                println!("  :history-clear  Wipe the on-disk history file");
                println!();
                println!("Line editing: arrow keys recall history, Tab completes");
                println!("Kryos keywords and builtins.");
                println!();
                println!("Enter any Kryos expression or statement to evaluate.");
            }
            ":history" => {
                if session_history.is_empty() {
                    println!("(history empty)");
                } else {
                    for (i, h) in session_history.iter().enumerate() {
                        println!("{:>4}: {}", i + 1, h);
                    }
                }
            }
            ":history-clear" => {
                session_history.clear();
                let _ = rl.clear_history();
                let _ = std::fs::remove_file(&history_path);
                println!("(history cleared)");
            }
            ":clear" => {
                // ANSI clear screen
                print!("\x1b[2J\x1b[H");
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
            }
            ":reset" => {
                decl_history.clear();
                let_history.clear();
                println!("(state cleared)");
            }
            input if input.starts_with(":type ") => {
                let expr = &input[6..];
                // Wrap in a function with accumulated state so prior definitions are visible.
                let preamble = decl_history.join("\n");
                let lets = let_history.join("\n");
                let wrapper = format!(
                    "{preamble}\nfn __repl_type_check__() {{ {lets}\nlet __result__ = {expr}\n}}"
                );
                let mut config = kryos_driver::BuildConfig::for_file("<repl>");
                config.output_type = kryos_driver::OutputType::Mir;
                let result = kryos_driver::compile_source(&wrapper, "<repl>", &config);
                if !result.success {
                    for d in &result.diagnostics {
                        eprint!("{}", kryos_errors::render_diagnostic(d, &result.source_map));
                    }
                } else if let Some(ref mir) = result.mir {
                    // Look up __result__ in the MIR locals of __repl_type_check__.
                    let ty_str = mir
                        .functions
                        .iter()
                        .find(|f| f.name == "__repl_type_check__")
                        .and_then(|f| {
                            f.locals
                                .iter()
                                .find(|l| l.name.as_deref() == Some("__result__"))
                        })
                        .map(|l| l.ty.to_string())
                        .unwrap_or_else(|| "?".to_string());
                    println!("{expr} : {ty_str}");
                } else {
                    println!("{expr} : ?");
                }
            }
            input => {
                // Classify input: declaration (top-level) vs let-binding vs
                // expression/statement. The REPL persists state by accumulating
                // source strings and re-compiling them each iteration.
                let is_decl = input.starts_with("fn ")
                    || input.starts_with("struct ")
                    || input.starts_with("enum ")
                    || input.starts_with("impl ")
                    || input.starts_with("trait ")
                    || input.starts_with("const ")
                    || input.starts_with("use ")
                    || input.starts_with("type ")
                    || input.starts_with("extern ")
                    || input.starts_with("actor ")
                    || input.starts_with("pub ")
                    || input.starts_with("@");
                let is_let = input.starts_with("let ");

                // Detect assignment statements (e.g. `x = 10`, `arr[0] = 5`).
                // These must be persisted so mutations carry across REPL lines.
                let is_assignment = !is_let && !is_decl && is_assignment_stmt(input);

                // Build the source with accumulated state.
                let preamble = decl_history.join("\n");
                let lets = let_history.join("\n");

                let mut config = kryos_driver::BuildConfig::for_file("<repl>");
                config.output_type = kryos_driver::OutputType::Mir;

                let wrapper = if is_decl {
                    // Top-level declaration — place it alongside history,
                    // with an empty eval body just to validate.
                    format!("{preamble}\n{input}\nfn __repl_eval__() {{ {lets} }}")
                } else if is_let || is_assignment || input.ends_with(';') {
                    format!("{preamble}\nfn __repl_eval__() {{ {lets}\n{input} }}")
                } else {
                    // Bare expression — try to auto-print via println(to_string(...)).
                    let print_wrapper = format!(
                        "{preamble}\nfn __repl_eval__() {{ {lets}\nprintln(to_string({input})) }}"
                    );
                    let probe = kryos_driver::compile_source(&print_wrapper, "<repl>", &config);
                    if probe.success {
                        print_wrapper
                    } else {
                        // Fall back to running silently (e.g. void call or side-effecting stmt).
                        format!("{preamble}\nfn __repl_eval__() {{ {lets}\n{input} }}")
                    }
                };

                let result = kryos_driver::compile_source(&wrapper, "<repl>", &config);

                if !result.success {
                    for d in &result.diagnostics {
                        let rendered = kryos_errors::render_diagnostic(d, &result.source_map);
                        eprint!("{rendered}");
                    }
                } else {
                    // Compilation succeeded — accumulate into history so
                    // subsequent lines see these definitions/bindings.
                    if is_decl {
                        decl_history.push(input.to_string());
                    } else if is_let || is_assignment {
                        let_history.push(input.to_string());
                    }

                    if let Some(ref mir) = result.mir {
                        // JIT compile ALL functions so cross-function calls work.
                        let backend = kryos_codegen_cranelift::CraneliftBackend::new();
                        match backend.jit_compile_module(mir) {
                            Ok(ptrs) => {
                                if let Some(&ptr) = ptrs.get("__repl_eval__") {
                                    // Safety: `ptr` points to JIT-compiled code with the
                                    // signature `fn()` produced by the Cranelift backend.
                                    let f: fn() = unsafe { std::mem::transmute(ptr) };
                                    f();
                                } else {
                                    eprintln!("(internal: __repl_eval__ not found in MIR)");
                                }
                            }
                            Err(e) => {
                                eprintln!("JIT error: {e}");
                            }
                        }
                    } else {
                        println!("(no output)");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Count net open bracket depth in a string, ignoring brackets inside string literals.
/// Returns 0 if brackets are balanced or over-closed.
fn bracket_depth(s: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for ch in s.chars() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            _ => {}
        }
    }
    depth.max(0)
}

/// Detect simple assignment statements like `x = 10`, `point.x = 5`, etc.
/// Returns false for comparisons (`==`, `!=`, `<=`, `>=`) and `let` bindings.
fn is_assignment_stmt(input: &str) -> bool {
    // Find the first `=` that isn't part of `==`, `!=`, `<=`, `>=`, `=>`.
    let bytes = input.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'=' {
            // Skip compound operators
            if i > 0 && matches!(bytes[i - 1], b'!' | b'<' | b'>' | b'=') {
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                continue; // `=>`
            }
            // The left-hand side should look like an identifier or field access.
            let lhs = input[..i].trim();
            if !lhs.is_empty()
                && lhs
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '[' || c == ']')
            {
                return true;
            }
        }
    }
    false
}

// ─── Persistent history ──────────────────────────────────────────────────

fn history_file_path() -> std::path::PathBuf {
    let dir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    dir.join(".kryos_history")
}

fn read_history(path: &std::path::Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => s.lines().map(|l| l.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

fn append_history(path: &std::path::Path, line: &str) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}
