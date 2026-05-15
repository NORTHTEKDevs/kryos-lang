# markdown

CommonMark-subset Markdown to HTML renderer for Kryos.

## Installation

```
kryos pkg add markdown
```

## Usage

```kryos
import "markdown"

fn main() {
    let html = markdown_to_html("# Hello\n\nThis is **bold** and *italic*.\n")
    println(html)
}
```

## API

- `fn markdown_to_html(md: str) -> str` — converts a Markdown string to HTML

## Supported syntax

- Headers (`#` through `######`)
- Paragraphs and blank-line separation
- Fenced code blocks (` ``` `)
- Inline code (`` `x` ``)
- **Bold** (`**x**`) and *italic* (`*x*`)
- [Links](url) (`[text](url)`)
- Unordered lists (`- item`)
- Ordered lists (`1. item`)
- Horizontal rules (`---`)
- HTML-safe escaping for `<`, `>`, `&`

## License

MIT — see https://github.com/NORTHTEKDevs/kryos-registry
