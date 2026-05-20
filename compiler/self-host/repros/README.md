# Self-host Bug Repros

Minimal `.kry` test cases captured during bisection of stage-1 / stage-2
self-host bugs.

## Usage

From `compiler/`:

```bash
KRYOS_SKIP_TYPES=1 ./target/bootstrap/kryos-stage1.exe obj \
    self-host/repros/<filename>.kry -o /tmp/_repro.obj
```

A repro that crashes stage-1 (rc != 0) or produces "Type errors" output
captures a real stage-1 bug.

## Staleness

Many of these repros captured bugs that have since been fixed. If a
repro now compiles cleanly, it's a historical artifact — feel free to
leave it as a regression sentinel.

Last verified (2026-05-20):
- Repros tested in shift kryos-self-compile/21 (`repro_lex_scan_char_min.kry`,
  `repro_nested_while_continue.kry`, `repro_long_elif.kry`) all PASS through
  stage-1. The bugs they captured are fixed.

## Naming convention

- `repro_<area>_<descriptor>.kry` — generic
- `repro_<area>_<descriptor>_v<N>.kry` — bisect iterations
- `repro_<symptom>_<minimization>.kry` — minimized form

## Categories

- `repro_arr_*` — array indexing, layout, struct-in-array
- `repro_lex_*` — lexer string / char / number scanning
- `repro_str_*` — string operations and concat
- `repro_rv_binop_*` — rvalue binop lowering bisections
- `repro_parser_v*` — parser failure minimizations
- `repro_3struct`, `repro_two_arr_field`, etc. — struct layout / copy
