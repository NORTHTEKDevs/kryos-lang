# Stdlib smoke tests

Minimal end-to-end programs that exercise each unblocked stdlib module
through the **Cranelift (debug) backend** used by `kryos run`. Added in
2.4.1 to prove the modules actually compile and execute, not just that
they type-check.

## Coverage

| File              | Module / area                               |
| ----------------- | ------------------------------------------- |
| `hello.kry`       | Baseline: parse, compile, run.              |
| `test_io.kry`     | `std::io::{print, println, eprintln}`       |
| `test_fs.kry`     | `std::fs` write_file, read_file, size, ...  |
| `test_os.kry`     | `std::os` name, arch, is_linux, dirs        |
| `test_crypto.kry` | `std::crypto::sha256`, random helpers       |
| `test_re.kry`     | `std::re::is_match`, `is_email`             |
| `test_process.kry`| `std::process::command(...).run()`          |
| `test_term.kry`   | `std::term::width`/`height` via raw FFI     |
| `test_net.kry`    | TCP bind/close via raw FFI                  |
| `test_net2.kry`   | `std::net::bind` wrapper                    |
| `test_db.kry`     | `std::db` open / execute / close            |
| `test_db2.kry`    | `std::db` SELECT roundtrip                  |
| `test_tracked.kry`| `std::tracked` lineage                      |
| `test_ffi.kry`    | FFI primitives: `alloc`, `ptr_set_byte`, `ptr_byte_at`, `buf_to_str`, `free_bytes` |
| `test_ffi2.kry`   | FFI primitives: `ptr_read_i64`, `ptr_write_i64` |

## Running

```sh
source $HOME/.cargo/env
cd compiler
cargo build --release -p kryos-cli
for f in ../tests/smoke/*.kry; do
    ./target/release/kryos run "$f" || echo "FAIL: $f"
done
```

All 15 programs should exit 0 and print a `*_ok` marker as the last
non-empty line.

## Known caveats

- `test_process.kry` checks that the process spawned and returned exit
  code 0; it does not yet check `stdout` content because
  `std::process::Command::run()` doesn't forward arguments to
  `kryos_process_exec` yet (see 2.4.1 CHANGELOG known issues).
- `test_term.kry` uses raw FFI (`kryos_term_width`) instead of
  `std::term::width` because the latter `throw`s on non-tty stdin in
  CI sandboxes.
