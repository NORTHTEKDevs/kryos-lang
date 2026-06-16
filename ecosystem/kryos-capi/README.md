# kryos-capi

Build a shared library (`.dll` / `.so`) whose **every exported C entry point
ships a compiler-verified statement of the authority it holds.**

A C, Python, or Go host that `dlopen`s a `libfoo.so` today has no
machine-readable way to know whether `foo_parse()` can touch the network or the
filesystem. `cbindgen` emits a header that describes *types* and says nothing
about *authority*. Kryos computes a per-function `CapabilitySet` and verifies it
against what the function actually calls, so kryos-capi can emit a companion
`lib.caps.json` keyed by C symbol:

```json
{
  "library": "csvlib",
  "abi": "c",
  "schema": "kryos-capi-caps-v1",
  "exports": {
    "csv_field_count":  { "capabilities": [],     "verified": true },
    "csv_row_count":    { "capabilities": [],     "verified": true },
    "csv_to_json_file": { "capabilities": ["io"], "verified": true }
  }
}
```

`csv_field_count` is **compute-only** (`[]`); `csv_to_json_file` calls
`file_write`, so the compiler *forces* it to declare `io`. A host can read this
**before it binds a single symbol** and refuse anything that exceeds its
allow-set.

## Why "verified" is not just a label

Kryos rejects a function whose declared capabilities understate what it does:

```kryos
@capabilities()                       // claims compute-only ...
fn mislabeled_export(ptr: i64) -> i64 {
    file_write("x.txt", "data")       // ... but calls io
    return ptr
}
// error[E0505]: builtin `file_write` requires `io` capability
```

Because the library would not compile otherwise, a clean build is proof the
`caps.json` is honest. `fixtures/mislabel_compute.kry` is a negative fixture and
`build/build.sh` asserts the E0505 rejection.

## Layout

```
src/csvlib.kry      the demo library -> compiled to dist/csvlib.dll
                    exports: csv_field_count [compute], csv_row_count [compute],
                             csv_to_json_file [io]
src/capset.kry      runtime capability-set algebra (subset / excess / render)
src/manifest.kry    reads a <lib>.caps.json -> declared caps for a C symbol
gen_caps.kry        build wrapper: raw `kryos manifest --caps` -> curated caps.json
consumer.kry        the capability-aware host: read caps.json, refuse or bind+call
fixtures/           negative compile-fail fixture (kept out of tests/)
build/build.sh      end-to-end build wrapper + done-criteria assertions
tests/test_capi.kry pure-logic tests (policy + manifest), run by `kryos test`
```

## Build and run

```bash
# Build the DLL + caps.json and run the full end-to-end checks:
bash ecosystem/kryos-capi/build/build.sh

# Pure-logic tests (no DLL needed):
kryos test --path ecosystem/kryos-capi
```

The consumer, run under two different allow-sets:

```bash
# compute-only host: the io export is REFUSED, the compute exports run
kryos run consumer.kry dist/csvlib.caps.json dist/csvlib.dll compute
#   ALLOW   csv_field_count  requires={}      -> csv_field_count = 3
#   ALLOW   csv_row_count    requires={}      -> csv_row_count = 2
#   REFUSE  csv_to_json_file requires={io}  excess={io}

# {compute, io} host: all three run
kryos run consumer.kry dist/csvlib.caps.json dist/csvlib.dll compute,io
#   ALLOW   csv_to_json_file requires={io}   -> wrote 80 bytes to dist/csvlib_out.json
```

## How the shared library is produced

`kryos build` does not expose a `--shared` flag. The compiler driver and linker
*do* support shared-lib output (`OutputType::Library` -> `LinkType::SharedLib`),
but there is no CLI surface for it, so the wrapper builds the DLL on the demo's
terms:

1. `kryos build --release --emit-llvm` to get LLVM IR.
2. Promote the exported symbols from `internal` to `dllexport` (Kryos does not
   name-mangle, so the C symbol is exactly the function name).
3. Assemble + link with `zig cc` (a clang frontend that locates the MSVC SDK),
   linking the same `kryos_rt` / `kryos_stdlib_native` libraries the normal
   kryos linker uses. On Linux this is the same recipe targeting a `.so`.

The host loads it with `std::ffi` (`open` / `sym` / `call1` / `call2` / `cstr`).

## Honest scope and limitations

- **FFI is i64-marshalled** (`std::ffi`: max arity 6, ints/pointers). Exports
  take C string pointers and return ints; strings are rebuilt inside the export
  with `ffi.from_ptr`. No struct-by-value or float-heavy signatures.
- The capability checker is **non-transitive**: a function is labelled by the
  gated builtins it calls *directly*. Calling another Kryos function does not
  inherit that function's capabilities. Exports here call their gated builtins
  directly, so the labels are exact.
- The JSON the `io` export writes is built by **plain string concatenation**,
  not `std::json`'s value tree: the LLVM/AOT backend (which the DLL uses) loses
  heap payloads when `json_object`/`json_string` results are stored in an array
  and stringified later. Hand-built `str` JSON round-trips correctly on both
  backends. The CSV reader assumes simple, unquoted comma-separated fields.
- Building the DLL requires `zig` on `PATH` (used purely as the IR assembler +
  linker driver) until `kryos build` grows a `--shared` flag.

## License

Apache-2.0.
