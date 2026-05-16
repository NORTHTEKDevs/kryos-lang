# Kryos for Zed

Syntax highlighting and Language Server integration for the
[Kryos](https://github.com/NORTHTEKDevs/kryos-lang) programming language
inside the [Zed](https://zed.dev) editor.

## Features

- `.kry` file recognition
- Tree-sitter grammar reference (`editors/tree-sitter-kryos` in the
  Kryos repo)
- Language Server support via `kryos lsp` (diagnostics, hover,
  completion, goto-definition)

## Installation

### From source (dev extension)

1. Clone the Kryos repo and ensure `kryos` is on your `PATH`:

   ```bash
   curl -fsSL https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.sh | sh
   ```

2. In Zed, open the **Extensions** view (`cmd+shift+x` / `ctrl+shift+x`).

3. Click **Install Dev Extension** and pick the
   `editors/zed/` directory of this repository.

### From the Zed extension registry

Once published, the extension will be available under the name **Kryos**.

## Building

```bash
cd editors/zed
cargo build --release --target wasm32-wasi
```

This produces a WASM artifact suitable for the Zed extension runtime.

## Configuration

The extension automatically looks for the `kryos` binary on your
`PATH` first, then falls back to `./compiler/target/release/kryos` if
you are inside a Kryos checkout.
