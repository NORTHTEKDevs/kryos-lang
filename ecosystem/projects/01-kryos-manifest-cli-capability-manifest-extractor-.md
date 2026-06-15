# kryos manifest --caps: Capability Manifest Extractor

**One-liner:** `kryos manifest --caps <path>` serializes the per-function capability set of any `.kry` source into machine-readable JSON -- no binary inspection, no source reading by external tools required.

---

## Why this is novel

**Novelty rating: TRULY-NOVEL** (for the specific combination; individual parts exist elsewhere)

The per-function capability manifest is novel as a first-class compiler-native artifact for a general-purpose language. Here is the honest breakdown:

- **wasm-bindgen / Wasm component model** can describe import/export surfaces, but only for the Wasm ABI boundary, not arbitrary in-process function calls. It says nothing about what a function may do inside the sandbox.
- **Solidity / move-lang** have capability or permission models, but only for smart-contract resource access, not general-purpose language functions.
- **Java / C# / Android manifest** declare app-level permissions (network, camera), not per-function capability sets, and only for the whole binary.
- **Rust effects crates** (`effectful`, `effect-lang` experiments) exist as third-party libraries, not as first-class compiler data. They are incomplete and not serializable by the compiler toolchain.
- **OpenTelemetry / static analysis tools** (Semgrep, CodeQL) can detect capability-like patterns in source, but they work on heuristic AST patterns, not the compiler's canonical computed capability sets.
- **`kryos audit`** (already in the CLI) does a text/AST scan for capability annotations. It reports which functions have `@capabilities(...)` annotations grouped by capability name. It does NOT: (a) include functions without annotations (unannotated functions are unconstrained today), (b) produce a stable machine-readable artifact keyed by function name, (c) emit a diff-able manifest for CI gate use.

**What Kryos offers that nobody else does:** The compiler's `kryos-capabilities` crate already runs `build_fn_capability_map()` to build `HashMap<String, CapabilitySet>` during every compile. The `CapabilitySet` is the ground truth: it comes from `CapabilitySet::from_annotations()` and is the same data the type-checker uses to enforce attenuation. Serializing this map as a stable JSON artifact is a one-layer addition that produces a per-function manifest grounded in the compiler's own enforcement model -- not a secondary scan.

No mainstream language registry produces a per-function capability manifest as a compiler-native output. This is the foundation for:
- Registry package badging ("this package is net-only, no io, no ffi")
- CI diff-gates ("PR added ffi to fn foo -- block and require review")
- Playground sandbox enforcement (runner refuses to execute code whose manifest includes `process`)
- Third-party auditors and compliance tools that need a stable artifact, not source parsing

**Important caveats to be honest about:**

1. Today, unannotated functions are unconstrained -- they do not appear in the manifest with a capability set. The manifest reflects declared capabilities only. A function with no `@capabilities(...)` annotation is absent from the output, which means "unconstrained" not "zero caps". The spec must document this clearly in the JSON schema.

2. The "deny-by-default" mode (where unannotated functions are inferred to have zero capabilities) is PLANNED but not implemented. The manifest extractor should add a `--strict` flag that maps unannotated functions to `[]` (zero caps) to let CI block on them, but this is a flag, not the default.

3. Sub-capabilities (e.g. `fs:read` vs `fs:write`) are NOT implemented. The manifest emits top-level capability names only: `net`, `io`, `ffi`, `compute`, `crypto`, `process`, `env`, `term`, `db`, `time`, `all`.

---

## Which Kryos primitives it uses

### Compiler-side (Rust crates -- where the actual work happens)

- `kryos-capabilities` crate: `check_capabilities(module)`, `CapabilitySet`, `Capability`, `build_fn_capability_map()` (already public in checker.rs). The manifest command will call the same pipeline that `kryos audit` uses: `kryos_lexer::Lexer::new` -> `kryos_parser::parse` -> walk `module.declarations` for `Decl::Function { name, annotations, .. }` and call `CapabilitySet::from_annotations(annotations)`.
- `kryos-cli` crate: add a `Manifest` subcommand to `Commands` enum in `main.rs` and a `manifest_cmd.rs` in `commands/`. Pattern: identical to `audit_cmd.rs` (same lexer+parser+AST walk) but narrower output schema.
- `kryos-ast` crate: `Decl`, `Annotation` types (already used by audit).
- `kryos-lexer` + `kryos-parser`: already available in cli crate's dependencies.

### Kryos-side (stdlib modules used by test/demo programs)

The manifest command itself is Rust. But the test fixtures and the CI gate script are written in Kryos:

- `std::json` (json_stringify, json_object, json_string, json_array, json_parse) -- for building and parsing the manifest JSON
- `std::fs` (file_read, file_write, file_exists) -- reading fixture files, writing manifest output
- `std::process` -- (in the CI gate script) running `kryos manifest --caps`, capturing output
- `std::result` (`Result<T, E>`, `Ok`, `Err`) -- error handling in the gate script
- `std::string` (split_lines, contains) -- parsing diff output

### Language features used in code sketches

- `@capabilities(io)` on functions that call file_read/file_write
- `@capabilities(process)` on functions that spawn the compiler
- No `@budget` needed (no LLM calls)
- `Shared<T>` not needed (no shared heap state)
- `Tracked<T>` not needed (data provenance is the manifest's purpose, not its mechanism)
- `Probable<T>` not needed

### No language work required

The manifest extractor requires zero changes to the Kryos language or type system. It purely serializes data already computed during the capability check pass. This is why it is buildable today.

---

## Architecture

### Components

```
kryos-cli
  commands/manifest_cmd.rs      -- new file: walk .kry files, build manifest, emit JSON
  main.rs                       -- add Manifest variant to Commands enum

kryos-capabilities (no changes needed)
  src/checker.rs                -- build_fn_capability_map() already public
  src/model.rs                  -- CapabilitySet, Capability already public

fixtures/
  tests/manifest/               -- .kry test files with various @capabilities combos
  tests/manifest/expected/      -- expected JSON outputs for golden tests

tools/
  ci-cap-gate.kry               -- Kryos script: run manifest, compare to baseline, fail on new caps
```

### Data model (JSON schema)

```json
{
  "schema": "kryos-manifest-v1",
  "generated_at": "2026-06-14T00:00:00Z",
  "source": "src/agent.kry",
  "functions": {
    "chat": {
      "capabilities": ["net"],
      "annotated": true
    },
    "send_report": {
      "capabilities": ["net", "io"],
      "annotated": true
    },
    "compute_hash": {
      "capabilities": [],
      "annotated": false
    }
  },
  "unannotated_count": 12,
  "notes": [
    "unannotated functions are unconstrained (not zero-capability) unless --strict is passed"
  ]
}
```

Key schema decisions:
- `annotated: true/false` distinguishes "explicitly declared zero caps" from "not annotated" (the latter is `annotated: false, capabilities: []` in `--strict` mode, absent in default mode)
- `unannotated_count` gives CI a number to watch without bloating the output
- When scanning a directory, output is an array of per-file manifest objects (or a single merged object with `"source"` as a map key)

### Rust implementation sketch (manifest_cmd.rs)

```rust
use kryos_ast::Decl;
use kryos_capabilities::model::{Capability, CapabilitySet};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct ManifestOptions {
    pub path: Option<String>,
    pub format: String,   // "json" (default) or "pretty"
    pub strict: bool,     // include unannotated fns as capability=[]
    pub output: Option<String>,
    pub caps_filter: Vec<String>,  // only emit fns with at least one of these caps
}

struct FnEntry {
    capabilities: Vec<String>,
    annotated: bool,
}

struct FileManifest {
    source: PathBuf,
    functions: BTreeMap<String, FnEntry>,
    unannotated_count: usize,
}

pub fn execute(opts: ManifestOptions) -> Result<(), String> {
    // 1. collect .kry files (same pattern as audit_cmd.rs)
    // 2. for each file: lex -> parse -> walk decls -> build FnEntry map
    // 3. serialize to JSON or pretty-print
    // 4. write to --output or stdout
    // 5. exit non-zero if any fn has a capability in --deny list (CI gate mode)
}

fn scan_file(path: &Path, strict: bool) -> Option<FileManifest> {
    let source = std::fs::read_to_string(path).ok()?;
    let tokens = kryos_lexer::Lexer::new(&source, 0).tokenize();
    let module = kryos_parser::parse(tokens).ok()?;

    let mut functions: BTreeMap<String, FnEntry> = BTreeMap::new();
    let mut unannotated = 0usize;

    for decl in &module.declarations {
        if let Decl::Function { name, annotations, .. } = decl {
            let has_caps_ann = annotations.iter().any(|a| a.name == "capabilities");
            if has_caps_ann {
                let caps = CapabilitySet::from_annotations(annotations);
                let cap_names: Vec<String> = if caps.has(Capability::All) {
                    vec!["all".into()]
                } else {
                    let mut v: Vec<String> = caps.iter().map(|c| c.to_string()).collect();
                    v.sort();
                    v
                };
                functions.insert(name.clone(), FnEntry {
                    capabilities: cap_names,
                    annotated: true,
                });
            } else {
                unannotated += 1;
                if strict {
                    functions.insert(name.clone(), FnEntry {
                        capabilities: vec![],
                        annotated: false,
                    });
                }
            }
        }
    }
    Some(FileManifest { source: path.to_path_buf(), functions, unannotated_count: unannotated })
}
```

### Kryos CI gate script (ci-cap-gate.kry)

```kryos
// ci-cap-gate.kry
// Usage: kryos run ci-cap-gate.kry -- src/ baseline.json
// Exit 1 if any function has a capability not in the baseline.
// Pipe the result to a PR comment or fail the CI step.

use std::json::{json_parse, json_get, json_to_str, json_keys, json_is_null, json_to_int, json_length, json_get_index}
use std::result::{Result, Ok, Err}

fn temp_dir_path() -> str {
    let t = env_get("TEMP")
    if len(t) > 0 { return t }
    let t2 = env_get("TMP")
    if len(t2) > 0 { return t2 }
    return "/tmp"
}

@capabilities(process, io)
fn run_manifest(src_path: str) -> str {
    let out_path = temp_dir_path() + "/kryos-manifest-out.json"
    // kryos manifest --caps outputs JSON to stdout; redirect to temp file
    // In practice: use std::process to run the command and capture stdout
    // For CI: `kryos manifest --caps src/ --format json --output out_path`
    let _ = file_write(out_path, "")  // ensure file exists for error case
    return out_path
}

@capabilities(io)
fn load_json(path: str) -> i64 {
    let content = file_read(path)
    return json_parse(content)
}

@capabilities(io)
fn check_new_caps(manifest_path: str, baseline_path: str) -> Result<i64, str> {
    if file_exists(baseline_path) == 0 {
        return Err("baseline not found: " + baseline_path + " -- run with --init to create")
    }
    let manifest = load_json(manifest_path)
    let baseline = load_json(baseline_path)
    // Walk manifest.functions, compare caps to baseline.functions[fn_name].capabilities
    // Return count of new capabilities found
    return Ok(0)
}

fn main() {
    let argv = args()
    if len(argv) < 3 {
        println("usage: kryos run ci-cap-gate.kry -- <src_path> <baseline.json>")
        return
    }
    let src_path = argv[1]
    let baseline_path = argv[2]
    println("kryos-cap-gate: scanning " + src_path)
    // In a real CI step: shell out to `kryos manifest --caps src_path --format json`
    // capture the output, compare against baseline_path
    println("kryos-cap-gate: compare against " + baseline_path)
    println("kryos-cap-gate: PASS (no new capabilities)")
}
```

---

## MVP scope vs full vision

### MVP (smallest shippable slice -- buildable today, ~1 day of work)

1. `kryos manifest --caps <file-or-dir>` subcommand in kryos-cli
2. Output: JSON with schema version, source path(s), and per-function capability list (annotated functions only)
3. `--output <file>` flag to write JSON to disk instead of stdout
4. `--strict` flag: include unannotated functions as `capabilities: [], annotated: false`
5. `--deny <cap>[,<cap>]` flag: exit 1 if any function has any of the listed caps (CI gate mode)
6. Golden-file tests: 5 fixture .kry files with known @capabilities combos, expected JSON outputs
7. Integration in kryos-cli test suite: `kryos manifest --caps tests/manifest/net_only.kry --format json` output matches expected

### Full vision (post-MVP)

- `--diff <baseline.json>`: compare current manifest to a previous one, emit added/removed capabilities per function
- `--badge`: emit a shield-style SVG/JSON for registry display ("net, io" or "no-net, no-ffi")
- Registry integration: `kryos pkg publish` automatically attaches the manifest to the published tarball
- Playground runner reads the manifest from the package before JIT-executing and refuses any `ffi` or `process` cap
- `kryos manifest --watch`: re-emit on file change (for LSP integration)
- IDE integration: inline capability badge on each function in the hover card

---

## Build plan

### Step 0: verify the toolchain compiles (5 min)

```
cd C:\Users\Krist\projects\active\kryos-lang\compiler
cargo build -p kryos-cli 2>&1 | tail -5
```

Verify it compiles clean before touching anything.

### Step 1: add manifest_cmd.rs (core logic)

Create `compiler/crates/kryos-cli/src/commands/manifest_cmd.rs`.

The file should:
- Define `ManifestOptions { path, format, strict, output, deny_caps }`
- Implement `execute(opts: ManifestOptions) -> Result<(), String>`
- Reuse the exact same `collect_kry(dir, out)` helper from `audit_cmd.rs` (or extract it to a shared `utils.rs` if you prefer DRY; either is fine for MVP)
- Implement `scan_file(path, strict) -> Option<FileManifest>` as described above
- Implement `emit_json(manifests, deny_caps) -> Result<(), String>` -- writes to stdout or --output file, returns Err if any deny_cap was found
- The JSON serializer should use `serde_json` if it is already a dependency, otherwise hand-build the JSON string as `audit_cmd.rs` does (audit_cmd.rs hand-builds; check Cargo.toml for serde_json)

Check Cargo.toml for available deps:

```
cat compiler/crates/kryos-cli/Cargo.toml
```

### Step 2: register the command in main.rs

In `compiler/crates/kryos-cli/src/main.rs`:

Add to the `Commands` enum (after `Audit`, before `Bench` alphabetically):

```rust
/// Emit a per-function capability manifest for a Kryos source tree
Manifest {
    /// Source file or project directory. Default: current directory.
    #[arg(value_name = "PATH")]
    path: Option<String>,

    /// Output format: json (default) or pretty.
    #[arg(long, value_name = "FORMAT", default_value = "json")]
    format: String,

    /// Include unannotated functions as capability=[] in output.
    #[arg(long)]
    strict: bool,

    /// Write manifest to FILE instead of stdout.
    #[arg(short, long, value_name = "FILE")]
    output: Option<String>,

    /// Exit 1 if any function has one of these capabilities (comma-separated, e.g. ffi,process).
    #[arg(long, value_delimiter = ',', value_name = "CAP")]
    deny: Vec<String>,
},
```

Add to the `match cli.command { ... }` block:

```rust
Commands::Manifest { path, format, strict, output, deny } => {
    commands::manifest_cmd::execute(commands::manifest_cmd::ManifestOptions {
        path,
        format,
        strict,
        output,
        deny_caps: deny,
    })
}
```

Add `pub mod manifest_cmd;` to `commands/mod.rs`.

### Step 3: write golden-file tests

Create `compiler/crates/kryos-cli/tests/manifest/` with:

- `net_only.kry` -- one fn with `@capabilities(net)`
- `multi_cap.kry` -- fns with `@capabilities(net, io)`, `@capabilities(crypto)`, and one unannotated fn
- `all_cap.kry` -- one fn with `@capabilities(all)`
- `no_caps.kry` -- all fns unannotated
- `extern_block.kry` -- fn with `@capabilities(ffi)` and an extern block

Create `compiler/crates/kryos-cli/tests/manifest/expected/` with the expected JSON for each fixture.

Write a Rust integration test in `compiler/crates/kryos-cli/tests/manifest_tests.rs` that:
1. Calls `manifest_cmd::execute(ManifestOptions { path: Some("tests/manifest/net_only.kry"), format: "json".into(), strict: false, output: None, deny_caps: vec![] })`
2. Captures stdout (use a temp file + --output flag)
3. Compares output to expected JSON (normalize key order with serde_json if available, otherwise compare sorted)

### Step 4: build and run tests

```
cargo build -p kryos-cli
cargo test -p kryos-cli manifest
```

Fix any compile errors (most likely: missing `pub mod manifest_cmd` in mod.rs, or a Cargo.toml dependency missing).

### Step 5: smoke test the CLI

```
cargo run -p kryos-cli -- manifest --caps compiler/stdlib/llm.kry
cargo run -p kryos-cli -- manifest --caps compiler/stdlib/llm.kry --strict
cargo run -p kryos-cli -- manifest --caps compiler/stdlib/ --deny ffi,process
```

The last command should exit non-zero if any stdlib function declares `ffi` or `process`.

### Step 6: write the Kryos CI gate demo

Write `tools/ci-cap-gate.kry` (Kryos source, not Rust). This is the demo artifact -- it shows that the manifest tooling is useful in real Kryos code:

```kryos
// tools/ci-cap-gate.kry
// Reads a kryos manifest JSON and compares to a baseline.
// Usage: kryos run tools/ci-cap-gate.kry -- manifest.json baseline.json

use std::json::{json_parse, json_get, json_to_str, json_keys, json_is_null, json_length, json_get_index}

@capabilities(io)
fn load_manifest(path: str) -> i64 {
    let content = file_read(path)
    return json_parse(content)
}

@capabilities(io)
fn diff_manifests(current_path: str, baseline_path: str) -> i64 {
    let current = load_manifest(current_path)
    let baseline = load_manifest(baseline_path)
    let current_fns = json_get(current, "functions")
    let baseline_fns = json_get(baseline, "functions")
    let mut new_cap_count = 0
    let fn_names = json_keys(current_fns)
    let mut i = 0
    while i < len(fn_names) {
        let fn_name = fn_names[i]
        let cur_entry = json_get(current_fns, fn_name)
        let bas_entry = json_get(baseline_fns, fn_name)
        if json_is_null(bas_entry) {
            // new function -- check if it has caps
            let caps = json_get(cur_entry, "capabilities")
            if json_length(caps) > 0 {
                println("NEW FN WITH CAPS: " + fn_name)
                new_cap_count = new_cap_count + 1
            }
        }
        i = i + 1
    }
    return new_cap_count
}

fn main() {
    let argv = args()
    if len(argv) < 3 {
        println("usage: kryos run tools/ci-cap-gate.kry -- current.json baseline.json")
        return
    }
    let new_caps = diff_manifests(argv[1], argv[2])
    if new_caps > 0 {
        println("FAIL: " + to_string(new_caps) + " new capability-bearing function(s) detected")
        println("Review the diff and update the baseline if intentional.")
    } else {
        println("PASS: no new capabilities")
    }
}
```

Note: `json_keys` may not exist in the current std::json stdlib. Check `compiler/stdlib/json.kry` first. If it is missing, the gate script iterates over a known list of functions instead, or the Rust manifest command emits an array form rather than an object form.

### Step 7: verify against the actual stdlib

```
cargo run -p kryos-cli -- manifest --caps compiler/stdlib/llm.kry --format json
```

Expected: `chat` and `complete` and `chat_within` and `chat_tools` and `continue_with_tool_results` all appear with `["net"]`. Internal helpers like `_openai_body` and `_parse_openai` appear with `[]` (if `--strict`) or are absent.

---

## Success criteria / how to demo it

### Demo 1: basic manifest

```bash
kryos manifest --caps compiler/stdlib/llm.kry --format json
```

Expected output (abbreviated):
```json
{
  "schema": "kryos-manifest-v1",
  "source": "compiler/stdlib/llm.kry",
  "functions": {
    "chat": { "capabilities": ["net"], "annotated": true },
    "chat_tools": { "capabilities": ["net"], "annotated": true },
    "chat_within": { "capabilities": ["net"], "annotated": true },
    "complete": { "capabilities": ["net"], "annotated": true },
    "continue_with_tool_results": { "capabilities": ["net"], "annotated": true }
  },
  "unannotated_count": 17
}
```

### Demo 2: CI gate (deny ffi)

```bash
kryos manifest --caps compiler/stdlib/ --deny ffi,process --format json
echo "exit code: $?"
```

Should exit 1 if any stdlib module has `ffi` or `process` caps.

### Demo 3: strict mode shows all functions

```bash
kryos manifest --caps compiler/stdlib/llm.kry --strict --format json | python -m json.tool | head -30
```

Shows all 22+ functions (helper functions too).

### Demo 4: Kryos CI gate script

```bash
# Generate current manifest
kryos manifest --caps src/ --format json --output current-manifest.json
# Compare to baseline
kryos run tools/ci-cap-gate.kry -- current-manifest.json baseline-manifest.json
echo "exit: $?"
```

### Verification checklist

- [ ] `cargo test -p kryos-cli` passes with at least 5 new manifest golden tests
- [ ] `kryos manifest --caps compiler/stdlib/llm.kry` shows `chat` with `["net"]`
- [ ] `kryos manifest --caps compiler/stdlib/llm.kry --deny net` exits 1
- [ ] `kryos manifest --caps compiler/stdlib/llm.kry --deny io` exits 0
- [ ] `kryos run tools/ci-cap-gate.kry -- current.json baseline.json` runs without crashing
- [ ] Output JSON validates against the schema (correct field names, no extra keys)

---

## Risks and honest unknowns

### Risk 1: json_keys does not exist in std::json

**Probability: high.** The CI gate script uses `json_keys(node) -> [str]` to iterate over object keys. This may not be implemented in `compiler/stdlib/json.kry`. **Mitigation:** Before writing the gate script, read `compiler/stdlib/json.kry` and check. If missing, rewrite the gate script to iterate over a pre-known list of function names, or have the Rust manifest command emit a JSON array instead of an object (simpler to iterate in Kryos).

### Risk 2: `kryos-parser::parse` API signature

The manifest command calls `kryos_parser::parse(tokens)`. Verify the exact signature in `compiler/crates/kryos-parser/src/lib.rs` before writing the Rust code -- it may take additional arguments or return a different error type than `audit_cmd.rs` demonstrates.

### Risk 3: impl methods and actors are not scanned

`build_fn_capability_map()` in checker.rs recurses into `Decl::Impl { methods }` and `Decl::Trait { methods }`. The MVP scan_file in manifest_cmd.rs must do the same -- a flat walk of `module.declarations` misses impl methods. Look at how `audit_cmd.rs` handles this: it only scans top-level `Decl::Function`. If you need impl methods in the manifest, add recursive descent.

### Risk 4: `@capabilities` annotation argument spelling

The attributes doc and the model.rs source list `net`, `io`, `ffi`, `compute`, `crypto`, `process`, `env`, `term`, `db`, `time`, `all`. The attributes.md doc also mentions `network`, `filesystem`, `gpu` as aliases. The `Capability::from_str()` function in model.rs only handles the short forms. If any stdlib or user code uses the long forms, they will be treated as unknown. Flag this in the manifest output under `"warnings"`.

### Risk 5: the `--deny` exit code must be non-zero for CI to catch it

Standard Unix CI tools check the exit code. Rust `std::process::exit(1)` in the execute function is the correct mechanism. Do not return `Err(String)` -- the main.rs already handles `Err` by printing and exiting 1, which is correct.

### Risk 6: overlap with `kryos audit`

`kryos audit` already does a subset of this. The manifest command differs in: (a) it is machine-readable JSON by default, (b) it has a `--deny` flag for CI gating, (c) it excludes the secret-pattern scan and extern inventory that audit includes, (d) it is keyed by function name not by capability name. Make sure the help text is clear about the distinction so the commands do not confuse users.

### Non-risk: no language changes needed

This project requires zero changes to the Kryos type system, parser, or codegen. It is purely additive to the CLI crate. The capability data is already computed; this command just exposes it.
