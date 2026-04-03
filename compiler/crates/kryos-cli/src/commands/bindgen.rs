//! `kryos bindgen` — generate Kryos FFI bindings from C headers.

use std::path::Path;

/// Execute the bindgen command.
pub fn execute(header: &str, output: Option<&str>) -> Result<(), String> {
    let path = Path::new(header);
    if !path.exists() {
        return Err(format!("header file not found: {header}"));
    }

    let source =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read `{header}`: {e}"))?;

    let kryos_source = kryos_bindgen::generate_bindings(&source).map_err(|errors| {
        let joined = errors.join("\n  ");
        format!("bindgen failed:\n  {joined}")
    })?;

    match output {
        Some(out_path) => {
            std::fs::write(out_path, &kryos_source)
                .map_err(|e| format!("failed to write `{out_path}`: {e}"))?;
            eprintln!("kryos bindgen: wrote {out_path}");
        }
        None => {
            print!("{kryos_source}");
        }
    }

    Ok(())
}
