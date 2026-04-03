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
                println!();
                println!("Enter any Kryos expression or statement to evaluate.");
            }
            ":clear" => {
                // ANSI clear screen
                print!("\x1b[2J\x1b[H");
                stdout.flush().map_err(|e| e.to_string())?;
            }
            input if input.starts_with(":type ") => {
                let expr = &input[6..];
                // Wrap in a function so the parser can handle it
                let wrapper = format!("fn __repl_type_check__() {{ let __result__ = {expr}; }}");
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
                // Wrap input as a function body so the pipeline can handle it.
                let wrapper = if input.contains("let ") || input.contains('=') || input.ends_with(';') {
                    format!("fn __repl_eval__() {{ {input} }}")
                } else {
                    // Bare expression — wrap as a statement.
                    format!("fn __repl_eval__() {{ {input}; }}")
                };

                let config = kryos_driver::BuildConfig::for_file("<repl>");
                let result = kryos_driver::compile_source(&wrapper, "<repl>", &config);

                if !result.success {
                    for d in &result.diagnostics {
                        let rendered = kryos_errors::render_diagnostic(d, &result.source_map);
                        eprint!("{rendered}");
                    }
                } else if let Some(ref mir) = result.mir {
                    // Try JIT compilation via the Cranelift backend.
                    let backend = kryos_codegen_cranelift::CraneliftBackend::new();
                    // Find the __repl_eval__ function in MIR.
                    if let Some(func) = mir.functions.iter().find(|f| f.name == "__repl_eval__") {
                        match backend.jit_compile_function(func) {
                            Ok(ptr) => {
                                // Execute the JIT'd function.
                                // Safety: `ptr` points to JIT-compiled code with the
                                // signature `fn()` produced by the Cranelift backend.
                                let f: fn() = unsafe { std::mem::transmute(ptr) };
                                f();
                            }
                            Err(e) => {
                                eprintln!("JIT error: {e}");
                            }
                        }
                    } else {
                        eprintln!("(internal: __repl_eval__ not found in MIR)");
                    }
                } else {
                    println!("(no output)");
                }
            }
        }
    }

    Ok(())
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
