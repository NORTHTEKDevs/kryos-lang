# Stage-1 Self-Hosted Compiler on Windows

The self-hosted Kryos compiler (Stage 1) now produces working
Windows `.exe` binaries for a non-trivial subset of the language.

## Build

```cmd
:: Build stage-1 itself with the Rust compiler (one-time).
cd compiler
cargo build --release -j 2
target\release\kryos.exe build self-host\main.kry ^
    -o target\bootstrap\kryos-stage1.exe --skip-ownership

:: Build the C runtime stub (one-time).
self-host\build_runtime.bat
```

## Compile a Kryos program

```cmd
self-host\kryos-build.bat hello.kry hello.exe
hello.exe
```

`kryos-build.bat`:
1. Runs `kryos-stage1 obj hello.kry -o hello.obj` to produce a COFF
   object file.
2. Invokes `link.exe` (MSVC) to link `hello.obj` with
   `self-host\kryos_runtime.lib` (the C shim providing
   `kryos_println_str`, etc.) and `kernel32.lib`.

## Working language features

- Functions with parameters and recursion
- `let` and `let mut`, mutation in loops
- `i64` arithmetic (`+ - * / %`) with proper signedness
- Comparison operators (`< <= > >= == !=`)
- Boolean logic (`and or not`)
- `if` / `elif` / `else`, including chained `if-return` cascades
- `while` loops with mutation
- String literals + `+` concatenation
- `to_string(i64)` for printing numbers
- `println` / `print` / `eprintln` / `eprint`
- `exit(code)` returning a real Windows exit code
- Arrays: `let a: [i64] = []`, `push(a, x)`, `a[i]`, `len(a)`

## Verified programs

| Program            | Behaviour                                            |
| ------------------ | ---------------------------------------------------- |
| `stage1_hello.kry` | prints "hello from stage1"                           |
| `t_lit.kry`        | `let x = 42; println(\"ok\"); exit(x)` -> exit 42    |
| `t_dbl.kry`        | `double(21)` -> 42                                   |
| `t_loop.kry`       | while-loop with 3 iterations + println               |
| `t_full.kry`       | `sum += double(i)` for i in 0..5 -> 20               |
| `t_str.kry`        | `"The number is: " + to_string(1729)` -> correct     |
| `t_if.kry`         | if-cascade returning string literals                 |
| `t_fib4.kry`       | `fib(10)` -> 55                                      |
| `t_fib.kry`        | print fib(0..9) -> 0 1 1 2 3 5 8 13 21 34            |
| `demo_calc.kry`    | basic arithmetic over function calls                 |
| `demo_fizz.kry`    | FizzBuzz(15)                                         |
| `t_arr.kry`        | array push x3 + sum loop -> 60                       |
| `t_many_if.kry`    | 21 sequential `if (n == k) return v`                 |

## Not yet working

- Optimizer passes — segfault on functions with many basic blocks.
  Currently disabled in the `obj` path.
- Self-hosted bootstrap (Stage 2) — `ast.kry`, `x86.kry`, and
  larger self-host files still segfault during the lower pass.
  Likely related to a different MIR construct (struct literal with
  many fields, or nested match arms).
- Maps, traits, generics, async, channels — not exercised yet.
- Type checker doesn't expose error position cleanly; some files
  fail with "undefined variable: \<error\>" placeholder text.

## Architecture notes

- Calling convention is Win64 (RCX/RDX/R8/R9 + 32-byte shadow space).
- Parameters are always spilled to `[rbp - cs_count*8 - 8 - i*8]`
  on function entry. Slower than register-resident params but
  avoids the cross-call clobber problem.
- Mutable locals have lifetime extended to function end (workaround
  for the back-edge problem in linear-scan regalloc).
- Allocatable register order prefers callee-saved (RBX, R12-R15) so
  values survive any call by default.
- COFF emitter writes a synthetic `__text_base` / `__data_base` /
  `__rodata_base` static symbol per object so codegen's PC-relative
  LEA loads can be resolved by the system linker.
