// Kryos Zed extension — launches `kryos lsp` as the language server.
//
// Build (from this folder):
//   cargo build --release --target wasm32-wasi
//
// The compiled extension is then installable via:
//   Zed → Extensions → Install Dev Extension → select this folder

use zed_extension_api as zed;

struct KryosExtension;

impl zed::Extension for KryosExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        // Prefer a `kryos` binary on PATH, then fall back to the worktree's
        // local toolchain directory (`./target/release/kryos`) if present.
        let path = worktree
            .which("kryos")
            .or_else(|| {
                let local = worktree.root_path() + "/compiler/target/release/kryos";
                if std::path::Path::new(&local).exists() {
                    Some(local)
                } else {
                    None
                }
            })
            .ok_or_else(|| "`kryos` binary not found on PATH or in workspace".to_string())?;

        Ok(zed::Command {
            command: path,
            args: vec!["lsp".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(KryosExtension);
