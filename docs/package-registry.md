# Kryos Package Registry

This document specifies the **on-disk format**, **distribution protocol**,
and **operational model** of the Kryos package registry. The current client
implementation lives in [`compiler/crates/kryos-package`](../compiler/crates/kryos-package).
A reference server implementation lives in [`tools/registry`](../tools/registry).

> **Status: this is still the design spec, not a byte-for-byte description
> of current client behavior — the client now DOES verify a checksum
> before trusting a package (LEDGER item 1b is CLOSED), but the mechanism
> differs from what's described below in two ways.** First, the hash
> algorithm is SHA-256, not BLAKE3 -- the pure-Rust `kryos-package/src/
> sha256.rs` implementation, chosen to avoid a new external dependency.
> Second, `kryos pkg install` never fetches or verifies a "tarball": it
> still `git clone --depth 1`s the registry repo's current HEAD and copies
> out `packages/<name>/<version>/` directly (`kryos-package/src/
> fetch.rs::fetch_github_subdir`). What IS real now: the index's
> `checksum` field is a canonical `sha256:<hex>` hash of the package's
> actual `kryos.toml` + `src/**.kry` (+ `stdlib/**.kry` if present) content
> in deterministic sorted-path order (`kryos-package/src/
> registry.rs::content_checksum`) -- the SAME function computes it at
> publish time (`pack()`) and recomputes it over the fetched/cached
> directory at install time, and `fetch::fetch_resolved` rejects a
> mismatch OR a missing checksum before the package is used, including on
> a cache hit (so a package tampered with on disk after a previous install
> is caught, not silently reused). `kryos.lock` now records the verified
> checksum instead of always leaving the field empty. `copy_dir_all` also
> refuses any symlink entry encountered while copying a fetched package,
> so a malicious commit cannot use a symlink to pull unrelated files from
> outside the package into the local cache. See `tools/loop/LEDGER.md`
> item 1b (CLOSED table) for the live repro, the fix, and what's still
> aspirational in this doc (BLAKE3, true tarball transport, IPFS/S3
> hosting) versus implemented today.

## Design goals

1. **Zero-trust bootstrap** — every download is content-addressed
   (BLAKE3 hash) and verifiable offline.
2. **Git-native index** — the registry index is a public Git repository,
   so anyone can mirror it without server cooperation.
3. **Optional hosting** — the actual package tarballs can sit anywhere
   (GitHub Releases, S3, IPFS); the index just points at a URL.
4. **Decentralized publish** — publishing is a Pull Request against the
   index repository, not an upload to a privileged HTTP endpoint.
5. **No marketing** — the registry exists to serve packages; metadata
   is strictly technical.

## Architecture

```
                ┌──────────────────────────────┐
                │  Kryos client (kryos pkg)    │
                │  crate: kryos-package         │
                └─────────────┬────────────────┘
                              │
        ┌─────────────────────┴────────────────────┐
        │                                          │
        ▼ (1) git clone/pull index                 ▼ (2) HTTPS GET tarball
┌─────────────────────────┐              ┌──────────────────────────────┐
│  kryos-registry (git)   │              │  Tarball host (e.g. GitHub   │
│  Index JSON per package │              │  Releases / S3 / IPFS)       │
└─────────────────────────┘              └──────────────────────────────┘
        ▲                                          ▲
        │ (3) PR to publish                        │ (3) upload tarball
        │                                          │
        └────────────── publisher ─────────────────┘
```

## On-disk index format

The registry index is a Git repository with this layout:

```
kryos-registry/
├── README.md
├── config.toml            # registry-wide config: tarball base URL, schema version
└── <prefix>/<name>.json   # one file per package
```

The `<prefix>` is the first two characters of the package name, lowercase.
For names shorter than 2 characters, `<prefix>` is the whole name.

### Index entry format

Each line of `<prefix>/<name>.json` is a single JSON object representing
one **published version** of the package:

```json
{"name": "math_utils", "version": "1.2.3", "deps": {"core_io": "^1.0"}, "checksum": "blake3:abcd...", "download_url": "https://github.com/foo/math_utils/releases/download/v1.2.3/math_utils-1.2.3.tar.gz"}
{"name": "math_utils", "version": "1.2.4", "deps": {"core_io": "^1.0"}, "checksum": "blake3:efgh...", "download_url": "https://github.com/foo/math_utils/releases/download/v1.2.4/math_utils-1.2.4.tar.gz"}
```

One JSON object per line (NDJSON) so appends never rewrite history.

Required fields:

| Field | Type | Description |
|---|---|---|
| `name` | string | Package name; must match the file's `<name>` |
| `version` | string | Semver `MAJOR.MINOR.PATCH` |
| `deps` | object | Map of dep name → semver spec (`"^1.0"`, `">=0.5"`, etc.) |
| `checksum` | string | `blake3:<hex>` of the tarball contents |
| `download_url` | string | Direct HTTPS URL to the tarball |

Optional fields (any client must tolerate unknown fields):

- `yanked` (bool) — version is published-but-deprecated; clients warn
- `kryos_version` (string) — minimum Kryos version constraint
- `features` (object) — feature flags for conditional compilation
- `description`, `license`, `homepage`, `repository` — informational only

### `config.toml` at the registry root

```toml
schema_version = 1
index_url      = "https://github.com/NORTHTEKDevs/kryos-registry"
tarball_base   = "https://github.com/NORTHTEKDevs/kryos-registry/releases/download"
```

The client respects `schema_version`; mismatched versions trigger a hard
error rather than silent misbehavior.

## Client protocol

### `kryos pkg sync`

Clones (first time) or `git pull --ff-only` the index repository into
`~/.kryos/registry/index/`. This is the only "network" operation that
talks to the index — everything else is local.

### `kryos pkg search <query>`

Walks the local index cache and substring-matches `<query>` against
package names.

### `kryos pkg info <name>`

Reads `~/.kryos/registry/index/<prefix>/<name>.json` and prints all
published versions, deps, and checksums.

### `kryos pkg install`

The design intent (below) vs. current implementation, since they differ:
there is no `.tar.gz`/`download_url` fetch or BLAKE3 hash today -- the
client `git clone`s the registry repo and copies out
`packages/<name>/<version>/`, then verifies a SHA-256 content hash over
that directory. See the status callout at the top of this document for
the exact current mechanism.

1. Reads the project's `kryos.toml`.
2. Resolves dependencies against the local index (`crate::resolve`).
3. For each pinned `(name, version)`:
   - If `~/.kryos/registry/cache/<name>-<version>.tar.gz` exists *and*
     its BLAKE3 hash matches `checksum`, skip.
   - Otherwise, HTTPS GET the `download_url`, verify the BLAKE3 hash,
     cache locally.
   - Either way, reject the package (and remove it from the cache) if
     the checksum is missing or does not match -- an unverifiable or
     tampered package must never install silently.
4. Writes `kryos.lock` with concrete versions and checksums.

### `kryos pkg publish`

1. Runs `crate::registry::pack()` to produce
   `target/package/<name>-<version>.tar.gz`.
2. Generates a JSON index entry via `generate_index_entry()`.
3. Prints both. The author is then responsible for:
   - Uploading the tarball to their chosen host (or the canonical
     registry's release page if they have write access).
   - Opening a PR against `kryos-registry` that appends the JSON line
     to `<prefix>/<name>.json`.

This is intentional — there is no implicit upload step. The author
must explicitly publish-host the tarball, and the index lives behind
human review (the PR).

## Reference server: `tools/registry`

The reference server in `tools/registry` is a small Rust HTTP service
that exposes a **read-only API** over the index for clients that prefer
HTTP to Git pulls:

```
GET /v1/packages/<name>           → all versions (NDJSON)
GET /v1/packages/<name>/<version> → single version (JSON)
GET /v1/search?q=<query>          → matching package names (JSON array)
GET /v1/health                    → "ok"
```

The server keeps a local Git checkout of the index, periodically `git
pull`s, and serves JSON from memory. It has **no write endpoints** —
publishing is still PR-based.

Deployable to any platform that can run a small Rust binary: bare
metal, Docker, Fly.io, Modal, or a single VPS. See
[`tools/registry/README.md`](../tools/registry/README.md) for run
instructions.

## Hosting decisions (deferred)

The canonical registry URL `https://github.com/NORTHTEKDevs/kryos-registry`
is hardcoded as the client default but is **not yet provisioned**.
The decisions still owed:

- **Index repo**: create `NORTHTEKDevs/kryos-registry` (public, MIT
  licensed) with the schema above.
- **Tarball host**: GitHub Releases is the path of least resistance;
  alternative is an S3 bucket behind CloudFront.
- **Public HTTP gateway**: optional; only needed if we want HTTPS
  search/info without a `git pull`. The reference server in
  `tools/registry` can be deployed to Fly.io / Modal / a VPS for ~$5/month.
- **Publish governance**: PR-based with a CODEOWNERS file gating
  changes to specific package paths once a package has an established
  author.

These are deployment-time choices, not specification choices, so they
do not block 1.0.0-beta.1 of the toolchain.

## Versioning policy

- The **schema** is versioned via `config.toml`'s `schema_version`.
- The **client** treats unknown fields as forward-compatible (ignored).
- Yanked versions stay in the index forever but clients must warn and
  must not auto-upgrade to them.
- Once published, a `(name, version)` is **immutable**. To "fix" a bad
  release, yank it and publish a new version.

## Security

- Every tarball download is verified against the BLAKE3 checksum in
  the index. A mismatch is a hard error — never a warning.
- The index is served over HTTPS / SSH (Git). The Git history itself
  is the audit log.
- The reference server has no write endpoints, so it has no
  publish-time attack surface. The only state it owns is a local Git
  cache.

## What's intentionally out of scope

- **A web UI for browsing packages.** The Git web view is enough.
- **Automated dependency security scanning.** Would belong in a
  separate CI tool, not the registry.
- **Private packages.** Self-host the index repo and point
  `kryos.toml`'s `[registry]` table at it.
- **Multi-registry resolution.** A package name maps to exactly one
  registry per project. Vendoring is the escape hatch.
