//! `kryos pack` — build a release archive for the current project.
//!
//! Produces `<name>-<version>.tar` (uncompressed tar) containing:
//!   src/, kryos.toml, README.md (if present), LICENSE (if present),
//!   tests/ (if present), examples/ (if present).
//!
//! Skips: target/, *.kry.lock, hidden dirs, node_modules, the archive
//! itself. Designed to be deterministic so two runs on the same tree
//! produce identical bytes (sorted entries, mtime zeroed).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use kryos_package::Manifest;

#[derive(Debug, Clone, Default)]
pub struct PackOptions {
    pub path: Option<String>,
    pub output: Option<String>,
}

pub fn execute(opts: PackOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("kryos pack: {e}"))?,
    };
    let manifest_path = root.join("kryos.toml");
    if !manifest_path.is_file() {
        return Err("kryos pack: no kryos.toml in target directory".into());
    }
    let manifest = Manifest::from_file(&manifest_path)?;
    let archive_name = format!("{}-{}.tar", manifest.package.name, manifest.package.version);
    let output_path = opts
        .output
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(&archive_name));

    let mut files: Vec<PathBuf> = Vec::new();
    files.push(manifest_path.clone());
    for sub in &["src", "tests", "examples"] {
        let dir = root.join(sub);
        if dir.is_dir() {
            collect_files(&dir, &mut files);
        }
    }
    for name in &["README.md", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "CHANGELOG.md"] {
        let p = root.join(name);
        if p.is_file() {
            files.push(p);
        }
    }
    files.sort();

    let mut out = File::create(&output_path)
        .map_err(|e| format!("kryos pack: create {}: {e}", output_path.display()))?;

    for f in &files {
        let rel = f.strip_prefix(&root).map_err(|e| format!("strip: {e}"))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let data = std::fs::read(f).map_err(|e| format!("read {}: {e}", f.display()))?;
        write_tar_entry(&mut out, &rel_str, &data)
            .map_err(|e| format!("tar write: {e}"))?;
    }
    // Two zero blocks = end-of-archive.
    out.write_all(&[0u8; 1024]).map_err(|e| format!("tar trailer: {e}"))?;

    eprintln!(
        "\x1b[32mpacked\x1b[0m {} ({} files, {} bytes)",
        output_path.display(),
        files.len(),
        std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0)
    );
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() {
            let skip_ext = p.extension().is_some_and(|x| x == "tar" || x == "tar.gz");
            if !skip_ext {
                out.push(p);
            }
        } else if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            collect_files(&p, out);
        }
    }
}

/// Minimal USTAR/POSIX tar entry writer. Deterministic (mtime = 0).
fn write_tar_entry(w: &mut impl Write, name: &str, data: &[u8]) -> std::io::Result<()> {
    let mut header = [0u8; 512];

    // Name (offset 0, len 100).
    let name_bytes = name.as_bytes();
    let n = name_bytes.len().min(100);
    header[..n].copy_from_slice(&name_bytes[..n]);

    // Mode (offset 100, len 8).
    write_octal(&mut header[100..108], 0o644, 7);
    // UID (108), GID (116) — both 0.
    write_octal(&mut header[108..116], 0, 7);
    write_octal(&mut header[116..124], 0, 7);
    // Size (124, len 12).
    write_octal(&mut header[124..136], data.len() as u64, 11);
    // Mtime (136, len 12).
    write_octal(&mut header[136..148], 0, 11);

    // Checksum placeholder (148, len 8) — spaces while computing.
    for b in &mut header[148..156] {
        *b = b' ';
    }

    // Typeflag (156): '0' = regular file.
    header[156] = b'0';

    // USTAR magic + version.
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");

    // Compute + write checksum.
    let sum: u64 = header.iter().map(|&b| b as u64).sum();
    write_octal(&mut header[148..156], sum, 6);
    header[154] = 0;
    header[155] = b' ';

    w.write_all(&header)?;
    w.write_all(data)?;
    // Pad to 512.
    let pad = (512 - (data.len() % 512)) % 512;
    if pad > 0 {
        w.write_all(&vec![0u8; pad])?;
    }
    Ok(())
}

fn write_octal(buf: &mut [u8], mut value: u64, digits: usize) {
    // Tar fields are left-zero-padded ASCII octal followed by a NUL.
    let dst_len = buf.len().min(digits);
    // Fill the digit range with ASCII '0'.
    for b in buf[..dst_len].iter_mut() {
        *b = b'0';
    }
    // Then write digits right-to-left from the end of the digit range.
    let mut i = dst_len;
    while value > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 8) as u8;
        value /= 8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_writes_known_value() {
        let mut buf = [0u8; 8];
        write_octal(&mut buf, 0o644, 7);
        assert_eq!(&buf[..7], b"0000644");
        assert_eq!(buf[7], 0);  // NUL terminator preserved
    }
}
