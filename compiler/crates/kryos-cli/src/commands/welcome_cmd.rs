//! `kryos welcome` — friendly first-run banner with example workflow.

pub fn execute() -> Result<(), String> {
    let v = env!("CARGO_PKG_VERSION");
    println!();
    println!("\x1b[1;36m  ╔════════════════════════════════════════╗\x1b[0m");
    println!("\x1b[1;36m  ║\x1b[0m  \x1b[1mWelcome to Kryos\x1b[0m  \x1b[2mv{v:<22}\x1b[0m \x1b[1;36m║\x1b[0m");
    println!("\x1b[1;36m  ╚════════════════════════════════════════╝\x1b[0m");
    println!();
    println!("A capability-safe, ownership-aware systems language.");
    println!();
    println!("\x1b[1mFirst steps\x1b[0m");
    println!("  \x1b[36mkryos new myapp\x1b[0m         scaffold a new project");
    println!("  \x1b[36mcd myapp\x1b[0m");
    println!("  \x1b[36mkryos run src/main.kry\x1b[0m  compile + execute");
    println!("  \x1b[36mkryos test\x1b[0m              run tests in tests/");
    println!();
    println!("\x1b[1mUseful commands\x1b[0m");
    println!("  \x1b[36mkryos check\x1b[0m            fast type-check");
    println!("  \x1b[36mkryos build --release\x1b[0m  optimized AOT binary via LLVM");
    println!("  \x1b[36mkryos eval '<expr>'\x1b[0m    one-liner evaluator");
    println!("  \x1b[36mkryos lint\x1b[0m             style + correctness lints");
    println!("  \x1b[36mkryos audit\x1b[0m            secret-pattern + capability scan");
    println!("  \x1b[36mkryos doctor\x1b[0m           diagnose your toolchain");
    println!();
    println!("\x1b[1mLearn more\x1b[0m");
    println!("  docs/learn/README.md         30-minute tour");
    println!("  docs/learn/cookbook/         19 task-oriented recipes");
    println!("  docs/commands.md             full command reference");
    println!("  docs/STDLIB.md               stdlib reference");
    println!();
    println!("Run \x1b[36mkryos --help\x1b[0m for the full command list.");
    println!();
    Ok(())
}
