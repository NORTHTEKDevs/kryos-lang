//! `kryos changelog` — auto-generate a markdown changelog from git tags.
//!
//! For each tag matching `v*.*`, runs `git log <prev>..<tag> --pretty=format:- %s`
//! and emits a Keep-a-Changelog-style section. Useful when prepping a
//! release: redirect to CHANGELOG.md, edit by hand, ship.

use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct ChangelogOptions {
    /// Limit to the latest N tags. 0 = all.
    pub last: Option<usize>,
    /// Show commits between this tag (or HEAD) and the previous tag only.
    pub since: Option<String>,
}

pub fn execute(opts: ChangelogOptions) -> Result<(), String> {
    // Resolve tag list, newest first.
    let out = Command::new("git")
        .args(["tag", "-l", "v*", "--sort=-creatordate"])
        .output()
        .map_err(|e| format!("kryos changelog: git tag: {e}"))?;
    if !out.status.success() {
        return Err("kryos changelog: `git tag` failed".into());
    }
    let tags: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if tags.is_empty() {
        return Err("kryos changelog: no `v*` tags found".into());
    }

    if let Some(since) = &opts.since {
        let prev = find_prev_tag(&tags, since);
        return print_section(prev.as_deref(), since);
    }

    let limit = opts.last.unwrap_or(usize::MAX).max(1);
    println!("# Auto-generated changelog");
    println!();
    println!("Run `kryos changelog > CHANGELOG.gen.md` to regenerate.");
    println!();

    for (i, tag) in tags.iter().take(limit).enumerate() {
        let prev = tags.get(i + 1).map(|s| s.as_str());
        let _ = print_section(prev, tag);
    }
    Ok(())
}

fn find_prev_tag(tags: &[String], current: &str) -> Option<String> {
    let pos = tags.iter().position(|t| t == current)?;
    tags.get(pos + 1).cloned()
}

fn print_section(prev: Option<&str>, tag: &str) -> Result<(), String> {
    let range = match prev {
        Some(p) => format!("{p}..{tag}"),
        None => tag.to_string(),
    };
    let out = Command::new("git")
        .args(["log", &range, "--pretty=format:%s"])
        .output()
        .map_err(|e| format!("kryos changelog: git log {range}: {e}"))?;
    if !out.status.success() {
        return Err(format!("kryos changelog: git log {range} failed"));
    }
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    println!("## {tag}");
    println!();
    if lines.is_empty() {
        println!("_no commits_");
    } else {
        for line in lines {
            println!("- {line}");
        }
    }
    println!();
    Ok(())
}
