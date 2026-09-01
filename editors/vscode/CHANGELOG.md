# Changelog

All notable changes to the **Kryos for VS Code** extension are documented here.

## 1.0.0 — 2026-08-31

- Version realigned onto the toolchain's 1.0 line so `publish-vscode.yml`
  ships the extension alongside the `v1.0.0` toolchain release. No
  functional change to the extension itself in this bump.
- Compatible with Kryos toolchain **v1.0.0** (the `v2.x`/`v4.x` numbers
  named in the 0.4.0 entry below are from the abandoned pre-recalibration
  scheme -- see VERSIONING.md).

## 0.4.0 — 2026-05

- Marketplace-ready packaging: license, icon, categories, keywords
- Bundled LSP client wiring stable on `kryos lsp` stdio transport
- Compatible with Kryos toolchain **v2.3.0+**

## 0.3.0 — 2026-04

- Initial LSP client (diagnostics, hover, completion, goto-definition)
- TextMate grammar for `.kry` files
- Snippet pack: `fn`, `let`, `if`, `for`, `while`, `match`, `actor`, `async`
- `kryos.serverPath` / `kryos.serverArgs` configuration

## 0.1.0 — 2026-02

- Syntax highlighting only (no LSP)
