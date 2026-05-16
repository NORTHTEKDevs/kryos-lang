# Kryos for Visual Studio Code

Syntax highlighting, snippets, and Language Server integration for the [Kryos](https://github.com/NORTHTEKDevs/kryos-lang) programming language.

## Features

- Syntax highlighting for `.kry` files
- Snippets for common Kryos constructs
- Language Server support: diagnostics, hover, completion, goto-definition (via `kryos lsp`)

## Setup

1. Install the Kryos toolchain so that the `kryos` binary is on your `PATH`:

   ```bash
   curl -fsSL https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.sh | sh
   ```

2. Install this extension.

3. (Optional) point the extension at a specific binary via `kryos.serverPath` in your VS Code settings.

## Building from source

```bash
cd editors/vscode
npm install
npm run compile          # produces out/extension.js
# Press F5 in VS Code with this folder open to launch an Extension Development Host.
```

## Packaging for the Marketplace

```bash
cd editors/vscode
npm install
npm run compile
npm run package            # produces kryos-<version>.vsix
# or, with @vscode/vsce installed globally:
#   vsce package
```

The resulting `.vsix` can be installed in VS Code via:
Command Palette → "Extensions: Install from VSIX..."

Publishing requires a `vsce` publisher token for the `northtekdevs` publisher:

```bash
vsce login northtekdevs
npm run publish
```

## Configuration

| Setting             | Default   | Description                                                 |
|---------------------|-----------|-------------------------------------------------------------|
| `kryos.serverPath`  | `kryos`   | Path to the Kryos binary that hosts the language server.    |
| `kryos.serverArgs`  | `["lsp"]` | Args passed to the Kryos binary to start the LSP.           |
| `kryos.trace.server`| `off`     | LSP trace level (`off` / `messages` / `verbose`).           |
