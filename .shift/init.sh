#!/usr/bin/env bash
# Shift init for kryos-self-compile.
# Idempotent. Prints READY when env is hot.
set -euo pipefail
echo "shift-init: $(date -u +%FT%TZ)"

# Repo sanity
if [ ! -f compiler/Cargo.toml ]; then
  echo "ERROR: not at repo root (compiler/Cargo.toml missing)"
  exit 1
fi

# We rebuild kryos.exe via `cargo build --release -j 2` only on demand,
# not on every resume — full build is ~3-5 min on this host and the
# binary at compiler/target/release/kryos.exe is the source of truth.
if [ -x compiler/target/release/kryos.exe ] || [ -x compiler/target/release/kryos ]; then
  echo "stage-0 binary present"
else
  echo "WARN: no stage-0 binary in compiler/target/release; run cargo build --release -j 2 from compiler/ before resuming work"
fi

# Stage-1 binary (built FROM kryos source via stage-0)
if [ -x compiler/target/bootstrap/kryos-stage1.exe ] || [ -x compiler/target/bootstrap/kryos-stage1 ]; then
  echo "stage-1 binary present"
else
  echo "INFO: no stage-1 binary; build with: kryos build self-host/main.kry -o target/bootstrap/kryos-stage1 --skip-ownership (from compiler/)"
fi

echo READY
