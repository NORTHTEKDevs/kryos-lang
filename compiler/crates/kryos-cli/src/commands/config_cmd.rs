//! `kryos config get|set|list|unset` — user-level configuration.
//!
//! Config lives at `~/.config/kryos/config.toml` (XDG) or
//! `%APPDATA%\kryos\config.toml` (Windows). Simple `key = value` lines —
//! no nested tables. Keys understood at v4.5:
//!
//! - `default_backend` — "cranelift" (default) or "llvm"
//! - `default_opt_level` — "debug" or "release"
//! - `color` — "auto" (default), "always", or "never"

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum ConfigAction {
    Get { key: String },
    Set { key: String, value: String },
    Unset { key: String },
    List,
    Path,
}

pub fn execute(action: ConfigAction) -> Result<(), String> {
    let path = config_path()?;
    let mut data = load(&path);

    match action {
        ConfigAction::Path => {
            println!("{}", path.display());
        }
        ConfigAction::List => {
            if data.is_empty() {
                println!("(no config keys set; file: {})", path.display());
            } else {
                for (k, v) in &data {
                    println!("{k} = {v}");
                }
            }
        }
        ConfigAction::Get { key } => {
            match data.get(&key) {
                Some(v) => println!("{v}"),
                None => return Err(format!("kryos config: key '{key}' not set")),
            }
        }
        ConfigAction::Set { key, value } => {
            if !is_valid_key(&key) {
                return Err(format!(
                    "kryos config: '{key}' is not a recognized key — try: default_backend, default_opt_level, color"
                ));
            }
            data.insert(key.clone(), value.clone());
            save(&path, &data)?;
            eprintln!("\x1b[32mset\x1b[0m {key} = {value}");
        }
        ConfigAction::Unset { key } => {
            if data.remove(&key).is_some() {
                save(&path, &data)?;
                eprintln!("\x1b[33munset\x1b[0m {key}");
            } else {
                eprintln!("(key '{key}' was not set)");
            }
        }
    }
    Ok(())
}

fn config_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("KRYOS_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let base = if cfg!(windows) {
        std::env::var("APPDATA").map(PathBuf::from)
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
    }
    .map_err(|e| format!("kryos config: cannot resolve config dir: {e}"))?;
    Ok(base.join("kryos").join("config.toml"))
}

fn load(path: &std::path::Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return out;
    };
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
            continue;
        }
        if let Some(eq) = t.find('=') {
            let k = t[..eq].trim().to_string();
            let v = t[eq + 1..].trim().trim_matches('"').to_string();
            if !k.is_empty() {
                out.insert(k, v);
            }
        }
    }
    out
}

fn save(path: &std::path::Path, data: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("kryos config: mkdir {}: {e}", parent.display()))?;
    }
    let mut text = String::from("# Kryos user config — managed by `kryos config`\n");
    for (k, v) in data {
        text.push_str(&format!("{k} = \"{v}\"\n"));
    }
    fs::write(path, text).map_err(|e| format!("kryos config: write {}: {e}", path.display()))
}

fn is_valid_key(k: &str) -> bool {
    matches!(
        k,
        "default_backend" | "default_opt_level" | "color"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_parses_kv_lines() {
        let tmp = std::env::temp_dir().join(format!("kryos_cfg_test_{}.toml", std::process::id()));
        fs::write(&tmp, "# comment\nfoo = \"bar\"\nbaz=qux\n").unwrap();
        let map = load(&tmp);
        assert_eq!(map.get("foo"), Some(&"bar".to_string()));
        assert_eq!(map.get("baz"), Some(&"qux".to_string()));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn save_round_trips() {
        let mut m = BTreeMap::new();
        m.insert("default_backend".to_string(), "llvm".to_string());
        let tmp = std::env::temp_dir().join(format!("kryos_cfg_save_{}.toml", std::process::id()));
        save(&tmp, &m).unwrap();
        let loaded = load(&tmp);
        assert_eq!(loaded.get("default_backend"), Some(&"llvm".to_string()));
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn valid_key_check() {
        assert!(is_valid_key("default_backend"));
        assert!(!is_valid_key("random_unknown_key"));
    }
}
