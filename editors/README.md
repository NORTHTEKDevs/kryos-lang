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

The Zed extension references a tree-sitter grammar at
`editors/tree-sitter-kryos/`. That grammar is a work-in-progress; for
the v2.3 release window the Zed extension uses a minimal language
configuration that delegates highlighting decisions to the LSP's
semantic tokens. The full tree-sitter grammar will land in a follow-up.
