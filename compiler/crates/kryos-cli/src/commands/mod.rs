//! Command implementations for the `kryos` CLI.

pub mod audit_cmd;
pub mod bench_cmd;
pub mod manifest_cmd;
pub mod bindgen;
pub mod build;
pub mod changelog_cmd;
pub mod cheat_cmd;
pub mod check;
pub mod clean_cmd;
pub mod config_cmd;
pub mod coverage_cmd;
pub mod diff_cmd;
pub mod doc_serve_cmd;
pub mod doctor_cmd;
pub mod eval_cmd;
pub mod info_cmd;
pub mod lint_cmd;
pub mod new_cmd;
pub mod pack_cmd;
pub mod profile_cmd;
pub mod trace_cmd;
pub mod tree_cmd;
pub mod watch_cmd;
pub mod dap;
pub mod doc;
pub mod explain;
pub mod fmt;
pub mod caps_badge;
pub mod lsp;
pub mod pkg;
pub mod repl;
pub mod run;
pub mod test_cmd;
pub mod version;
pub mod welcome_cmd;
pub mod workspace_cmd;

/// Most diagnostics `check`, `run` and `build` print. A cascade after one real
/// error can run to tens of thousands of lines (an abandoned deep parse
/// emitted 24,873 identical follow-on errors, 5.9 MB under `run`); nobody
/// reads past the first few. Error and warning COUNTS still cover all of them.
pub const MAX_SHOWN_DIAGNOSTICS: usize = 100;

/// The trailing line that tells the user how many diagnostics were not
/// printed, or `None` when everything was shown.
pub fn hidden_diagnostics_note(total: usize) -> Option<String> {
    let hidden = total.saturating_sub(MAX_SHOWN_DIAGNOSTICS);
    if hidden == 0 {
        return None;
    }
    Some(format!(
        "... {hidden} more diagnostic{} not shown",
        if hidden == 1 { "" } else { "s" }
    ))
}
