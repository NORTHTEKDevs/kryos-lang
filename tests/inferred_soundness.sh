#!/usr/bin/env bash
# inferred_soundness.sh -- regression gate for `--capabilities-mode=inferred`
# (deny-by-default with interior inference). Asserts that every path authority
# can take past an unannotated boundary is REJECTED, and that safe code is
# accepted. These probes encode the leaks found and fixed by three adversarial
# review passes; if a refactor reopens one, this gate turns red.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
taskkill //F //IM kryostokens.exe >/dev/null 2>&1 || true

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# want_reject <name> <source> : the program uses authority from an unannotated
# boundary and MUST be rejected under inferred mode.
want_reject() {
  local name="$1" src="$2" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  if timeout 30 "$KRYOS" check --capabilities-mode=inferred "$f" >/dev/null 2>&1; then
    echo "  LEAK  $name -- passed inferred check but exercises undeclared authority"
    fail=$((fail+1))
  fi
}

# want_pass <name> <source> : safe / correctly-declared code MUST be accepted.
want_pass() {
  local name="$1" src="$2" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  if ! timeout 30 "$KRYOS" check --capabilities-mode=inferred "$f" >/dev/null 2>&1; then
    echo "  FALSE-POSITIVE  $name -- rejected safe/declared code"
    fail=$((fail+1))
  fi
}

# --- authority must be caught at the boundary through every path ---
want_reject direct_builtin \
'fn main() { file_write("/tmp/z","a") }'

want_reject transitive_helper \
'fn helper() { file_write("/tmp/z","a") }
fn main() { helper() }'

want_reject method_dispatch \
'struct S { t: i64 }
impl S { fn w(self: S) { file_write("/tmp/z","a") } }
fn main() { let s: S = S { t: 1 }  s.w() }'

want_reject builtin_as_arg \
'fn ap(f: fn(str,str)->i64, a: str, b: str) -> i64 { return f(a,b) }
fn main() { ap(file_write, "/tmp/z", "a") }'

want_reject builtin_let_bound \
'fn main() { let f = file_write }'

want_reject builtin_in_array \
'fn main() { let a = [env_get] }'

want_reject env_get_direct \
'fn main() { env_get("PATH") }'

# --- stdlib wrappers that reach authority via raw externs (annotated) ---
want_reject stdlib_env_get_or \
'use std::process::{env_get_or}
fn main() { let p = env_get_or("PATH","x") }'

want_reject stdlib_env_has \
'use std::process::{env_has}
fn main() { let h = env_has("PATH") }'

want_reject stdlib_create_dir_all \
'use std::fs::{create_dir_all}
fn main() { create_dir_all("/tmp/kx/y") }'

want_reject stdlib_os_home_dir \
'use std::os::{home_dir}
fn main() { let h = home_dir() }'

# --- safe / correctly-declared code must be accepted ---
want_pass pure_helper \
'fn add(a: i64, b: i64) -> i64 { return a + b }
fn main() { println(to_string(add(1,2))) }'

want_pass declared_main \
'@capabilities(fs:write)
fn main() { file_write("/tmp/z","a") }'

want_pass declared_transitive \
'fn helper() { file_write("/tmp/z","a") }
@capabilities(fs:write)
fn main() { helper() }'

want_pass user_method_named_like_builtin \
'struct Doc { n: i64 }
impl Doc { fn write_file(self: Doc) { println("noop") } }
fn main() { let d: Doc = Doc { n: 1 }  d.write_file() }'

want_pass ambient_builtins \
'fn main() { exit(0) }'

if [ "$fail" -eq 0 ]; then
  echo "inferred-soundness: all probes correct (leaks rejected, safe code accepted)"
else
  echo "inferred-soundness: $fail probe(s) WRONG"
fi
[ "$fail" -eq 0 ]
