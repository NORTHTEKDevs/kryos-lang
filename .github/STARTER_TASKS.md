# Starter tasks

This file is a curated list of small, well-scoped tasks for first-time
contributors. They are intentionally narrower than the open issue tracker
and are the right place to land your first PR.

Pick one, comment on the matching issue (or open one referencing this
file), and submit a PR. If no matching issue exists yet, open one with
the `good first issue` label and link the relevant task number below.

---

## Documentation

1. **Add a cookbook recipe.** `docs/learn/cookbook/` currently has six
   recipes. Good candidates for a seventh:
   - **CSV parser:** read a `.csv` file, sum a numeric column, print the
     total. Touches `file_read`, `str_split`, parsing primitives.
   - **TCP echo server:** accept connections in a loop, echo each line
     back. Touches `tcp_listen`, `spawn`, channels.
   - **WebSocket client:** connect to a public echo server, send a
     message, print the reply. Touches `ws_connect` from the stdlib.
   - **SQLite query tool:** open a DB file, run a query passed on the
     command line, print rows. Touches `sqlite_open`, `sqlite_query`.

   Recipe format: one `.md` file, ~80–150 lines, one runnable program,
   a short "what this teaches" intro, a "next steps" footer.

2. **Improve a chapter in the reference manual.** Chapters `01` through
   `19` in `docs/` are the formal reference. Pick one and:
   - Add a missing code example.
   - Fix a stale code sample that no longer compiles against `kryos
     2.3.0`.
   - Expand the "common mistakes" footer at the end of the chapter.

3. **Write a "porting from <X>" guide.** A short doc under `docs/learn/`
   that translates the ~20 most common idioms from another language to
   Kryos. Pick whichever language you know well: Rust, Go, Python,
   TypeScript, OCaml, etc.

## Standard library

4. **Add a missing string function.** Audit `compiler/stdlib/string.kry`
   for gaps. Common requests: `str_pad_left`, `str_pad_right`,
   `str_repeat`, `str_count`. Each new function needs:
   - The Kryos implementation in the stdlib module.
   - At least three `@test` cases.
   - A documentation comment matching the existing style.

5. **Add a missing array function.** Same shape as (4), against
   `compiler/stdlib/array.kry`. Candidates: `array_chunk`,
   `array_flatten`, `array_unique`, `array_partition`.

6. **Add a JSON helper.** `compiler/stdlib/json.kry` exposes parse and
   stringify; useful additions: `json_pretty(s)` that re-stringifies
   with indentation, `json_get(s, path)` that returns a nested value by
   `"a.b.c"`-style path.

## Examples

7. **Add an example program.** `examples/` showcases concrete
   capabilities. Good additions:
   - **Markdown to HTML converter.** ~150 lines. Show string handling.
   - **JSON-to-CSV converter.** ~100 lines. Show JSON + file I/O.
   - **Mini-templating engine.** Substitute `{{ name }}` style holes in
     a template string from a map.

   Examples must run on `kryos run examples/yourfile.kry` with no
   external setup beyond `examples/`-local files.

## Diagnostics

8. **Improve one error message.** Pick any `kryos-errors`, `kryos-parser`,
   or `kryos-types` diagnostic that you found confusing and:
   - Update its message to say what was expected and what was found.
   - Add a hint line suggesting a fix when there's a clear common cause.
   - Add a snapshot test in the relevant crate.

## Tooling

9. **Add a `kryos new --template <name>` flag.** Today `kryos new <name>`
   scaffolds a default project. Extend it with `--template lib`,
   `--template cli`, `--template http-server` that lay down a starter
   structure for that shape. Each template is a small directory bundled
   with the CLI crate.

10. **Add a `kryos fmt --check` flag.** Today `kryos fmt` rewrites files.
    `--check` should print the diff and exit non-zero without modifying
    anything. Mirrors `rustfmt --check`. Roughly 30–50 lines in
    `kryos-fmt`.

## Editor support

11. **Polish the VS Code extension.** `editors/vscode/`. Open candidates:
    - More snippet completions in `snippets/kryos.json`.
    - Better TextMate grammar handling for `@` decorators.
    - An icon set for the file explorer.

12. **Finish the Zed extension wiring.** `editors/zed/` is a scaffold
    that auto-discovers a `kryos` binary on `PATH`. It needs LSP
    completions to land cleanly; verify with a real Zed install and
    file follow-up issues for any rough edges.

---

If you want to tackle something not on this list, open a Discussion
first so we can scope it together.
