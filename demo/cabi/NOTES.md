# C ABI Demo Notes

## What runs

The governed agent from `demo/cabi/agent_lib.kry` compiled to a Windows DLL
and called from `demo/cabi/harness.c` (a plain C program) via
`LoadLibrary` / `GetProcAddress`. The DLL is built in four steps; no
`--shared` CLI flag exists yet so the IR path is used.

## Link recipe (check.sh reproduces this automatically)

```
# 1. Emit LLVM IR
kryos build --release --emit-llvm demo/cabi/agent_lib.kry -o agent_lib.ll

# 2. Patch IR: Kryos emits all functions as `define internal`.
#    Change the export's linkage so the Windows linker exposes it.
sed 's/define internal i64 @agent_query_c/define dllexport i64 @agent_query_c/' \
    agent_lib.ll > agent_lib_exp.ll

# 3. Link DLL (zig cc acts as a clang frontend; finds MSVC SDK automatically)
zig cc -target x86_64-windows-msvc -shared -o agent_lib.dll agent_lib_exp.ll \
    compiler/target/release/kryos_rt.lib \
    compiler/target/release/kryos_stdlib_native.lib \
    -lntdll -luserenv -lws2_32 -ldbghelp -luser32 -lbcrypt -ladvapi32 \
    -Wno-override-module

# 4. Build C harness
zig cc -target x86_64-windows-msvc -o harness.exe demo/cabi/harness.c

# 5. Run
harness.exe agent_lib.dll
```

## C ABI surface

`agent_query_c(max_calls: i64, calls_spent: i64) -> i64`

Kryos `str` fat-pointers are NOT portable across the boundary; all governance
semantics are communicated via i64 return codes and labelled `println` lines
on stdout that the C host (or check.sh) greps:

| Output line prefix | Meaning |
|---|---|
| `WITHIN_BUDGET_ANSWER:` | Answer text (includes source, tokens, call count) |
| `WITHIN_BUDGET_SOURCE:` | Provenance string (`mock-llm-v1`) |
| `OVER_BUDGET_REFUSED:` | Refusal message |
| `OVER_BUDGET_SPEND:` | Spend counter at refusal time (always `0`) |

Return value: `1` = answered, `0` = refused.

## Governance properties verified

- (a) Within-budget call (`max_calls=3, calls_spent=0`): returns 1, answer
  contains `source=mock-llm-v1`, C host sees `result=1`.
- (b) Over-budget call (`max_calls=0, calls_spent=0`): refused BEFORE the mock
  LLM is called, `OVER_BUDGET_SPEND: 0` confirms no spend was recorded,
  C host sees `result=0`.
- @capabilities(net:http) annotation present on `agent_query_c` and its
  callees — propagates compile-time at the Kryos level; the C host is outside
  the capability type system by definition.

## Known limitations

- No `--shared` / `--emit-obj` flag on the Kryos CLI yet. The IR-patch route
  works but is a two-step workaround; a future `kryos build --shared` flag
  will replace it.
- Kryos `str` (fat pointer: `{ptr, len}`) cannot be passed across the C ABI
  safely. The workaround is i64 return codes + labelled stdout lines.
- Windows only (uses `LoadLibrary`/`GetProcAddress`). Linux/macOS would use
  `dlopen`/`dlsym` with the same IR-patch recipe (`dso_local` instead of
  `dllexport`).
