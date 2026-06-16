# kryos-fmt

Capability-zero config and data parsers for Kryos: a **TOML** reader/writer, a
**minimal YAML** reader, and a **CSV** reader/writer, all bridging through one
typed `Value` model to and from `std::json::JsonValue`.

The distinctive part is not that Kryos can parse TOML. Every language can. The
point is that these parsers are pure `compute`: every public function is
`@capabilities(compute)`, so `kryos manifest --caps` over the source emits
`compute` (or empty) and nothing else. A config library that *structurally
cannot* read a file or open a socket is a guarantee no npm or pip config package
can offer -- it is enforced by the compiler, not promised in a doc that rots.

```
$ kryos manifest --caps --deny net,io,ffi,crypto,process,env,term,db,time ecosystem/kryos-fmt/src
$ echo $?
0          # no function in the library carries any capability beyond compute
```

## The `Value` model (the bridge)

`src/value.kry` defines the typed model every parser produces and consumes:

```kryos
enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(str),
    Arr([Value]),
    Table([str], [Value])    // keys, values -- insertion-ordered
}
```

It is deliberately one notch richer than JSON: `Int` and `Float` are distinct
(TOML and YAML separate them; the round-trip writers need the distinction),
whereas JSON collapses both to a single `Number`. The bridge maps both onto
`JsonValue.Number` and reclassifies a `Number` back to `Int` when it is integral:

```kryos
to_json(v: Value)   -> JsonValue      // Int/Float -> Number; else 1:1
from_json(j: JsonValue) -> Value      // integral Number -> Int, else Float
to_json_string(v: Value) -> str       // compact JSON text
from_json_string(s: str) -> Value     // parse JSON text into a Value
```

So any of TOML/YAML/CSV reaches `std::json::JsonValue` for free.

## TOML (`src/toml.kry`)

`parse_toml(input) -> Value` and `to_toml(value) -> str`. Supported subset (what
a `kryos.toml` needs):

- `[table]` and `[dotted.table]` headers, arbitrarily nested
- `key = value` lines with bare keys
- basic `"double"` and literal `'single'` quoted strings
- integers, floats, booleans
- single-line arrays of the above (`[a, b, c]`)
- `#` line comments and blank lines (dropped on read; the writer regenerates
  canonical blank lines between sections)

Rejected with a clear error: inline tables (`{ ... }`), arrays of tables
(`[[x]]`), and multi-line arrays.

**Byte-stable round-trip.** The writer emits a canonical LF form -- top-level
scalars first, one blank line before each `[section]`, super-tables with no
direct keys elided in favor of dotted child headers. Parsing a canonical
document and re-emitting it reproduces the input byte for byte:

```kryos
let doc = "[package]\nname = \"kryos-fmt\"\nversion = \"0.1.0\"\n\n[capabilities]\nallowed = [\"compute\"]\n"
to_toml(parse_toml(doc)) == doc        // true
```

(The canonical form uses `\n`. The `kryos.toml` checked into this repo uses CRLF
because of the Windows checkout; round-trip stability is defined over the LF
canonical form, which is what the writer produces.)

## YAML (`src/yaml.kry`) -- reader only, hard subset

YAML's full grammar is a tar pit, so the subset is drawn **hard** and anything
outside it is rejected with a clear error rather than half-supported.

**Supported**

- block mappings: `key: value`, nested by 2-space-deeper indentation
- block sequences: `- item` (scalar, mapping, or nested-block items)
- sequences of mappings: `- key: value` with aligned continuation keys
- scalars: bare, `"double"`/`'single'` quoted, integers, floats, `true`/`false`,
  null (`null` or `~`)
- `#` comments (line and inline) and blank lines
- a single leading `---` / trailing `...` document marker (ignored)

**Rejected (clear error, never half-supported)**

- anchors `&`, aliases `*`, tags `!`
- block scalars `|` and `>`
- flow collections `[ ... ]` and `{ ... }`
- merge keys `<<`
- tab characters in indentation

There is no YAML *writer*: round-tripping YAML losslessly is out of the subset.
To serialize, bridge to a `Value` and emit TOML or JSON instead.

```kryos
parse_yaml(input) -> Value
```

## CSV (`src/csv.kry`)

RFC-4180-style, following `std::csv` (quoted fields, embedded commas and
newlines, doubled-quote `""` escapes). It is re-implemented here rather than
imported: `std::csv` and `std::json` both export a top-level `parse`, and Kryos
resolves function names in a single flat namespace, so importing both into a
project that already uses `std::json` (via the `Value` bridge) is a
duplicate-symbol error.

```kryos
parse_csv(input) -> [CsvRow]           // rows of raw string fields
to_csv(rows: [CsvRow]) -> str
to_csv_line(fields: [str]) -> str
parse_records(input) -> Value          // header row -> Array of Tables
records_to_csv(value) -> str           // Array of Tables -> CSV text
```

`parse_records` reads the first row as a header and yields an `Arr` of `Table`
Values (one record each, keyed by the header), so a CSV reaches JSON through the
same bridge.

## Layout

```
kryos.toml            package manifest, [capabilities] allowed = ["compute"]
src/value.kry         the typed Value model + JsonValue bridge
src/toml.kry          parse_toml / to_toml (byte-stable round-trip)
src/yaml.kry          parse_yaml (the hard subset reader)
src/csv.kry           parse_csv / to_csv + the records<->Value bridge
src/strutil.kry       shared pure-compute string + number helpers
demo.kry              all three parsers + the JSON bridge, in one file
tests/test_value.kry  Value model + JsonValue bridge (5 @test)
tests/test_toml.kry   TOML parse/emit + byte-stable round-trip (8 @test)
tests/test_yaml.kry   YAML subset + rejection (8 @test)
tests/test_csv.kry    CSV quoting + records bridge (6 @test)
```

## Running

From the repo root, with the in-repo toolchain:

```
kryos test --path ecosystem/kryos-fmt
kryos run  ecosystem/kryos-fmt/demo.kry
kryos manifest --caps --format pretty ecosystem/kryos-fmt/src
kryos manifest --caps --deny net,io,ffi,crypto,process,env,term,db,time ecosystem/kryos-fmt   # exit 0 == compute-only
```

## Notes and honest limitations

- **YAML is read-only and intentionally small.** The subset above is the whole
  contract; anything else errors rather than silently misparsing.
- **Float rendering is approximate.** Floats are emitted to up to 9 fractional
  digits with trailing zeros trimmed. This is a deliberate workaround: the
  `kryos test` harness JIT declares the polymorphic `to_string` with a fixed
  `i64` signature, so `to_string(<f64>)` does not compile under `kryos test`. The
  emit paths therefore render floats with `strutil._su_float_text`, which uses
  only integer arithmetic and `to_string(i64)`. Config files rarely carry
  high-precision floats; integers, strings, and booleans are exact.
- **The library never touches IO.** Reading a file and handing the bytes to
  `parse_toml` is the caller's job (and the caller's `io` capability). The
  parsers stay in `compute` so they cannot widen a caller's capability surface.
- Licensed Apache-2.0 (see `LICENSE`).
