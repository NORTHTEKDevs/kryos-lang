# kryos-schema

Capability-pure data-validation combinators for Kryos.

Build a schema with combinators, validate an untrusted `std::json` value against
it, and get back **every** violation at once. The whole validation path is
annotated `@capabilities(compute)`, so `kryos manifest --caps` is a
machine-checkable proof that **validating an input cannot perform I/O** -- no
network call, no file read, no environment access hidden inside a "validate"
step.

## The Kryos-shaped property

Validation libraries (zod, pydantic, joi) are everywhere, and they share a
quiet hazard: a "validator" is ordinary code, free to open a socket or read a
file while it inspects your data. That is a real supply-chain concern -- a
compromised validator in a dependency tree can exfiltrate the very input it is
trusted to check.

Kryos makes the absence of that power *provable*. Every function's capability
set is inferred and surfaced by the compiler. A validator built as
`@capabilities(compute)` is statically guaranteed to be pure computation. That
makes Kryos validators safe to run on untrusted input inside a plugin sandbox or
at an MCP tool boundary, where "validation must not have side effects" is a
security requirement, not a style note.

```
== capability manifest (src/) ==
fn validate:       [compute]
fn collect_errors: [compute]
fn matches:        [compute]
... (every function) ...
unannotated: 0

== deny: net,io,ffi,crypto,process,env,term,db,time ==
PASS: every function in src/ is compute-only (no I/O capabilities).
```

Run that yourself: `./check_caps.sh` (the manifest test).

## Quick start

```kryos
use std::result::{Result, Ok, Err}
use std::json::{JsonValue, json_object, json_string, json_number}
use schema
use validate

// name: 2..40 chars, age: 0..130, role: one of a fixed set, tags: optional
let user = object(
    ["name", "age", "role", "tags"],
    [
        with_max(with_min(str_(), 2), 40),
        with_max(with_min(int_(), 0), 130),
        enum_of(str_(), ["admin", "user", "guest"]),
        optional(array(str_())),
    ]
)

let input = json_object(
    ["name", "age", "role"],
    [json_string("x"), json_number(200.0), json_string("wizard")]
)

match validate(user, input) {
    Ok(_)   => println("valid"),
    Err(es) => {
        // ALL violations, not just the first:
        //   name: string length 1 is below minimum 2
        //   age: value 200 exceeds maximum 130
        //   role: value "wizard" is not one of [admin, user, guest]
        for e in es { println(e) }
    }
}
```

## API

### Type combinators (in `schema`)

| Combinator              | Builds                                                |
| ----------------------- | ----------------------------------------------------- |
| `str_()`                | a JSON string                                         |
| `int_()`                | a JSON integer (a whole number; `3.5` is rejected)    |
| `bool_()`               | a JSON boolean                                         |
| `object(names, schemas)`| an object; parallel arrays of field names and schemas |
| `array(of)`             | an array whose every element satisfies `of`           |
| `optional(inner)`       | accepts null / absence; otherwise validates `inner`   |

The primitive combinators carry a trailing underscore (`str_`, `int_`, `bool_`)
because `str`, `int`, and `bool` collide with Kryos's built-in cast functions.

### Constraint refiners (in `schema`)

| Refiner                  | Effect                                                  |
| ------------------------ | ------------------------------------------------------- |
| `with_min(s, n)`         | inclusive minimum: string length, or integer value      |
| `with_max(s, n)`         | inclusive maximum: string length, or integer value      |
| `with_pattern(s, regex)` | the string must match `regex` (see "Regex" below)       |
| `enum_of(s, [str])`      | the string must be one of the listed values             |
| `int_enum_of(s, [i64])`  | the integer must be one of the listed values            |

Refiners return a refined **copy** -- `Schema` is `@copy`, so reusing a base
schema is safe: `let id = int_(); let a = with_max(id, 10); let b = with_max(id, 99)`.

### Validation (in `validate`)

```kryos
fn validate(schema: Schema, value: JsonValue) -> Result<JsonValue, [str]>
```

Returns `Ok(value)` when valid, or `Err([message, ...])` listing **every**
violation. `collect_errors(schema, value, path)` is also exported if you want the
raw error list (with a path prefix) without the `Result` wrapper.

Error messages carry a JSON-path-style location: `name`, `tags[0]`,
`addr.city`, etc.

## Regex

The `with_pattern` constraint is backed by `rematch.kry`, a **pure-Kryos**
regex-subset matcher -- deliberately *not* `std::re`. `std::re` is FFI-backed
(it links a native regex library), so a validator that called it would not
actually be capability-pure: its `compute` manifest would be hiding an FFI
dependency behind Kryos's non-transitive capability checker. A from-scratch
matcher keeps the guarantee honest -- the entire validation path is genuinely
I/O- *and* FFI-free.

Supported syntax (unanchored like `std::re::is_match`; add `^...$` for a full
match):

```
literals          a b c
.                 any character
^  $              start / end anchors
[abc] [a-z] [^..] classes, ranges, negation
*  +  ?           greedy quantifiers
\d \w \s          digit / word / whitespace (\D \W \S negate)
\. \[ \\          backslash escapes a metacharacter to a literal
```

`demo_mcp.kry` shows `std::re` interop: the same email pattern decides the same
way under the FFI engine, so you can inject `std::re` where you need full PCRE
while the default path stays compute-only.

## Layout

```
src/schema.kry    combinators and constraint refiners (the Schema record)
src/rematch.kry   pure regex-subset matcher behind with_pattern
src/validate.kry  validate / collect_errors -- the error-aggregating engine
tests/            test_schema, test_rematch, test_validate
demo_mcp.kry      end-to-end: validate an MCP tool's input schema
check_caps.sh     the manifest test (compute-only proof)
```

## Build & test

```bash
kryos test --path .              # run the suite (Cranelift JIT)
kryos run demo_mcp.kry           # the end-to-end MCP demo
./check_caps.sh                  # prove the surface is compute-only
```

## Scope and limitations

- **In scope (MVP):** the combinators above, min/max, regex, enum membership,
  and `validate` collecting all errors.
- **Out of scope:** JSON-Schema import/export, custom async refinements, type
  coercion. Unknown object keys are allowed (not rejected) -- only declared
  fields are checked.
- **Integers:** JSON has one number type; an `int_()` schema accepts whole
  numbers and rejects fractional ones. `int_` range checks compare against the
  `i64` value.
- **String length** is measured in bytes (`len`), not Unicode scalar values.
- **Backend:** the supported path is Cranelift -- `kryos test` and `kryos run`.
  LLVM AOT (`kryos build --release`) currently fails to codegen programs that
  pass a `JsonValue` through the recursive validator: the backend lowers the
  3-word `JsonValue` enum into an `i64` slot and clang rejects the mismatch
  (`'{ i64, i64, i64 }' but expected 'i64'`). That is a compiler-backend issue,
  not a package bug -- the same code is correct under the JIT. (The schema
  combinators and the pure matcher do build and run correctly under AOT on their
  own.)
