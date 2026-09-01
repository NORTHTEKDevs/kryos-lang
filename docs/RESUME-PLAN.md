# Resume Plan -- state when the 2026-08-29 session was stopped

Written 2026-08-31 to record exactly where the v1.0.0 release effort stood
when the last session ended, and what remained. Read together with
[VERSIONING.md](../VERSIONING.md) (the 1.0.0 waiver), [CHANGELOG.md](../CHANGELOG.md)
(the `[1.0.0] - 2026-08-29` entry).

**CORRECTION (2026-08-31, release-track verification):** this file
originally linked `../LAUNCH.md` as a STALE launch doc to re-verify (item 5
below). **There is no `LAUNCH.md` in this repo** -- checked at `master`, at
`17afa1a1`, at `v0.9.0`, and at `v1.0.0-rc.1` via the contents API; all four
return 404, so the file was never committed and the link was always dead.
The launch-surface claims that item 5 wanted re-checked live in `README.md`,
`install.sh` / `install.ps1`, `docs/deploy/docker.md`, and
`editors/vscode/package.json`; they were re-verified against the actual
published `v1.0.0` release and corrected in that same commit. The
evidence is in the release-track wave entry in
[tools/loop/LEDGER.md](../tools/loop/LEDGER.md). `docs/LAUNCH-READINESS.md`
is a separate, deliberately historical document and was left verbatim.

## What was done (committed and pushed)

- `v1.0.0` was cut on 2026-08-29 with the external-workload precondition of
  VERSIONING.md **waived by the project owner** rather than met. The waiver,
  and precisely what it does and does not claim, is recorded in VERSIONING.md.
- Release prep landed as commit `17afa1a1` ("release: prepare v1.0.0 --
  version bump, waiver recorded, known limitations documented"):
  `compiler/Cargo.toml` -> `1.0.0`, CHANGELOG `[1.0.0]` entry, VERSIONING.md
  waiver, STABILITY.md / README.md / LAUNCH.md refresh. Pushed to
  `origin/master`.
- Known limitations shipped deliberately with 1.0.0 (leaks, not corruption;
  see STABILITY.md A5): struct-with-heap-fields call-boundary leak (~86MB/1M
  calls, LEDGER item 3), struct-field overwrite loop leak (~92MB/1M, item 51),
  and 41/75 pure-closure shapes needing explicit `@capabilities(all)`
  (item 41).

## The plan when stopped (remaining steps, in order)

1. **Tag `17afa1a1` as `v1.0.0` and push it.** Pushing a `v*` tag triggers
   `.github/workflows/release.yml`, which builds the Linux / Windows / macOS
   binaries and creates the public GitHub release.
   - HAZARD, now resolved: the repo locally carried a STALE `v1.0.0` tag from
     the abandoned pre-recalibration internal numbering (it pointed at a
     commit 1448 behind master, parented by the v0.5.0 release). It was
     deleted before re-tagging. The other local-only `v1.1.0`..`v4.46.0` tags
     are from that same abandoned scheme and must NEVER be pushed.
2. **Verify the release end-to-end**: all platform assets attached, release
   marked Latest (superseding v0.9.0), `install.sh` / `install.ps1` resolve
   the 1.0 release, and the installed binary reports `1.0.0`.
3. **VSCode extension**: bump `editors/vscode/package.json` to a 1.0-line
   version so `publish-vscode.yml` ships it with this release (re-publishing
   the same version fails with "already exists" -- expected).
4. **Post the announcement**: the finished, IP-cleared, fact-checked draft
   (Show HN / blog / social) leads with the capability wedge and the
   30-second reject-the-exfiltration demo (`examples/showcase/trust_agent.kry`
   + the deliberately rejected `trust_agent_overreach.kry`).
5. ~~**Re-verify LAUNCH.md**~~ -- see the CORRECTION at the top of this
   file: no such file exists. Done instead: the published-binaries /
   installers / registry / VSCode-extension claims were re-verified against
   the real `v1.0.0` release in the files that actually carry them.

## Housekeeping done in this resume session (2026-08-31)

- Stale local `v1.0.0` tag deleted; `17afa1a1` re-tagged as `v1.0.0` and
  pushed (release workflow triggered).
- `compiler/fuzz/corpus/` committed (6,761 entries, ~0.2 MB): per the
  .gitignore contract, the corpus under `compiler/fuzz/corpus/<target>/` is
  the permanent record of every fuzz finding, alongside a regression test.
  `docs/Firstclassdriver report.pdf` was left untracked pending an owner
  decision on its provenance.

## Open engineering queue (unchanged by the release)

The ranked live queue is [tools/loop/LEDGER.md](../tools/loop/LEDGER.md).
Both shipped memory leaks share one root cause (struct/enum ownership not
modelled uniformly across the two backends) and close together when the
planned unified ownership pass lands.
