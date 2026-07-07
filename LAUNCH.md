# Kryos Launch Checklist

Status as of v1.0.0-rc.1 (2026-07-06). This tracks the remaining launch steps,
split by who does them.

## Done (automated + verified)

- [x] **v1.0.0-rc.1 cut** — tag pushed, CI + Release workflows green, binaries
      published for Linux / macOS (arm64 + x86_64) / Windows.
- [x] **Dogfood demo** — `examples/showcase/trust_agent.kry` (capability-audited
      AI agent, runs offline) + `trust_agent_overreach.kry` (deliberately
      rejected). The examples gate asserts the rejection so it can't silently rot.
- [x] **Installers fixed** — `install.sh` / `install.ps1` now resolve the newest
      1.0 release dynamically (were pinned to beta.1). Verified end-to-end as a
      fresh user: install -> `kryos new` -> run -> test -> AOT build.
- [x] **Registry verified live** — `kryos pkg add/search/info/install` all work
      against NORTHTEKDevs/kryos-registry (13 packages, sha256-pinned). Fixed a
      `pkg add` bug that wrote a malformed version to kryos.toml.
- [x] **STABILITY.md refreshed** — §4 pass rates and §5 honest residuals current.

## Your actions (need credentials or a human decision)

### 1. VSCode extension — DONE (live on the marketplace)
- [x] `northtekdevs.kryos v0.4.0` is **published and installable** on the VS Code
      Marketplace. `VSCE_PAT` is stored as a repo secret and verified valid.
- To ship a NEW version: bump `editors/vscode/package.json` `version`, then the
  next release build packages it and the `publish-vscode` workflow pushes it.
  (Re-running publish on the SAME version fails with "already exists" — expected.)
  This will be folded into the 1.0.0-final cut (bump to a 1.0-line version).

### 2. Post the announcement
A finished, IP-cleared, fact-checked draft is ready (Show HN / blog / social).
It leads with the capability wedge and the 30-second reject-the-exfiltration demo.
Ask me for it, or I can drop it into `docs/` if you want it in-repo.

### 3. Cut v1.0.0 final (a human decision — the SemVer stability lock)
rc.1 is the "last call". After a short soak (days) with no new bug reports, say
**"ship 1.0 final"** and I will:
- bump `compiler/Cargo.toml` to `1.0.0`,
- add the `[1.0.0]` CHANGELOG entry,
- re-run the full gate battery,
- tag `v1.0.0` (a bare tag auto-drops the pre-release label on GitHub),
- push and verify the release.

## WASM status (playground-ready)

The `wasm32` backend (v0.5) now covers enough of the language for a real
browser playground. Verified via the node host (`tools/wasm-host/run.mjs`):

- **Works:** i64/f64, strings, structs, enums, **arrays (mutable, in-place
  push/pop)**, **maps** (`map<str,i64>`), control flow, loops, recursion,
  generics, traits, `Result`/`Option`, casts, **no-capture closures**, and
  **higher-order functions** (`fold`/`map`/`filter` via a funcref table +
  `call_indirect`). Cross-backend parity checked (wasm == native output).
- **Not on wasm:** capturing closures (`|x| x + n`) and concurrency
  (spawn/channels/actors — single-threaded by design). Use the native
  backends for those.

Examples: `examples/wasm_maps.kry`, `examples/wasm_closures.kry`, plus the
original `wasm_*` set. All build + run.

A browser playground is now a viable build (the language subset that runs in
wasm is broad enough for real snippets). It is not yet deployed — that's a
frontend build wiring the compiler's wasm output to the JS host in a web page.
