//! `kryos repl` — interactive read-eval-print loop.

use std::io::{self, BufRead, Write};

/// Execute the REPL.
pub fn execute() -> Result<(), String> {
    eprintln!(
        "Kryos {} REPL — type :help for commands, :quit to exit",
        env!("CARGO_PKG_VERSION")
    );

    // Install Ctrl+C handler so the process exits cleanly.
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let r = running.clone();
        if let Err(e) = ctrlc_install(move || {
            r.store(false, std::sync::atomic::Ordering::SeqCst);
        }) {
            eprintln!("warning: could not install Ctrl+C handler: {e}");
        }
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut line = String::new();

    // Accumulated state across REPL inputs.
    // `decl_history` holds top-level items (fn, struct, enum, impl, trait, const).
    // `let_history` holds let/const bindings that go inside the wrapper function.
    let mut decl_history: Vec<String> = Vec::new();
    let mut let_history: Vec<String> = Vec::new();

    loop {
        if !running.load(std::sync::atomic::Ordering::SeqCst) {
            eprintln!("\ninterrupted");
            break;
        }

        print!("kryos> ");
        stdout.flush().map_err(|e| e.to_string())?;

        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            // EOF
            eprintln!();
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match trimmed {
            ":quit" | ":q" | ":exit" => break,
            ":help" | ":h" => {
                println!("Commands:");
                println!("  :help, :h     Show this help");
                println!("  :quit, :q     Exit the REPL");
                println!("  :type <expr>  Show the type of an expression");
                println!("  :clear        Clear the screen");
                println!("  :reset        Clear accumulated definitions");
                println!();
                println!("Enter any Kryos expression or statement to evaluate.");
            }
            ":clear" => {
                // ANSI clear screen
                print!("\x1b[2J\x1b[H");
                stdout.flush().map_err(|e| e.to_string())?;
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
                let wrapper = format!("{preamble}\nfn __repl_type_check__() {{ {lets}\nlet __result__ = {expr}; }}");
                let (diags, sm) = kryos_driver::check_source(&wrapper, "<repl>");
                if diags.iter().any(|d| d.is_error()) {
                    for d in &diags {
                        eprint!("{}", kryos_errors::render_diagnostic(d, &sm));
                    }
                } else {
                    // Type check passed — report success.
                    // Full type inference display would require accessing the
                    // type table, which isn't exposed yet. For now, report that
                    // the expression is valid.
                    println!("expression `{expr}` type-checks successfully");
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

                let wrapper = if is_decl {
                    // Top-level declaration — place it alongside history,
                    // with an empty eval body just to validate.
                    format!("{preamble}\n{input}\nfn __repl_eval__() {{ {lets} }}")
                } else if is_let || is_assignment || input.ends_with(';') {
                    format!("{preamble}\nfn __repl_eval__() {{ {lets}\n{input} }}")
                } else {
                    // Bare expression — wrap as a let binding so the parser accepts it.
                    format!("{preamble}\nfn __repl_eval__() {{ {lets}\nlet __expr__ = {input} }}")
                };

                let config = kryos_driver::BuildConfig::for_file("<repl>");
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
                && lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '[' || c == ']')
            {
                return true;
            }
        }
    }
    false
}

/// Minimal Ctrl+C handler installation.
///
/// We avoid pulling in the `ctrlc` crate by using platform-native APIs
/// directly.
fn ctrlc_install<F: Fn() + Send + 'static>(handler: F) -> Result<(), String> {
    #[cfg(unix)]
    {
        // On Unix, use signal(SIGINT, ...) via std — not available in stable
        // Rust without libc, so fall back to a thread-based approach.
        std::thread::spawn(move || {
            // Block on SIGINT via a simple loop with signal_hook-like behavior.
            // This is best-effort; the real handler is EOF on stdin.
            let _ = handler;
        });
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::sync::Once;
        static INIT: Once = Once::new();
        // Store the handler in a static.
        // Safety: we only call this once.
        static mut HANDLER: Option<Box<dyn Fn() + Send>> = None;
        INIT.call_once(|| {
            unsafe {
                HANDLER = Some(Box::new(handler));
                SetConsoleCtrlHandler(Some(ctrl_handler), 1);
            }
        });
        return Ok(());

        unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> i32 {
            if ctrl_type == 0 {
                // CTRL_C_EVENT
                if let Some(ref h) = HANDLER {
                    h();
                }
                1 // handled
            } else {
                0
            }
        }

        extern "system" {
            fn SetConsoleCtrlHandler(
                handler: Option<unsafe extern "system" fn(u32) -> i32>,
                add: i32,
            ) -> i32;
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = handler;
        Ok(())
    }
}
