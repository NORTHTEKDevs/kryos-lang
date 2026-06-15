# Project 05 -- Kryos Registry Capability Badging

**Pitch:** Every package published to the Kryos registry carries a machine-readable
capability badge; `kryos pkg show <name>` displays whether it is compute-only,
network-capable, or process-spawning before you install it, and `kryos pkg audit
<name>` fails CI if `ffi` or `process` appear in a new version without a prior
version having them.

---

## Context for a Fresh Session

This spec is self-contained. The relevant repo is at the Kryos language compiler
under `compiler/` (Rust workspace). Key crates:

- `compiler/crates/kryos-capabilities/` -- `Capability` enum, `CapabilitySet`,
  `CapabilitiesConfig`; already knows about `net, io, ffi, compute, crypto,
  process, env, term, db, time, all`
- `compiler/crates/kryos-package/` -- `Manifest`, `RegistryEntry`, `pack()`,
  `generate_index_entry()`; currently emits `name, version, dependencies,
  checksum, download_url` -- NO capability field yet
- Registry index lives at `NORTHTEKDevs/kryos-registry` on GitHub; entries are
  NDJSON (one JSON object per line) at `<prefix>/<name>.json`
- The `kryos` CLI has `kryos pkg add`, `kryos pkg sync`, no `show` or `audit`
  sub-commands yet

The standard library lives at `compiler/stdlib/*.kry`. Important stdlib modules
for this project: `std::json` (json_stringify, json_object, json_string, etc.),
`std::http` (net-capable), `std::tracked`, `std::cost`. The `@capabilities`
attribute syntax is `@capabilities(net,io,ffi)` on a function definition.

---

## Novelty Assessment

**Rating: TRULY-NOVEL**

No mainstream registry exposes per-package capability manifests or per-function
capability breakdowns. Comparison:

| Registry        | What it shows today                                   | What this adds |
|-----------------|-------------------------------------------------------|----------------|
| npm (npmjs.com) | License, README, weekly downloads, zero trust signal  | Nothing comparable |
| crates.io       | License, no sandbox model at all                      | Nothing comparable |
| PyPI            | Trove classifiers (subjective), no permissions model  | Nothing comparable |
| Deno land       | Shows which Deno permissions a module *requests* at runtime (--allow-net etc.) -- closest analog | Similar idea but runtime-requested, not compile-time proved |
| Wasm sandboxing | WASI modules are capability-gated by the host         | Host-side gate only; no registry-level manifest |

Deno is the closest prior art. The critical difference: Deno permissions are
*requested at runtime by the module itself* (the host can deny them). Kryos
capabilities are *proven at compile time* from the source. A Kryos badge says
"the compiler verified this code cannot call net-related builtins" -- not
"this code asked for net and the user accepted." That is a stronger, auditable
claim. The badge is derived by running `kryos manifest --caps` over the package
source, not inferred from readme or author declaration.

**What Kryos is NOT doing yet (be honest):**
- Deny-by-default is not implemented. Unannotated functions are unconstrained.
  The badge reflects *annotated functions only*. A package with zero
  `@capabilities` annotations emits an empty badge, not a "no capabilities"
  guarantee. This must be stated in the badge JSON (`annotation_coverage`).
- Sub-capabilities (e.g. `fs:read` vs `fs:write`) are not in the language. The
  badge granularity is the 10 top-level capabilities only.
- Runtime enforcement / sandboxing is planned but not present. The badge is a
  static-analysis output.

These limitations should appear in the badge JSON and in `kryos pkg show` output
so users know exactly what the claim means.

---

## Kryos Primitives Used

**Directly usable today:**

1. `kryos-capabilities` crate -- `Capability`, `CapabilitySet`,
   `CapabilitiesConfig`, `required_capability_for_builtin()`,
   `required_capability_for_path()`. The checker already walks the AST and
   infers capability sets for annotated functions. This is the engine.

2. `kryos-package` crate -- `Manifest`, `pack()`, `generate_index_entry()`.
   The pack step is where we hook in `kryos manifest --caps` to produce the
   sidecar. `generate_index_entry()` needs a new `capabilities` field.

3. `semver::Version` (already in `kryos-package`) -- used for version diffing
   in `kryos pkg audit`.

4. `std::json` stdlib -- `json_stringify`, `json_object`, `json_string`,
   `json_array` -- used in the Kryos-language tooling scripts and in any
   `.kry` test harness.

5. `@capabilities` annotation syntax -- confirmed in CLAUDE.md; compiler
   enforces attenuation (called fn may not exceed caller set) for annotated
   functions. The `kryos-capabilities` checker traverses the call graph.

**Language work needed FIRST:**

- The `kryos manifest --caps` sub-command does not exist. It must be added to
  `kryos-cli` (new sub-command that invokes the capability checker and emits
  JSON). This is a CLI extension, not a language change.
- `generate_index_entry()` in `kryos-package/src/registry.rs` does not include
  a `capabilities` field. Extend the struct and the JSON serializer.
- `kryos pkg show` and `kryos pkg audit` sub-commands do not exist. Add them.
- No language-level work required (the annotation and checker exist). Only
  tooling (CLI + package crate + registry schema) needs to be written.

---

## Architecture

### Components

```
kryos-cli
  kryos manifest --caps          (new) -> caps.json
  kryos pkg publish              (extend) uploads caps.json alongside tarball
  kryos pkg show <name>          (new) fetches + renders capability badge
  kryos pkg audit <name>@<ver>   (new) diffs caps vs previous version, exits 1 on escalation

kryos-package crate
  CapsBadge struct               (new) machine-readable capability summary
  pack() extension               calls capability checker, embeds CapsBadge
  generate_index_entry()         extended to include "capabilities" key
  RegistryEntry extension        adds capabilities: Option<CapsBadge>

kryos-capabilities crate
  extract_package_caps()         (new fn) -- walks all .kry files in a package
                                 and returns a CapsBadge

NORTHTEKDevs/kryos-registry
  index schema                   add "capabilities" key to each NDJSON line
  tarballs/                      keep as-is; caps.json is stored INLINE in the
                                 index entry (not as a separate file) for simplicity
```

### Data Model

**caps.json / inline index field:**

```json
{
  "schema": "kryos-caps/1",
  "package": "http-client",
  "version": "0.3.1",
  "capabilities": ["net", "crypto"],
  "per_function": {
    "fetch": ["net"],
    "verify_sig": ["crypto"],
    "parse_response": []
  },
  "annotation_coverage": {
    "annotated_fns": 12,
    "total_fns": 15,
    "coverage_pct": 80
  },
  "inferred_uncovered": ["net"],
  "dangerous": ["ffi", "process"],
  "generated_at": 1718300000
}
```

Key fields:
- `capabilities` -- union of all annotated function capability sets
- `per_function` -- per-function breakdown (omit if > 100 fns for size)
- `annotation_coverage` -- tells users how much of the package is proven vs
  uncovered; 100% = full static proof; below 100% = partial
- `inferred_uncovered` -- capabilities *inferred from builtins* in unannotated
  functions (best-effort; compiler can do this even without annotations)
- `dangerous` -- subset of capabilities that are high-risk (`ffi`, `process`);
  displayed prominently
- `schema` -- version the badging format for future evolution

**Extended `RegistryEntry` in Rust:**

```rust
pub struct RegistryEntry {
    pub name: String,
    pub version: Version,
    pub checksum: String,
    pub dependencies: HashMap<String, String>,
    pub download_url: String,
    pub capabilities: Option<CapsBadge>,   // NEW
}

pub struct CapsBadge {
    pub capabilities: Vec<String>,
    pub dangerous: Vec<String>,
    pub annotation_coverage_pct: u8,
    pub inferred_uncovered: Vec<String>,
}
```

### Kryos Code Sketches (real syntax -- no semicolons, elif, @capabilities)

**Tool: `tools/caps_checker.kry` -- reads a project and emits caps.json**

```kryos
use std::json::{json_stringify, json_object, json_string, json_array, json_number, json_bool}
use std::fs::{list_files}

struct CapsBadge {
    package: str,
    version: str,
    capabilities: [str],
    dangerous: [str],
    annotation_coverage_pct: i64,
    inferred_uncovered: [str]
}

fn is_dangerous(cap: str) -> bool {
    return cap == "ffi" or cap == "process"
}

fn badge_to_json(b: CapsBadge) -> str {
    let cap_vals: [str] = []
    let mut i = 0
    while i < len(b.capabilities) {
        let cap_vals = push(cap_vals, json_string(b.capabilities[i]))
        i = i + 1
    }

    let dangerous_vals: [str] = []
    let mut j = 0
    while j < len(b.dangerous) {
        let dangerous_vals = push(dangerous_vals, json_string(b.dangerous[j]))
        j = j + 1
    }

    let keys = ["schema", "package", "version", "capabilities",
                "dangerous", "annotation_coverage_pct"]
    let vals = [
        json_string("kryos-caps/1"),
        json_string(b.package),
        json_string(b.version),
        json_array(cap_vals),
        json_array(dangerous_vals),
        json_number(b.annotation_coverage_pct as f64)
    ]
    return json_stringify(json_object(keys, vals))
}

@capabilities(io)
fn main() {
    let args_list = args()
    if len(args_list) < 2 {
        println("usage: caps_checker <project-dir>")
        return
    }
    let project_dir = args_list[1]
    // In practice the CLI invokes the Rust capability checker;
    // this .kry tool is a thin wrapper for scripting/testing.
    let badge_json = file_read(project_dir + "/target/caps.json")
    println(badge_json)
}
```

**`kryos pkg show` output (rendered in CLI, not Kryos code):**

```
kryos-pkg: http-client v0.3.1
  Capabilities : net, crypto
  Dangerous    : (none)
  Coverage     : 80% of functions annotated (12/15)
  Uncovered    : net (inferred from builtins in unannotated fns)
  Note: coverage < 100% -- badge is not a full sandbox proof
```

**`kryos pkg audit` logic (Rust in kryos-cli, pseudocode):**

```rust
fn audit(name: &str, new_ver: &str, prev_ver: &str) -> Result<(), String> {
    let prev = registry.lookup_version(name, prev_ver)?;
    let next = registry.lookup_version(name, new_ver)?;
    let prev_caps = prev.capabilities.unwrap_or_default();
    let next_caps = next.capabilities.unwrap_or_default();

    let escalations: Vec<_> = next_caps.dangerous
        .iter()
        .filter(|c| !prev_caps.dangerous.contains(c))
        .collect();

    if !escalations.is_empty() {
        eprintln!("AUDIT FAIL: {name}@{new_ver} adds dangerous capabilities: {:?}", escalations);
        std::process::exit(1);
    }
    // Also check any NEW capability (not just dangerous)
    let new_caps: Vec<_> = next_caps.capabilities
        .iter()
        .filter(|c| !prev_caps.capabilities.contains(c))
        .collect();
    if !new_caps.is_empty() {
        println!("WARN: {name}@{new_ver} adds new capabilities: {:?}", new_caps);
        println!("Run with --strict to fail on any capability addition.");
    }
    Ok(())
}
```

**Example annotated package source (what a package author writes):**

```kryos
// src/main.kry in package "weather-fetcher"

use std::http::{get, Response}
use std::json::{json_parse}

@capabilities(net)
fn fetch_weather(city: str) -> str {
    let url = "https://api.weather.example.com/v1/current?q=" + city
    let resp = get(url)
    return resp.body
}

// No annotation: unannotated, will show in inferred_uncovered
fn parse_temp(json_body: str) -> f64 {
    // ... parsing logic
    return 21.5
}

fn main() {
    let data = fetch_weather("Anchorage")
    let temp = parse_temp(data)
    println("Temperature: " + to_string(temp))
}
```

After `kryos pkg publish`, the registry entry for `weather-fetcher` carries:
```json
"capabilities": {
  "schema": "kryos-caps/1",
  "capabilities": ["net"],
  "dangerous": [],
  "annotation_coverage_pct": 67,
  "inferred_uncovered": []
}
```

---

## MVP Scope (Smallest Shippable Slice)

**MVP (buildable today):**

1. `kryos manifest --caps` sub-command in `kryos-cli`: runs the existing
   `kryos-capabilities` checker over the project source, collects all
   `CapabilitySet`s from annotated functions, unions them, and writes
   `target/caps.json` with the `CapsBadge` JSON.

2. Extend `generate_index_entry()` in `kryos-package/src/registry.rs`:
   if `target/caps.json` exists in the project when `pack()` is called,
   embed it as a `"capabilities"` field in the index entry JSON. If absent,
   omit the field (backward-compatible -- existing entries without the field
   still parse fine via the `Option<CapsBadge>` type).

3. Extend `parse_index_entry()` to read the optional `"capabilities"` field
   from NDJSON and populate `RegistryEntry.capabilities`.

4. `kryos pkg show <name>` sub-command: syncs registry, looks up the latest
   version of `<name>`, and prints the capability summary table to stdout.
   If `capabilities` is absent (old package), prints "No capability badge --
   package predates capability badging."

5. `kryos pkg audit <name>` sub-command: compares capability badge of the
   latest version vs the previous version and exits 1 if `ffi` or `process`
   appears for the first time. Prints a clear diff.

6. Update `http-router` (the existing demo package in the registry) to include
   a `caps.json` generated by `kryos manifest --caps` so the feature is
   immediately demonstrable on a real package.

**Out of scope for MVP:**
- Web UI on kryos-playground showing badges (project 06 does this)
- Sub-capability granularity (fs:read vs fs:write) -- needs language work
- Per-function breakdowns in the show output (too verbose for v1)
- Coverage enforcement (failing publish if coverage < 80%) -- v2 feature
- Deny-by-default audit (would require full-coverage annotation pass first)

**Full vision (v2):**
- `kryos pkg publish` fails if annotation coverage < configurable threshold
- Per-function capability map in `show --verbose`
- `kryos pkg audit --strict` fails on ANY new capability (not just dangerous)
- kryos-playground UI renders capability badge as colored chips
- Registry search supports `kryos pkg search --no-net` (filter by caps)
- Badge shows trust tier: PROVEN (100% coverage), PARTIAL (>=50%), INFERRED (<50%)

---

## Build Plan (Ordered Steps for a Fresh Session)

**Step 0 -- Read before writing**

Read these files before touching anything:
- `compiler/crates/kryos-capabilities/src/model.rs` -- Capability enum,
  CapabilitySet
- `compiler/crates/kryos-capabilities/src/checker.rs` -- capability checker
- `compiler/crates/kryos-package/src/registry.rs` -- RegistryEntry,
  generate_index_entry(), parse_index_entry()
- `compiler/crates/kryos-package/src/manifest.rs` -- Manifest, CapabilitiesConfig
- `compiler/crates/kryos-cli/src/` -- existing CLI sub-command structure

**Step 1 -- CapsBadge struct and serialization in `kryos-package`**

Add to `compiler/crates/kryos-package/src/registry.rs` (or a new
`caps.rs` module):

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapsBadge {
    pub schema: String,                     // "kryos-caps/1"
    pub capabilities: Vec<String>,          // sorted union of all caps
    pub dangerous: Vec<String>,             // subset: ffi, process
    pub annotation_coverage_pct: u8,        // 0..100
    pub inferred_uncovered: Vec<String>,    // caps found in unannotated fns
}

impl CapsBadge {
    pub fn dangerous_caps() -> &'static [&'static str] {
        &["ffi", "process"]
    }

    pub fn from_capability_set(
        caps: &CapabilitySet,
        annotated_fns: usize,
        total_fns: usize,
        inferred: Vec<String>,
    ) -> Self {
        let mut capabilities: Vec<String> = caps
            .iter()
            .map(|c| c.to_string())
            .collect();
        capabilities.sort();
        let dangerous = capabilities
            .iter()
            .filter(|c| Self::dangerous_caps().contains(&c.as_str()))
            .cloned()
            .collect();
        let pct = if total_fns == 0 { 0 }
                  else { (annotated_fns * 100 / total_fns) as u8 };
        CapsBadge {
            schema: "kryos-caps/1".into(),
            capabilities,
            dangerous,
            annotation_coverage_pct: pct,
            inferred_uncovered: inferred,
        }
    }
}
```

Extend `RegistryEntry`:

```rust
pub struct RegistryEntry {
    pub name: String,
    pub version: Version,
    pub checksum: String,
    pub dependencies: HashMap<String, String>,
    pub download_url: String,
    pub capabilities: Option<CapsBadge>,   // NEW -- None for old entries
}
```

**Step 2 -- `extract_package_caps()` in `kryos-capabilities`**

Add a public function that, given a project directory, walks all `.kry` source
files, runs the existing capability checker over each, and returns a `CapsBadge`.

The capability checker already exists (`checker.rs`); this function is a
thin orchestrator:

```rust
pub fn extract_package_caps(src_dir: &Path) -> Result<CapsBadge, String> {
    // Walk src_dir for *.kry files
    // For each file: parse + run checker -> get CapabilitySet for each fn
    // Union all annotated fn sets -> total_caps
    // Count annotated vs total fns
    // Best-effort infer caps of unannotated fns from builtin calls
    // Return CapsBadge::from_capability_set(...)
}
```

**Step 3 -- `kryos manifest --caps` CLI sub-command**

Add to `kryos-cli` alongside existing sub-commands. The sub-command:
1. Reads `kryos.toml` to get package name + version
2. Calls `extract_package_caps("src/")`
3. Writes `target/caps.json` (creates `target/` if absent)
4. Prints a human-readable summary to stdout

Verify with: `kryos manifest --caps` in a project with annotated functions
should write `target/caps.json` and print "capabilities: net, io" etc.

**Step 4 -- Extend `pack()` and `generate_index_entry()`**

In `pack()`: after assembling files, check if `target/caps.json` exists; if so,
include it as `caps.json` in the package listing and embed a `caps_badge` field
on the returned `PublishPackage` struct.

In `generate_index_entry()`: serialize `caps_badge` as `"capabilities": {...}`
in the JSON output. If `caps_badge` is `None`, omit the field entirely (no null
-- just omit, so old packages remain valid).

**Step 5 -- Extend `parse_index_entry()`**

Add optional parsing of the `"capabilities"` field from the NDJSON index line.
On parse failure or field absence, set `capabilities: None`.

The existing minimal parser in `registry.rs` uses hand-written string scanning.
Either extend it with a `capabilities` key extraction, or switch to `serde_json`
for the badge sub-object (serde is already a workspace dep). Serde is preferred
for correctness.

**Step 6 -- `kryos pkg show <name>` CLI sub-command**

```
$ kryos pkg show weather-fetcher

Package   : weather-fetcher v0.3.1
Author    : (from index if present)
Capabilities : net, crypto
Dangerous    : (none)
Coverage     : 80% annotated (12/15 functions)
Uncovered    : net (inferred from builtins in unannotated functions)
Warning  : Coverage < 100%. Badge is not a full sandbox guarantee.

Previous versions with different badges:
  v0.3.0  net (same)
  v0.2.0  (no badge)
```

Use `RegistryClient::lookup(name)` -- already exists. Pick the latest version,
display the badge. If `capabilities` is `None`, display the "no badge" message.

**Step 7 -- `kryos pkg audit <name>` CLI sub-command**

```
$ kryos pkg audit http-client

Comparing http-client v0.4.0 (latest) vs v0.3.1 (previous)
  v0.3.1 capabilities: net
  v0.4.0 capabilities: net, ffi   <-- NEW

AUDIT FAIL: ffi is a dangerous capability and was not present in the previous
version. Review the foreign-function calls added in v0.4.0 before installing.

Exit code: 1
```

For CI integration: `kryos pkg audit http-client` in a CI step will fail the
build if ffi or process appears for the first time. Use `--strict` flag to fail
on any new capability.

**Step 8 -- Update registry demo package**

Run `kryos manifest --caps` inside the `http-router` package (the demo package
already in `kryos-registry`). Copy the resulting `target/caps.json` into the
registry index entry. Commit and push.

**Step 9 -- Tests**

In `kryos-package/tests/package.rs`:
- `test_caps_badge_serialization()` -- CapsBadge round-trips through JSON
- `test_generate_index_entry_with_caps()` -- `generate_index_entry()` includes
  capability field
- `test_parse_entry_with_caps()` -- `parse_index_entry()` reads badge back
- `test_parse_entry_no_caps()` -- old entries without `capabilities` field
  parse to `None` without error
- `test_audit_detects_dangerous_escalation()` -- audit exits 1 when ffi added

In `kryos-capabilities/tests/capabilities.rs`:
- `test_extract_package_caps_empty_src()` -- empty src dir returns empty badge
- `test_extract_package_caps_net_annotated()` -- a .kry file with
  `@capabilities(net)` on a fn returns `capabilities: ["net"]`

---

## Kryos Tooling Integration Points

The `kryos.toml` manifest already has a `[capabilities]` section
(`CapabilitiesConfig { allowed: Vec<String> }`). The badge can cross-reference
this: if the badge shows a capability that is NOT in `capabilities.allowed`,
the CLI warns that the installed package exceeds the declared project policy.

This sets up a future "policy-gated install" where `kryos pkg add` refuses to
install a package whose badge includes capabilities not in `[capabilities].allowed`.
The groundwork (manifest field already exists) is already there.

---

## Success Criteria / Demo

The feature is demonstrable when all of the following are true:

1. `kryos manifest --caps` runs in any Kryos project and produces
   `target/caps.json` with a valid `CapsBadge`.

2. `kryos pkg show http-router` prints a capability summary including
   the badge from the live registry (not "no badge").

3. `kryos pkg audit http-router` exits 0 (no escalation between last
   two versions of `http-router`).

4. A test package with `@capabilities(ffi)` added in v0.2.0 causes
   `kryos pkg audit test-pkg` to exit 1 with a clear escalation message.

5. `cargo test -p kryos-package` and `cargo test -p kryos-capabilities`
   pass with all new test cases green.

**Demo script for announcement:**
```
# Install a package and inspect it before it runs on your machine
kryos pkg show serde-kryos

Package   : serde-kryos v1.2.0
Capabilities : (none)
Dangerous    : (none)
Coverage     : 100%
=> Safe to install: this package makes no system calls.

kryos pkg show http-client
Package   : http-client v0.3.1
Capabilities : net
Dangerous    : (none)
Coverage     : 85%
=> Network-capable: this package opens outbound TCP connections.

kryos pkg show native-plugin
Package   : native-plugin v0.1.0
Capabilities : ffi, net
Dangerous    : ffi
Coverage     : 60%
Warning: Contains FFI (C function calls). Review before installing.
```

This is the demo line: "No other package registry shows you this before you install."

---

## Risks and Honest Unknowns

**Risk 1 -- Badge spoofing (HIGH -- must address in v1)**

The badge is generated from source and included in the index entry. A malicious
publisher could lie: write one set of source (clean), generate the badge, then
swap in different source for the tarball. Mitigation (v1): the `kryos pkg show`
output must display "Badge generated from source at publish time. It reflects
source audited by the author's machine, not a third-party verifier." Full
mitigation (v2): the registry CI re-runs `kryos manifest --caps` against the
uploaded tarball and overwrites the badge. This requires registry-side
infra (CI workflow in `kryos-registry` repo).

**Risk 2 -- Coverage gap (MEDIUM)**

Because capabilities are opt-in per annotated function today, a package can
have 0% coverage and an empty badge. The badge is then uninformative. Mitigation:
display `annotation_coverage_pct` prominently; add a future `kryos pkg publish
--require-coverage=80` flag. Do NOT suppress the badge when coverage is low --
that would hide the gap.

**Risk 3 -- Checker performance (LOW for MVP)**

`extract_package_caps()` walks all .kry source files. For large packages this
could be slow. The capability checker already exists and is fast (compile-time
analysis). No concern for typical package sizes (< 10k lines). Cache in
`target/caps.json` avoids re-running on every `pkg show`.

**Risk 4 -- Registry schema backward compatibility (LOW)**

The `"capabilities"` field is additive. All existing NDJSON lines without it
parse to `RegistryEntry { capabilities: None }`. No migration needed. Confirmed
by reading `parse_index_entry()` which uses `extract_json_string` which returns
`None` for absent fields -- just need to extend with optional capability parsing.

**Risk 5 -- Sub-capability false comfort (MEDIUM)**

`"io"` covers both read and write. A user might see `"io"` and think "it only
reads files" when it can also write. The badge display should note that top-level
capabilities are coarse-grained (no sub-capability support yet). Long term:
when `fs:read` / `fs:write` sub-capabilities land in the language, the badge
schema version bumps from `kryos-caps/1` to `kryos-caps/2`.

**Unknown: kryos-registry repo structure**

The spec assumes the registry index is at `NORTHTEKDevs/kryos-registry` with
NDJSON entries at `<prefix>/<name>.json`. Confirmed from `registry.rs`:
`parse_index_entry()` and `search()` use exactly this layout. Before step 8,
verify the current `http-router` entry format by reading the live repo.

---

## Depends On

Project 01 (`kryos manifest --caps` baseline). If project 01 has not been built,
the `kryos manifest --caps` sub-command and `extract_package_caps()` must be
implemented from scratch here. They do not exist in the current codebase -- this
is green-field CLI work on top of the existing `kryos-capabilities` checker
infrastructure.

---

## File Locations to Edit

```
compiler/crates/kryos-capabilities/src/
  lib.rs          -- pub use extract_package_caps
  extract.rs      -- NEW: extract_package_caps() implementation

compiler/crates/kryos-package/src/
  registry.rs     -- CapsBadge struct, extend RegistryEntry, extend
                     generate_index_entry(), extend parse_index_entry()
  lib.rs          -- pub use CapsBadge

compiler/crates/kryos-cli/src/
  main.rs         -- add manifest --caps, pkg show, pkg audit sub-commands
  commands/
    manifest.rs   -- NEW or extend existing
    pkg.rs        -- extend with show, audit

compiler/crates/kryos-package/tests/
  package.rs      -- new test cases for CapsBadge

compiler/crates/kryos-capabilities/tests/
  capabilities.rs -- new test cases for extract_package_caps
```
