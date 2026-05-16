# Changelog

All notable changes to Kryos will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.9.0] - 2026-05-16 — "LLVM backend production-ready: full benchmark suite"

The `kryos-codegen-llvm` crate has always existed but couldn't be exercised in
build environments without `clang`. With `clang 19` and `llvm 19` confirmed on
PATH, `kryos build --release` now produces native binaries that match Rust
`--release` and are within 1.0–1.6× of `gcc -O3` on standard numeric benchmarks
(see [BENCHMARKS.md](BENCHMARKS.md)).

### Added

- **`benchmarks/go/`** — Go equivalents of all 6 benchmark programs (`fib`,
  `mandelbrot`, `nbody`, `binary_trees`, `fannkuch`, `matmul`). All produce
  byte-identical output to the C reference implementations.
- **`benchmarks/python/`** — CPython equivalents of all 6 benchmark programs.
  `binary_trees` uses a 60 s timeout in the runner since CPython depth-18 recursion
  finishes in ~64 ms (no issue), but fib and python in other environments may time out.
- **`benchmarks/run.sh`** — Expanded from 4 columns (Kryos/Rust/C/ratio) to 8
  columns: Kryos LLVM, Kryos Cranelift, Rust --release, gcc -O3, clang -O3,
  Go, Python, and Kryos/gcc ratio. Uses `time.perf_counter()` with best-of-10
  for compiled languages and best-of-3 for Python.
- **`BENCHMARKS.md`** — New top-level benchmarking document with full methodology,
  per-benchmark analysis, honest assessment of wins and losses, and a roadmap for
  closing the remaining gap to gcc/Rust.

### Fixed

- **`kryos-codegen-llvm`: float array reads** — `kryos_array_get` returns raw
  `i64` bits; when the destination type is `f64` the codegen now emits
  `bitcast i64 → double` instead of the illegal `fadd double {i64}, 0`.
- **`kryos-codegen-llvm`: float array writes** — `kryos_array_set` expects its
  value argument as `i64`. Added `kryos_array_set` to the `runtime_param_types`
  table so `coerce_value` applies `bitcast double → i64` automatically.
- **`kryos-codegen-llvm`: undeclared math functions** — `sqrt`, `floor`, `ceil`,
  `round`, `sin`, `cos`, `tan`, `log`, `log2`, `log10`, `fabs` are standard C
  names called by Kryos builtins but were missing from the LLVM IR `declare`
  block, causing `undefined value '@sqrt'` link errors. All are now declared.

### Changed

- `benchmarks/RESULTS.md` — Regenerated with honest 7-language numbers
  including the two new LLVM codegen bug fixes (previously nbody could not
  compile under LLVM at all).
- README.md — Added link to BENCHMARKS.md and updated the speed claim to
  reflect LLVM backend parity with Rust on most numeric workloads.

### No language changes

This release is purely performance documentation and benchmark harness. No
syntax, type-system, or standard-library changes.

## [1.8.0] - 2026-05-15 — "Package registry: five starter packages"

This release closes Gap E (seed registry) by populating the empty
[kryos-registry](https://github.com/NORTHTEKDevs/kryos-registry) with five
starter packages. `kryos pkg add <name>` now resolves real metadata.

### Added
- `examples/extracted_packages/markdown/` — CommonMark-subset Markdown to HTML
  renderer extracted from `examples/showcase/markdown.kry`; public API:
  `markdown_to_html(md: str) -> str`
- `examples/extracted_packages/http-router/` — HTTP request/response structs
  (`Request`, `Response`) and routing helpers (`path_matches`, `parse_request_line`,
  `format_http_response`, etc.) extracted from `examples/http_server.kry`
- `examples/extracted_packages/json/` — friendly wrappers around Kryos built-in
  JSON builtins plus string-builder helpers (`json_object_literal`, `json_array_literal`,
  `json_escape`, etc.)
- `examples/extracted_packages/sqlite/` — FFI wrapper for SQLite via
  `libsqlite3.so.0`; public API: `sqlite_open`, `sqlite_exec`, `sqlite_close`
- `examples/extracted_packages/regex/` — regex matching via POSIX
  `regcomp`/`regexec` (libc) with pure-Kryos wildcard fallback; public API:
  `regex_match(pattern, text) -> bool`, `regex_find(pattern, text) -> i64`
- Registry index entries in `NORTHTEKDevs/kryos-registry` under NDJSON format
  (one JSON line per version) compatible with `kryos-package/src/registry.rs`
- GitHub Releases created for all five packages (`markdown-v0.1.0`,
  `http-router-v0.1.0`, `json-v0.1.0`, `sqlite-v0.1.0`, `regex-v0.1.0`)
  on `NORTHTEKDevs/kryos-registry`

### Verified
- `kryos pkg search markdown` → `markdown v0.1.0`
- `kryos pkg search regex` → `regex v0.1.0` (and all other packages)
- `kryos pkg add markdown` → `added dependency \`markdown\`` and writes entry
  to project `kryos.toml`

### Known limitations
- Tarball asset upload to GitHub Releases is blocked by the `uploads.github.com`
  domain not being proxied. Release tags exist and index URLs are correct;
  tarballs must be attached manually. `kryos pkg add` records the dependency
  and resolves metadata; tarball download will work once assets are attached.

---

## [1.7.0] - 2026-05-15 — "OpenGL 3.3 — 3D graphics"

This release closes Gap D (OpenGL 3.3) by adding full OpenGL 3.3 core-profile
bindings via SDL2's `SDL_GL_GetProcAddress`. Kryos can now build 2D/3D games
and visualizations entirely through the existing FFI subsystem.

### Added
- `examples/gl_cube.kry` — spinning cube demo using OpenGL 3.3 core profile:
  vertex + fragment shaders, VBO/VAO, indexed drawing, MVP matrix, and
  offscreen rendering verified with `glReadPixels` → PPM pixel dump
- `kryos_ffi_write_f32_bits(p, bits)` / `kryos_ffi_write_f64_bits(p, bits)` —
  write IEEE-754 floats to heap memory from integer bit-patterns (enables
  building float vertex/uniform buffers from Kryos source)
- `kryos_ffi_dlcallv_4f32(fp, b1, b2, b3, b4)` — call a `void(f32,f32,f32,f32)`
  function with per-bit-pattern arguments (used for `glClearColor`)
- `kryos_ffi_dlcall7` / `kryos_ffi_dlcallv5..7` — higher-arity FFI call
  helpers for 7-argument functions like `glReadPixels`
- `kryos_ffi_read_f32_bits` / `kryos_ffi_read_f64_bits` — read float memory
  as integer bit-patterns

### Verified
- Mesa software renderer (`LIBGL_ALWAYS_SOFTWARE=1`) renders orange cube;
  `glReadPixels` confirms 26 759 non-background pixels (vs 0 for clear)
- GL version 4.5 core profile obtained via `SDL_GL_CreateContext` offscreen

## [1.6.0] - 2026-05-15 — "HTTP/2, PostgreSQL, TLS server"

This release closes Gap C (HTTP/2 client) to complete the networking trifecta
alongside Gap A (TLS server) and Gap B (PostgreSQL driver) from v1.5.

### Added — HTTP/2 client (Gap C)

- `http2_get(url: str) -> str` — GET request with ALPN-negotiated HTTP/2,
  automatic HTTP/1.1 fallback, and shared connection pool. Returns body.
- `http2_post(url: str, body: str) -> str` — POST with body, returns response body.
- `http2_request(method: str, url: str, headers: str, body: str) -> str` — full
  request with method/headers/body control. headers is `"Name1: val1\nName2: val2"`
  newline-separated. Returns `"<status>\n<headers>\n\n<body>"` for complete
  response inspection.

All three builtins share a single `reqwest` blocking client instance (via
`OnceLock`) so the connection pool is reused across calls. `User-Agent: kryos/1.6`
is set by default. The h2 feature flag is enabled by default.

`examples/http2_demo.kry` demonstrates all three builtins against Cloudflare
(h2 trace) and httpbin.org (POST echo + custom headers).

---

## [1.5.0] - 2026-05-15 — "close the last five gaps"

This release closes every remaining gap toward true universality.
Kryos can now do TLS/HTTPS, build DOM/canvas/fetch WASM modules,
run cooperative async event loops, manage packages via a GitHub-backed
registry, drive a real Chromium browser over CDP+WebSocket, and paint
immediate-mode GUIs via SDL2 — all from pure Kryos.

### Added — Async I/O primitives

- `tcp_set_nonblocking(fd, bool) -> i64` — flip a socket between blocking
  and non-blocking modes.
- `tcp_try_accept(listener) -> i64` — returns 0 if no client is waiting
  instead of blocking. Pairs with `sleep_ms` for cooperative event loops.
- `tcp_try_recv(fd, max) -> str` — returns an empty string on WouldBlock.
- `poll_readable(fds, n, timeout_ms) -> i64` — bitmask of fds that
  became readable within the timeout (up to 63 fds).
- `sleep_ms(ms)` — sub-second cooperative pacing.

`examples/async_echo.kry` is a single-threaded non-blocking echo server
that accepts real TCP clients without spawning threads.

### Added — Crypto / binary primitives

- `sha1_hex(s) -> str` and `sha1_base64(s) -> str` — legacy SHA-1
  (RFC 6455 WebSocket handshakes, etc.) verified against the FIPS-180
  test vector for "abc".
- `base64_encode(s) -> str` / `base64_decode(s) -> str` — round-trip
  via latin-1 codepoints so binary data survives Kryos strings.
- `chr(n) -> str` / `byte_at(s, i) -> i64` — single-byte read/write
  primitives for hand-rolled binary protocols.

### Added — Browser bots (CDP)

- `examples/websocket_client.kry` — full RFC 6455 client framing:
  handshake (verified against the RFC test vector), masked text/ping/
  close frames, frame decoder for masked + unmasked frames.
- `examples/cdp_bot.kry` — Chrome DevTools Protocol driver that probes
  `http://localhost:9222/json` for an attached browser, opens a per-tab
  WebSocket, and dispatches `Page.navigate`, `Runtime.evaluate`, and
  `Page.captureScreenshot` over JSON-RPC.

### Added — Immediate-mode GUI

- `examples/sdl_imgui.kry` — pure-Kryos immediate-mode GUI on top of
  the existing SDL2 FFI bindings. Includes title bar, hoverable buttons,
  checkboxes, and a value slider. Runs headless under
  `SDL_VIDEODRIVER=dummy` for CI.

### Added — Package manager + registry

- Default registry rewritten to
  `https://github.com/NORTHTEKDevs/kryos-registry` (created this release,
  seeded with example entries).
- `kryos pkg add <name>` accepts bare names (resolves to `<name>@*`).
- `Op::Wildcard` added to the semver type so `*` matches any version.

### Notes

- TLS / HTTPS were already complete in 1.2.0 via rustls; verified
  end-to-end against httpbin.org and api.github.com in `examples/https_demo.kry`.
- WASM v0.3 (linear-memory arrays) and v0.4 (DOM/canvas/fetch host
  imports) shipped in this cycle on top of WASM v0.2 strings.

## [1.2.0] - 2026-05-15 — "truly universal: C FFI + graphics + WASM v0.2"

Kryos can now call any C library at runtime via dlopen/LoadLibrary,
including driving the full SDL2 window + renderer pipeline from pure
Kryos. No compiler changes were needed — the existing `extern "C"`
declaration syntax plus a small runtime in `kryos-stdlib-native::ffi`
is enough.

The WASM backend gained first-class strings: a `str` value now survives
in locals, parameters, and returns as a packed (offset, length) i64, and
string concatenation with `+` and `len(s)` both work in WASM modules.

This is the milestone where Kryos stops being a closed language and
becomes a real systems-level tool: anything libc, SDL2, libcurl,
libsqlite3, libssl, or any other shared library can do, Kryos can
now do. And anything text-shaped that runs in a browser can now be
written in Kryos.

### Added

- **Dynamic library FFI runtime** (`kryos-stdlib-native::ffi`).
  New runtime symbols:
  - `kryos_ffi_dlopen(name) -> handle` — wraps `dlopen` on Unix and
    `LoadLibraryA` on Windows.
  - `kryos_ffi_dlsym(handle, name) -> fnptr` — wraps `dlsym` /
    `GetProcAddress`.
  - `kryos_ffi_dlclose(handle)` — `dlclose` / `FreeLibrary`.
  - `kryos_ffi_dlcall0..6(fp, args...)` — call a resolved function
    pointer with 0-6 i64 args, returns i64.
  - `kryos_ffi_dlcallv0..4(fp, args...)` — void-return variants.
  - `kryos_ffi_dlcallv_4f(fp, f64, f64, f64, f64)` — four-f64-args
    helper for graphics APIs.
  - `kryos_ffi_cstr(s) -> *const char` — zero-copy convert a Kryos
    string to a NUL-terminated C string (KryosString is already
    NUL-terminated by design).
  - `kryos_ffi_string_from_ptr(ptr, len)` — read a C string back
    into a Kryos string (len = -1 uses strlen).
  - `kryos_ffi_malloc(n)` / `kryos_ffi_free(p, n)` — allocate raw
    memory blocks for C interop.
  - `kryos_ffi_read_i8/16/32/64`, `kryos_ffi_read_f32/64`, plus
    write variants — pointer-typed memory I/O.
- **C-compatible static-link FFI.** `extern "C" { fn foo(...); }`
  declarations are auto-resolved as `Linkage::Import` by Cranelift
  and linked against libc / system libs at build time. Works for
  any symbol the system linker can find.
- **SDL2 graphics demo, in pure Kryos.**
  - `examples/sdl_info.kry` — initializes SDL, queries version,
    platform, CPU count, RAM, performance counter.
  - `examples/sdl_window.kry` — opens a 320×240 window, creates a
    renderer, draws three colored rectangles (red, green, blue),
    presents the frame.
  - `examples/sdl_savepng.kry` — same scene but renders offscreen
    and dumps the framebuffer to disk via libc `fwrite`. Used to
    generate `docs/screenshots/sdl_kryos_demo.png` — the first
    rendered graphical output produced by a Kryos program.
- **libc FFI examples.**
  - `examples/ffi_libc.kry` — static-link smoke test
    (`getpid`/`getuid`/`time`).
  - `examples/ffi_module.kry` — dlopen libc and exercise
    `malloc`/`free`/`strlen`/`getpid` end-to-end.
  - `examples/ffi_test.kry`, `examples/ffi_dlopen.kry` — minimal
    starter snippets.
- **stdlib `ffi.kry` module.** Idiomatic wrappers (`dlopen`,
  `dlsym`, `call0..6`, `cstr`, `malloc`, etc.) for users who prefer
  named imports over raw `extern` blocks.

- **WASM v0.2: first-class strings.**
  - Strings now lower to a packed i64 `(offset | length << 32)` instead
    of just an i32 offset, so a `str` is a real value: it survives in
    locals, parameters, and returns without a side table.
  - String concatenation with `+` works end-to-end: the WASM backend
    detects `BinOp::Add` between two `Str` operands and emits a call to
    a new host import `kryos_string_concat(off1,len1,off2,len2) -> i64`.
  - `len(s)` is now a single `i64.shr_u` — the length is already there.
  - `println(s)` works on any string operand, not just literals.
  - New host imports plumbed (Node + browser): `kryos_string_concat`,
    `kryos_array_new`, `kryos_array_get`, `kryos_array_set`. Array
    builtins are wired in the runtime; Kryos-source-level array use in
    WASM lands in v0.3.
  - New example: `examples/wasm_strings.kry` — a Kryos program that
    builds greetings with `+`, prints them, and prints their lengths,
    running in Node and in any browser.
  - Updated `examples/wasm_runner.js` and `examples/wasm_browser_demo.html`
    with the new imports and a bump allocator above the static-data
    region (heap starts at 32 KB, memory grows on demand).

### Verified working

- `extern "C" { fn getpid() -> i64 }` returns the real PID.
- dlopen("libc.so.6") + dlsym("malloc") + write_i64 + read_i64 + free
  roundtrip.
- dlopen("libSDL2-2.0.so.0") + `SDL_Init` + `SDL_CreateWindow` +
  `SDL_CreateRenderer` + `SDL_SetRenderDrawColor` + `SDL_RenderClear`
  + `SDL_RenderFillRect` (3 colored rects) + `SDL_RenderPresent` +
  `SDL_RenderReadPixels` + clean shutdown — all from Kryos, **with
  no compiler changes**.
- `let s = "hello, " + name + "!"` compiles to WASM, runs in Node, and
  prints the concatenated string.
- All six v0.1 WASM examples (`wasm_hello`, `wasm_math`, `wasm_loop`,
  `wasm_fizz`, `wasm_control`, `wasm_browser_demo`) still pass.

## [1.1.0] - 2026-05-15 — "universal target"

Kryos now compiles to WebAssembly in addition to native code. The same
`.kry` source can be built to a native binary (Cranelift or LLVM) or to
a `.wasm` module that runs in any browser or WASI host.

Also fixes a long-standing TCP concurrency bug: spawned worker threads
no longer serialize through a global socket mutex during blocking
send/recv. Multi-client servers actually scale now.

### Added

- **WebAssembly backend (`--backend wasm`).** New crate
  `kryos-codegen-wasm` emits standalone `.wasm` modules. Supported in
  v0.1: i64/f64 arithmetic, comparisons, booleans, if/else/elif chains,
  while loops, function definitions, direct calls, recursion,
  `println(i64)`, `println(f64)`, `println(str-literal)` via host imports.
- **Browser demo.** `examples/wasm_browser_demo.{kry,wasm,html}` — a
  Kryos program (fib + factorial + sum) running in a real browser via
  `fetch` + `WebAssembly.instantiate`.
- **WASM host runner.** `examples/wasm_runner.js` — a 60-line Node.js
  host that provides the three imports the WASM backend expects.
- **New example programs.** `wasm_hello.kry`, `wasm_math.kry`,
  `wasm_fizz.kry`, `wasm_loop.kry`, `wasm_control.kry` — covers each
  category of WASM-supported control flow.

### Fixed

- **TCP send/recv no longer block other socket operations.** The
  global socket-table mutex is now released before `TcpStream::read`
  and `TcpStream::write` are called (via `try_clone`), matching the
  pattern `tcp_accept` already used. Verified by serving three
  concurrent curl requests against `examples/showcase/web_server.kry`.

### Backend coverage matrix

| Feature              | Cranelift | LLVM | WASM v0.1 |
|----------------------|:---------:|:----:|:---------:|
| Integer/float arith  | ✅ | ✅ | ✅ |
| Booleans, comparisons| ✅ | ✅ | ✅ |
| if/else/elif         | ✅ | ✅ | ✅ |
| while loops          | ✅ | ✅ | ✅ |
| Functions, recursion | ✅ | ✅ | ✅ |
| println              | ✅ | ✅ | ✅ |
| Heap strings         | ✅ | ✅ | ❌ |
| Arrays, maps         | ✅ | ✅ | ❌ |
| Structs, enums       | ✅ | ✅ | ❌ |
| Channels, spawn      | ✅ | ✅ | ❌ |
| HTTP, regex, JSON    | ✅ | ✅ | ❌ |

The `❌` rows in the WASM column track to v1.2 — they need ARC + a
linear-memory string runtime + WASI imports. v0.1 is intentionally
scoped to "the parts that need no heap".

## [1.0.1] - 2026-05-15 — "universal-language stress test"

Wrote 8 different classes of program in pure Kryos to validate the
universal-language claim. Found and fixed one real bug along the way.

### Added (showcases)
- `examples/showcase/extra/calc.kry` — arithmetic parser with recursive
  descent + precedence + mutual function recursion.
- `examples/showcase/extra/csv.kry` — CSV reader with group-by salary
  aggregation.
- `examples/showcase/extra/brainfuck.kry` — full Brainfuck interpreter
  (prints "Hello World!").
- `examples/showcase/extra/life.kry` — Conway's Game of Life on a 20×20
  grid (glider, blinker).
- `examples/showcase/extra/api_client.kry` — outbound HTTPS plus JSON
  tree walk against httpbin.org.
- `examples/showcase/extra/regression.kry` — linear regression by
  gradient descent. Learns y = 3x + 7 from noisy samples.
- `examples/showcase/extra/template.kry` — Mustache-style `{{var}}`
  templating engine.
- `examples/showcase/extra/regex.kry` — tiny regex engine: literals,
  `.`, `*`, `^`, `$`.

### Fixed
- **`sleep_ms` no longer fails to link.** The builtin was registered in
  the MIR builtins table but had no runtime symbol or codegen wiring.
  Now properly implemented as `kryos_rt::spawn::kryos_sleep_ms`, with
  Cranelift codegen dispatch and JIT symbol registration. Verified end
  to end: `sleep_ms(500)` waits exactly 500 ms.

## [1.0.0] - 2026-05-14 — "production"

First stable release. Same code as 0.5.0 with a 1.0 version stamp,
committing Kryos to the stability guarantees in `docs/STABILITY.md`.

From this release forward:

* The lexical grammar, the `pub` standard library, the documented
  builtins, the `kryos.toml` schema, and the `kryos` CLI subcommand
  set are stable. Breaking changes require a 2.0.0 bump.
* The `2026` language edition is the default for projects that omit
  `edition` from their manifest.
* Patch releases (`1.0.z`) fix bugs without changing behaviour. Minor
  releases (`1.y.0`) may add features and APIs but never change the
  meaning of existing code.
* Deprecations carry a warning for at least one minor cycle before
  removal in a future major.

No functional changes from 0.5.0 — see the entry below for the full
list of what shipped in this push.

## [0.5.0] - 2026-05-14 — "universal language"

The production-ready push. Kryos can now write the things it was designed
to write: HTTP servers, MCP servers, LLM agents, static site generators,
persistent databases, parallel job pools, and small compiler tools — all
in pure Kryos. The plumbing required to ship and run those programs is
also in place: a package manager with local path dependencies, prebuilt
binary distribution, a stable VS Code LSP client, and a written stability
policy.

### Added

#### Showcase apps (all runnable end-to-end)
- `examples/showcase/rest_api.kry` — full CRUD HTTP server using real
  mutable module-level globals; verified against curl.
- `examples/showcase/markdown.kry` — pure-Kryos markdown→HTML converter.
- `examples/showcase/kvdb.kry` — append-only persistent key/value store
  with tab/newline-safe percent encoding, in-memory replay, and compaction.
- `examples/showcase/mcp_server.kry` — real Model Context Protocol
  server speaking JSON-RPC 2.0 over stdio. Implements `initialize`,
  `tools/list`, `tools/call`, `shutdown`. Built-in tools: `echo`, `now`,
  `add`, `read_file`, `write_file`, `http_get`.
- `examples/showcase/agent.kry` — OpenAI-compatible Chat Completions
  agent with tool-use loop. Drives multi-turn conversations through
  function calling; falls back to an offline demo that prints the
  exact OpenAI wire-format request.
- `examples/showcase/ssg.kry` — static site generator: inlined
  markdown→HTML, layout template, manifest-driven build. Emits a real
  multi-page HTML site plus a shared `style.css`.
- `examples/showcase/worker_pool.kry` — fan-out/fan-in concurrency
  showcase using `spawn` plus channels and sentinel-based shutdown.
- `examples/showcase/kdoc.kry` — a small documentation extractor
  written in Kryos itself. Scans `.kry` files for `pub` declarations
  and emits a Markdown API reference. Satisfies the self-host milestone.

#### Language and compiler
- **Real mutable module-level globals.** `let mut <name>: <type> = <expr>`
  at file scope, no workarounds, with proper MIR type inference.
- **String comparison codegen.** `<`, `>`, `<=`, `>=` on strings now
  lower through `kryos_string_compare(a, b) -> i64` and `icmp`.
  Available in both the AOT and JIT backends.
- **f64↔i64 round-trips in codegen** for `json_number` and friends.
- **Mutable globals participate in type inference** for indexing and
  assignment.

#### Package manager
- `parse_dep_string` accepts bare relative/absolute paths (`./foo`,
  `../foo`, `/abs`) and an explicit `path:<dir>` form in addition to
  the existing `<source>@<version>` form.
- Driver import resolver: walks up from each source file looking for
  `.kryos/deps/<pkg>.redirect` written by `kryos pkg install`, parses
  the `path = "..."` entry, and resolves `use pkg` to `<dep>/src/lib.kry`
  (or `<dep>/src/<pkg>.kry`) and `use pkg::a::b` to `<dep>/src/a/b.kry`.
  Verified end-to-end with two side-by-side projects.

#### Distribution
- `install.sh` / `install.ps1` already shipped — now coupled with the
  release workflow that builds prebuilt binaries for
  `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`,
  `x86_64-apple-darwin`, and `aarch64-apple-darwin` when a `v*` tag is
  pushed.
- New `.github/workflows/cross.yml`: cheap cross-build matrix
  (`linux-gnu`, `linux-musl`, `windows-gnu`, `aarch64-linux-gnu`) on
  every push.

#### Editor support
- VS Code extension v0.3.0 wires up the LSP client. Launches
  `kryos lsp` over stdio with `vscode-languageclient`. Configurable
  via `kryos.serverPath`, `kryos.serverArgs`, and `kryos.trace.server`.

#### Documentation
- `docs/STABILITY.md` — written stability policy: SemVer, what's stable
  vs. internal, deprecation lifecycle, and the language-edition
  mechanism (`edition = "2026"` is the current default).
- `docs/12-modules-and-packages.md` — appended a verified local-path-dep
  walkthrough.

### Fixed
- `infer_expr_type` now consults `ctx.mutable_globals` so indexing a
  global `[str]` array returns a `str`, not a pointer-sized `i64`. This
  unblocked the kvdb showcase and similar code that holds collections
  in a global.

## [0.4.0] - 2026-05-11 — "credible beta"

This is the release that takes Kryos from a hand-rolled toy compiler to a
language that can credibly be tried by someone other than its author.
Every item below ships with documentation, tests, or a runnable demo;
nothing in this release is marked experimental.

### Added

#### Reliability
- **Runtime panics carry source spans.** Every runtime panic (overflow,
  division by zero, array-OOB, stack overflow, etc.) now points at the
  `file:line:col` where it originated rather than at the runtime crate
  internals.
- **Stack-overflow detection** via a `SIGSEGV` alt-stack handler that
  distinguishes recursion blow-outs from generic segfaults and reports
  them with a friendlier message + the offending span.
- **Integer-overflow policy** is now defined and documented in
  `docs/16-integer-overflow.md`: `wrapping_*` / `checked_*` /
  `saturating_*` builtins are available, signed overflow with the plain
  `+ - *` operators is well-defined as wrap-on-release, panic-on-debug.
- **Unsafe-block audit** in `docs/17-unsafe-audit.md`: every `unsafe`
  region in the runtime and native stdlib (8 patterns across 8 files)
  has a documented invariant.

#### Tooling
- **`kryos explain ERRXXXX`** with 20 long-form error articles (modelled
  on `rustc --explain`). Each includes a broken example, a fixed example,
  and the rationale behind the diagnostic. Run `kryos explain --list` for
  the catalog.
- **`kryos test` cargo-parity**: positional `FILTER` argument,
  `--exact`, `--nocapture`, `--list`, and `--format=json` for
  newline-delimited JSON output that mirrors
  `cargo test --format=json` events.
- **`kryos build --target=<triple>`** is now wired through to LLVM
  rather than silently using the host triple. Eleven known-good targets
  ship with descriptions; `--target=help` prints the table. See
  `docs/18-cross-compilation.md` for required toolchains and known
  failure modes.
- **Benchmark suite** under `benchmarks/` covering mandelbrot, n-body,
  binary-trees, fannkuch, matmul, and fib against Rust and C baselines.
  `benchmarks/run.sh` produces a reproducible `RESULTS.md`; on the
  reference hardware Kryos hits parity with C on mandelbrot (1.03×) and
  stays within 3.5×4.5× on the numeric benchmarks.

#### Documentation
- **`docs/19-language-reference.md`** — the authoritative v0.4 language
  spec: lexical structure, type system, expression grammar (with the
  full precedence table), control flow, declarations, pattern matching,
  ownership / drop order, integer overflow, concurrency, unsafe code,
  modules, panics, and a conformance checklist.
- **`docs/BUGS.md`** records the one known-leaky pattern in the v0.4
  ownership checker (string-field move across struct-returning function
  boundaries) along with its workaround.

#### Showcase suite
Five end-to-end programs under `examples/showcase/` proving the
language can be used to build the kinds of things it claims to support:

- `cli_tool.kry`       — grep-style CLI with POSIX exit codes.
- `parser.kry`         — recursive-descent calculator with error
  reporting (source columns, three failure modes).
- `bytecode_vm.kry`    — stack VM with a 13-opcode ISA, disassembler,
  and three demo programs (sum 1..10, factorial(7), fib(10)).
- `agent_runtime.kry`  — LLM-style tool-use loop: history, planner,
  tool registry, bounded step budget.
- `web_server.kry`     — minimal HTTP/1.0 server using `tcp_listen` /
  `tcp_accept` / `tcp_send`, serving HTML / JSON / 404 routes.

See `examples/showcase/README.md` for run instructions.

### Changed
- Workspace version bumped to `0.4.0` across all crates.
- The test runner library now exposes `RunOptions`, `run_test_with`,
  `run_all_with`, `run_annotated_tests_with`, and `format_report_json`
  in addition to the existing entry points. Existing callers keep
  working unchanged.
- 843 workspace tests now pass (up from 831 at the start of the v0.4
  cycle); +12 from new unit tests across the `kryos test`, `explain`,
  and `build --target` work.

### Status

Kryos v0.4.0 is the **credible-beta** release: the toolchain is
complete enough that someone other than the author can clone it, build
it, follow the docs, and write real programs. Real users and a stable
1.0 API still ahead.

---

## [0.3.6] - 2026-05-11

### Fixed
- CI green again: resolved clippy errors introduced in the LLVM aggregate-ABI and Cranelift drop-path commits (`collapsible_match`, `too_many_arguments`, `if_same_then_else`) via targeted `#[allow]` attributes; no behavior change.
- `rustfmt` drift across `kryos-codegen-cranelift`, `kryos-codegen-llvm`, `kryos-mir`, `kryos-stdlib-native`, `kryos-types` -- all formatted with `rustfmt 1.95`.

### Changed
- Repository home: all `FrostbyteDevTeam/kryos-lang` URLs in README, docs, install scripts, `Cargo.toml`, VS Code extension, and contributing guide updated to `NORTHTEKDevs/kryos-lang`.
- `README.md`: replaced the misleading "~48 GB RAM" debug-build warning with a calibrated build-footprint note (~6 GB disk, ~3 GB peak RAM with `-j 2`, ~2 min cold). Documented that LLVM is **not** a build dependency -- the LLVM backend emits IR as text.
- `README.md` quick-start example path now points at `../examples/hello.kry` (the previously referenced `examples/proof.kry` did not exist).
- Bumped to `v0.3.6` across `Cargo.toml`, `install.ps1`, `docs/01-getting-started.md`, `docs/WHY_KRYOS.md`.

## [0.3.5] - 2026-04-16

### Fixed
- `MirType::Map` sentinel migration: replaced `Ptr(Str)` map-handle hack with typed `Map { key, value }` variant throughout MIR, lowering, and both backends
- REPL `:type` map inference: map literals now report the actual key/value element types instead of always `Map<i64, i64>`
- REPL `:type` index inference: indexing into a `Map<K, V>` now returns `V` instead of `i64`
- Package registry: `parse_index_entry` now parses the `deps` JSON object into the dependency map; transitive dependency resolution from registry responses now works
- `NodeTable::get_mut` and `::remove` dead_code warnings suppressed in `kryos-stdlib-native/src/json.rs` -- intentional forward-facing API surface

### Changed
- `README.md`: corrected version badge from v0.3.4 to v0.3.5
- `CONTINUE.md` (internal dev artifact) replaced with `ARCHITECTURE.md` for public distribution
- `examples/README.md`: documented all 20 examples (was 12); added blocking notes for `http_api.kry` and `mcp_server.kry`

---

## [0.3.4] - 2026-04-14

### Added
- `float(str)` builtin -- parse a string as f64 via `kryos_builtin_parse_float` (Cranelift backend)
- Example: `ai_agent.kry` -- research agent using the Kryos agent framework with Anthropic API integration
- Example: `http_api.kry` -- in-memory task-list REST API with routing and JSON responses
- Example: `mcp_server.kry` -- Model Context Protocol server over stdio (JSON-RPC 2.0)

### Fixed
- MIR match arm type inference: enum variant field types now correctly propagate to the result local (fixes f64 fields inferred as i64 in `JsonValue::Number(n) => n` patterns)
- JSON stdlib: `if/else if/else` chains in `_parse_string`, `_parse_number`, and `_escape_string` converted to sequential `if` + flag pattern (avoids compiler branch target bug in deep else-if chains)
- JSON parser: `@copy` on `Parser` struct prevents ownership errors across recursive descent calls

---

## [0.3.3] - 2026-04-14

### Added
- `Self` type in trait method signatures resolves to the implementing type at each call site
- `Type::method(args)` associated function syntax (`StaticMethodCall` AST node, parsed, type-checked, MIR-lowered, both backends)
- `install.ps1` Windows PowerShell installer
- `CONTRIBUTING.md` developer guide with compiler pipeline walkthrough

### Fixed
- Clippy: `&param_ty` double-reference in `kryos-types/src/check.rs:1421` (immediate deref lint)
- Version bump: `compiler/Cargo.toml` 0.2.1 -> 0.3.3

---

## [0.3.2] - 2026-04-13

### Added
- Developer adoption sprint: stdlib completions, string safety improvements, DX ergonomics
- Module system for stage-0 self-host build (`use` imports in bootstrap)
- Calling closures stored as struct fields
- Correct MirType for fn-typed captures in lambda thunks

---

## [0.3.1] - 2026-04-12

### Added
- `@pure` attribute optimization -- CSE (common subexpression elimination) and dead call elimination at MIR level
- `@test` annotation runner -- discover and JIT-execute `@test` functions via `kryos test`

### Fixed
- REPL state persistence -- `use`/`type`/`extern`/`actor`/`pub` classified as declarations, persist across lines
- Array element drop recursion -- named type drop helpers for struct/enum fields (prevents infinite recursion)
- Closure capture memory leak -- per-closure dropper thunks generated for ARC env cleanup

---

## [0.2.2] - 2026-04-09

### Fixed
- Deep memory safety pass: ownership cloning, Shared drop, @copy ARC retain
- String interpolation intermediate leak
- try/catch result enum leak
- LLVM backend drop parity (enum, struct, array, map, function)
- Const eval overflow: checked arithmetic, unfoldable at compile time
- Formatter: doc comments preserved on Actor, TypeAlias, Import, Extern declarations

---

## [0.2.1] - 2026-04-08

### Fixed
- Critical memory safety, control flow, and type system fixes
- Exception cleanup includes MirType::Enum in droppable filter
- CI/CD GitHub Actions matrix (Ubuntu + Windows + macOS)
- Clippy clean (0 warnings)
- @copy struct deep-copy: Function/Shared fields call kryos_arc_retain
- ActorSend: heap-typed args cloned before send

---

## [0.2.0] - 2026-04-08

### Self-Hosting Milestone
- 18,700-line self-hosted compiler written in Kryos (15 files)
- Full compilation pipeline: lexer, parser, type checker, MIR lowering, optimizer, register allocator, x86_64 codegen, ELF/COFF linker
- Zero-dependency runtime (runtime.kry): raw Linux x86_64 syscalls, bump allocator, byte buffers, string/array/map operations
- 3-stage bootstrap verification script (stage-0 Rust -> stage-1 -> stage-2 -> stage-3, SHA-256 identity proof)
- Stage-1 binary: 1MB PE32+ executable, compiles and runs Kryos programs
- Self-host type-checks cleanly (0 errors) through the Rust compiler via concatenation

### Module System
- File-based module resolution with `use` imports
- Stdlib resolution via `use std::math`, `use std::json`, etc.
- Selective imports: `use std::math::{abs, min, max}`
- Transitive imports with diamond deduplication and cycle detection
- Sibling file and directory module (`foo/mod.kry`) resolution
- Const declarations now importable by name

### Capability Enforcement
- 35 builtin functions mapped to 7 capability categories (io, net, process, term, crypto, time, ffi)
- Deny-by-default enforcement within `@capabilities`-annotated scopes
- Cross-function capability propagation (caller must have callee's required capabilities)
- Opt-in design: unannotated functions have ambient authority (backward compatible)

### LLVM Backend
- Fixed systematic ptr/i64 type mismatch in LLVM IR emitter
- Added `coerce_value` helper for type-safe conversions at 15+ boundary points
- Fixed identity copy pattern (`add ptr` -> `getelementptr i8`) for pointer types
- Fixed `to_string` return type coercion and float argument dispatch
- LLVM tools available on Windows (clang 21.1.8, lld-link)

### Compiler Fixes
- Generic functions now monomorphize per call site (fresh type variables)
- Multiple trait impls no longer clobber each other's `self` type
- `throw` propagates across function boundaries via thread-local exception state
- `to_string()` on strings returns the string (not the raw pointer address)
- `sqrt`, `floor`, `ceil`, `abs` use native Cranelift instructions (fixes ICE)
- MIR type inference for untyped constants (no longer defaults to I64)
- Cranelift float/int type coercion uses proper bitcast instructions
- `kryos pkg init` now creates files on disk (kryos.toml, src/main.kry, .gitignore, README.md)
- `kryos check` now supports `--skip-ownership` flag

### Added
- Array concatenation operator: `a + b` and `a += b` for arrays (type-checked, MIR-lowered, both backends)
- `kryos_array_concat` runtime function for array concatenation
- Closure environments heap-allocated via `malloc` (fixes segfault when closures escape their creating function)
- `push(arr, val)` and `pop(arr)` now borrow the array instead of moving it in ownership analysis
- Native test runner prefers release binary over debug (matches `--release` build workflow)
- `StoreField` MIR instruction for proper struct field mutation (replaces `__kryos_field_store` hack)
- Full `StoreField` implementation in both Cranelift and LLVM backends
- `--skip-ownership` CLI flag for self-host bootstrap (ownership checker fires on refcounted patterns)
- `kryos_string_char_at` runtime function for string indexing
- `no_struct_lit` parser flag to prevent struct literal ambiguity in if/while/for/match conditions
- `parse_expr_no_struct_lit()` parser function used in all conditional contexts
- Array/tuple codegen now uses runtime `kryos_array_new`/`push`/`get` for consistency
- Array size coercion: fixed-size arrays assignable to dynamic arrays (`[T; N]` -> `[T]`)
- Division-by-zero check widened to i64 for narrow integer types
- Float-to-int and int-to-float bitcasting in function call argument coercion
- IndexAccess type inference for arrays, tuples, and strings in MIR lowering
- MIR elif duplicate block fix (prevents self-loop when last elif has no else)
- New example: `word_count.kry`
- Package registry now computes deterministic content hash (replaces TODO placeholder)

### Fixed
- Demo example: removed unimplemented tensor extern calls that caused segfault
- Calculator example: added `**` (power) operator to string-matched calculator
- Clippy: removed dead code, unused imports, function-cast-as-integer warnings
- Clippy: fixed prefix-stripping pattern in semver parser

### Changed
- Self-host MIR: array concatenation (`arr + [elem]`) replaced with `push(arr, elem)` for efficiency
- Self-host main: `std.io.read_file` -> `file_read`, `std.process.args()` -> `args()` (runtime functions)
- Self-host codegen: `&&`/`||` -> `and`/`or` (correct Kryos syntax), `char_at` -> `char_code(substr(...))`
- Bootstrap script upgraded from 2-stage to proper 3-stage verification (stage-2 == stage-3)
- FFI crates (`kryos-rt`, `kryos-stdlib-native`) now properly document safety and suppress raw-pointer clippy lints

## [0.1.1] - 2026-04-07

### Fixed
- Parser struct-literal ambiguity: `match TK_EOF { ... }` no longer parsed as struct literal
- Struct field access segfaults: structs now heap-allocated (malloc) instead of stack slots
- String match patterns: `match s { "hello" => ... }` now emits equality-comparison chain instead of integer switch
- Tail expression return: functions ending with bare `match`/`if` now implicitly return the result
- String concatenation with non-string operands: automatic coercion via `coerce_to_string()` helper
- Double-free prevention: `dropped_locals` tracking prevents nested scope re-drops
- ComptimeBlock type inference: `comptime { expr }` now infers correct result type
- Copy semantics for computed expressions: BinaryOp, FnCall, MatchExpr, IfExpr, UnaryOp, MethodCall, Cast, IndexAccess, Block, PipeExpr, and Borrow/Deref now correctly report copy when result is a primitive type
- `type_of()` builtin: compile-time type dispatch for all MIR types (f64, bool, str, etc.) instead of always returning "i64"
- `assert()` builtin: accepts 1 or 2 args, bool conditions extended to i64, default "assertion failed" message

### Added
- 4 new example programs: calculator, word_count, json_counter, all_features showcase
- String pattern matching in match expressions (via BinOp::Eq chain with Branch terminators)
- Implicit return for tail expressions in non-void functions
- `fn main()` wrapper for kryos_bootstrap.kry self-hosting lexer example
- Criterion benchmark suite: 9 groups (lex, parse, typecheck, ownership, capabilities, MIR, codegen, pipeline, JIT fibonacci)
- 9 new ownership analysis tests for copy semantics validation
- `is_type_expr_copy()` helper for cast expression type analysis

### Changed
- Ownership analyzer `expr_is_copy()` now recursively handles 15+ expression types
- Type checker: `assert()` signature updated to accept `bool` condition, 1-arg special case
- Type checker: `type_of()` parameter type set to `Error` (accepts any type)
- Documentation: fixed incorrect builtin names (`int`/`float`/`str` → `parse_int`/`parse_float`/`to_string`)
- Documentation: updated tail expression return note in Functions chapter
- Documentation: added implementation status callouts for borrowing and self-healing runtime
- Documentation: fixed `dyn Trait` implementation status (vtable-based dispatch is implemented)
- Standard library stubs: fixed broken references in math, string, collections, crypto, fmt, http, json, net, and test modules
- README: updated version to v0.1.1, added all_features example

## [0.1.0] - 2026-04-07

### Added
- 21-crate Rust compiler (49,000+ lines)
- Dual backends: Cranelift (fast debug builds) and LLVM (optimized release builds)
- Ownership-based memory safety without lifetime annotations
- Compile-time capability enforcement (deny-by-default resource access)
- Compile-time evaluation with `comptime` blocks
- Type inference with explicit annotations where needed
- Pattern matching with integer, string, enum, and wildcard patterns
- Dynamic dispatch via `dyn Trait` (vtable-based)
- Generics with monomorphization
- Concurrency: `spawn`, typed channels, actors, `select`
- 5 MIR optimization passes: constant folding, dead code elimination, function inlining, tail-call optimization, strength reduction
- 28 standard library modules (strings, math, collections, I/O, networking, crypto, JSON, regex, datetime, tensors, agents, probability, reactive streams)
- Ergonomic builtins: `file_read`, `file_write`, `env_get`, `time_now`, `assert`, `parse_int`, `parse_float`, `type_of`
- Error handling with `try`/`catch`/`throw`
- VS Code extension with syntax highlighting, snippets, and language configuration
- Language Server Protocol (LSP) server
- Code formatter (`kryos fmt`)
- Documentation generator (`kryos doc`)
- Package manager (`kryos pkg`)
- Test runner (`kryos test`)
- Interactive REPL (`kryos repl`)
- C header binding generator (`kryos bindgen`)
- Native tensor runtime with 38 FFI operations
- GitHub Actions CI (build, test, clippy, fmt on Linux and Windows)
- 13 example programs
- 15-chapter language manual
- 680+ tests, all passing
