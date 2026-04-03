//! `kryos test` — discover and run tests in a Kryos project.

use std::path::Path;

/// Execute the test command.
pub fn execute(filter: Option<&str>) -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    if !manifest_path.exists() {
        return Err(
            "no kryos.toml found — run `kryos pkg init` or navigate to a project directory"
                .to_string(),
        );
    }

    // TODO: Once the driver supports test discovery and execution:
    // 1. Parse kryos.toml to find source roots.
    // 2. Scan for `#[test]` functions.
    // 3. Compile with test harness enabled.
    // 4. Execute each test, collecting pass/fail/skip.
    // 5. Print summary.

    if let Some(f) = filter {
        eprintln!("kryos test: filtering tests matching `{f}`");
    }

    eprintln!("kryos test: test runner not yet connected to compiler pipeline");
    eprintln!("  hint: the test framework will discover #[test] functions once");
    eprintln!("        the driver pipeline is fully wired up.");

    Ok(())
}
