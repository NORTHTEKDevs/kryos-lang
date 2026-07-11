# kryos-markdown-pkg

Markdown to HTML in pure Kryos. No capabilities required — it is a pure
string transform, so it runs under deny-by-default with an unannotated `main`.

```kryos
use lib::{md_to_html}

fn main() {
    println(md_to_html("# Hello\n\nSome **bold** text and a [link](https://kryos.dev)."))
}
```

## Supported

- ATX headings `#` through `######`
- Fenced code blocks (``` ``` ```), contents HTML-escaped
- Unordered lists (`- item`)
- Paragraphs
- Inline: `**bold**`, `*italic*`, `` `code` ``, `[text](url)` (nested inline
  inside bold/italic/link text works; unterminated markers render literally)
- All text is HTML-escaped (`&`, `<`, `>`, `"`)

## Not supported (by design, v0.1)

Ordered lists, nested lists, blockquotes, tables, images, reference-style
links, setext headings, and raw HTML passthrough. The renderer never emits
un-escaped input text.

## API

| Function | Purpose |
| --- | --- |
| `md_to_html(md: str) -> str` | Render a document to an HTML fragment |
| `md_inline(s: str) -> str` | Render inline spans only |
| `md_escape(s: str) -> str` | HTML-escape a string |

## Test

```bash
kryos run packages/kryos-markdown-pkg/src/selftest.kry
# PASS kryos-markdown-pkg selftest (14 checks)
```
