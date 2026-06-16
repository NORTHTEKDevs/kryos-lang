# kryos-fixture-builder

Hermetic, capability-pure test data + golden-file harness for Kryos.

The comparison and assertion core is `@capabilities(compute)` — a
compiler-checked guarantee that no I/O occurs during the assertion phase.
The only surface that touches files is the thin `@capabilities(io)` layer that
reads and updates `testdata/<name>.golden`. A test body that accidentally calls
a network function fails to compile under `--strict-capabilities`, making
hermeticity a compiler property rather than reviewer discipline.

## API

### Golden-file assertions

```kryos
use golden

let store = golden_store("testdata")           // root all reads/writes here
let jailed = golden_store_jailed("testdata")   // also enforce path safety via kryos-fs-jail

// Read and assert (throws on mismatch; KRYOS_UPDATE_GOLDENS=1 rewrites instead)
let r = assert_golden(store, "my_snapshot", actual_output)

// Lower-level access
let r = golden_compare(expected, actual)       // pure -- no file access
match golden_read(store, "name") { ... }       // io -- file read
match golden_update(store, "name", text) { ... } // io -- file write
```

`golden_compare` returns a `GoldenResult { matched: bool, diff: str }`. The diff
shows `- expected_line` / `+ actual_line` for each differing position.

### Fixture data builder

```kryos
use fixture

let f = fixture_set(
    fixture_tag(fixture_new("alice"), "admin"),
    "email", "alice@example.com"
)

fixture_get(f, "email")      // "alice@example.com"
fixture_has_tag(f, "admin")  // true
fixture_to_str(f)            // deterministic, human-readable for assert_golden
```

All builder functions are `@capabilities(compute)`: fixture construction is
provably I/O-free. A fixture serialized via `fixture_to_str` is stable across
runs and can be fed directly to `assert_golden`.

## Update mode

Set `KRYOS_UPDATE_GOLDENS=1` before running tests to rewrite golden files
instead of asserting:

```
KRYOS_UPDATE_GOLDENS=1 kryos run tests/test_fixture_builder.kry
```

## Compile-fail fixture

`fixtures/net_in_pure_test/` demonstrates that a test body calling a network
function is rejected under `--strict-capabilities`:

```
kryos check --strict-capabilities fixtures/net_in_pure_test/src/pure_test.kry
```

Expected output: `E0505: builtin 'http_get' requires 'net' capability` and
`E0507: call requires [net] not granted to caller`. The file compiles normally
(capability inferred); only strict mode rejects it.

## Running tests

```bash
kryos test --path ecosystem/kryos-fixture-builder   # 4 file tests, 17 @test functions
kryos run tests/test_fixture_builder.kry            # also runs IO round-trip
kryos run demo_fixture.kry                          # full demo
```

## Depends on

- [27 kryos-fs-jail](../kryos-fs-jail) — optional jailed-store path validation
