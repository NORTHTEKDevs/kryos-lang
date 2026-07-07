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

### 1. Publish the VSCode extension (needs an Azure DevOps PAT)
The extension lives in `editors/vscode/` (publisher `northtekdevs`, v0.4.0) and a
publish workflow already exists (`.github/workflows/publish-vscode.yml`).

1. Create the publisher (once, web): https://marketplace.visualstudio.com/manage
   -> "Create publisher" -> id **must be** `northtekdevs`.
2. Mint a PAT: https://dev.azure.com -> User settings -> Personal access tokens
   -> scope **Marketplace: Manage**.
3. Store it: `gh secret set VSCE_PAT -R NORTHTEKDevs/kryos-lang`
4. Publish: run the `publish-vscode` workflow (Actions tab -> Run workflow ->
   enter the release tag, e.g. `v1.0.0-rc.1`).

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

No playground is currently deployed; if you want a browser "try it" page, that's
a separate build (the wasm target compiles today given a `wasm-ld` toolchain).
