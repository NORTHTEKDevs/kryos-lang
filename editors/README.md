# Kryos Editor Extensions

This directory holds editor integrations for the Kryos language.

| Editor | Folder | Status | Distribution |
|---|---|---|---|
| Visual Studio Code | [`vscode/`](./vscode) | Marketplace-ready | `vsce package` |
| Zed | [`zed/`](./zed) | Dev-extension ready | `cargo build --release --target wasm32-wasi` |

All extensions launch `kryos lsp` as the language server, so once the
Kryos toolchain is on your `PATH` the editor experience is unified
across editors.

## Tree-sitter grammar

A tree-sitter grammar for Kryos is planned (`editors/tree-sitter-kryos/`)
but not yet checked in. The Zed extension uses a minimal language
configuration that delegates highlighting decisions to the LSP's
semantic tokens, which works without a tree-sitter grammar. Status
tracked in v3.x follow-ups.
