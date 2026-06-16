# kryos-sandbox-runner

**Project 26** - Pre-execution capability sandbox: a `kryos-run` wrapper that refuses to execute a `.kry` file if its static capability surface exceeds the declared policy.

## What it does

Before running a file with `kryos run`, the sandbox runner calls:

```
kryos manifest --caps --strict --deny <non-allowed-caps> <file.kry>
```

If any annotated function declares a capability outside the allow set, execution is refused with exit code 2 and a clear message. If the manifest is clean, the runner passes the file to `kryos run` and transparently forwards its exit code.

## Usage

```
kryos-sandbox-runner [--allow <policy>] <file.kry>
```

**Policy presets:**

| Preset | Allowed capabilities |
|--------|----------------------|
| `pure` (default) | `compute` only |
| `net-readonly` | `compute`, `net`, `time` |
| `all` | all capabilities (no enforcement) |
| `compute,net,...` | comma-separated custom allow-set |

**Exit codes:**
- `0` - program ran and exited successfully
- `1` - usage error or compilation failure
- `2` - policy refusal
- `N` - program exit code (passed through)

## Build and run

```bash
# Run directly with the JIT
kryos run ecosystem/kryos-sandbox-runner/src/runner.kry --allow pure my_script.kry

# Build a standalone binary
kryos build --release ecosystem/kryos-sandbox-runner/src/runner.kry -o kryos-sandbox-runner
./kryos-sandbox-runner --allow pure my_script.kry
```

## Tests

```bash
# Unit tests (pure compute, no process spawning) -- kryos test compatible
kryos test --path ecosystem/kryos-sandbox-runner/tests

# Integration tests (invokes kryos manifest on fixtures -- uses process spawning)
# Must be run via kryos run, not kryos test, because kryos test JIT does not
# expose the ptr_read_i64 builtin required by std::process Command.run().
kryos run ecosystem/kryos-sandbox-runner/integration/test_manifest.kry
```

**Note on `kryos test`:** pointing `kryos test --path` at the project root
(`ecosystem/kryos-sandbox-runner`) will crash because `demo_sandbox.kry` and
`src/runner.kry` import `std::process`, which references the `ptr_read_i64`
JIT intrinsic that the test runner does not expose.  Use `--path tests` for
the clean, all-green run.

## Demo

```bash
kryos run ecosystem/kryos-sandbox-runner/demo_sandbox.kry
```

## Limitation

The sandbox is only as strong as the `@capabilities(...)` annotations in the target file. Unannotated functions that call IO builtins are not caught by the static manifest. Annotate source files or compile with `--strict-capabilities` to enforce coverage.

## Project structure

```
ecosystem/kryos-sandbox-runner/
  kryos.toml
  src/
    runner.kry        -- CLI entry point (@capabilities(process, env))
    policy.kry        -- pure policy logic: presets, deny computation
  tests/              -- kryos test --path tests/ runs these (all pure compute)
    test_policy.kry   -- 15 unit tests for policy module
    fixtures/
      io_program.kry  -- @capabilities(io) fixture, refused under pure
      pure_program.kry -- @capabilities() fixture, allowed under pure
  integration/        -- run via: kryos run integration/test_manifest.kry
    test_manifest.kry -- 6 integration tests using kryos manifest
  demo_sandbox.kry    -- interactive demo of the full enforcement flow
```

Composes with: `kryos-plugin-sandbox` (WASM host), `kryos-policy` (runtime policy engine).
