//! `kryos lsp` — start the Kryos language server.

/// Execute the LSP server.
pub fn execute() -> Result<(), String> {
    let mut server = kryos_lsp::LspServer::new();
    server.run();
    Ok(())
}
