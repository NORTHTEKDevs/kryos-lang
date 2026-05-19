# Kryos Editor Extensions

This directory holds editor integrations for the Kryos language. The LSP
server (`kryos lsp`) provides a comprehensive feature set as of v3.15:

| LSP capability | Method |
| --- | --- |
| Diagnostics (on save + on change) | `textDocument/publishDiagnostics` |
| Hover (symbol info + doc comments) | `textDocument/hover` |
| Completion (with member access after `.`) | `textDocument/completion` |
| Go to definition (cross-file) | `textDocument/definition` |
| Find references | `textDocument/references` |
| Rename symbol (workspace edit) | `textDocument/rename` |
| Document outline | `textDocument/documentSymbol` |
| Workspace symbol search (fuzzy) | `workspace/symbol` |
| Document highlight (read vs write) | `textDocument/documentHighlight` |
| Code folding (braces + comments) | `textDocument/foldingRange` |
| Format on save (delegates to `kryos fmt`) | `textDocument/formatting` |
| Signature help while typing args | `textDocument/signatureHelp` |
| Inline type hints + parameter names | `textDocument/inlayHint` |
| Quick-fix code actions (did-you-mean) | `textDocument/codeAction` |
| Semantic tokens (accurate highlighting) | `textDocument/semanticTokens/full` |


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
