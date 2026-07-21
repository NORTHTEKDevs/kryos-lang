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

# --- top-level const initializer runs at startup (backlog #24) ---
want_reject const_init_builtin \
'let mut x: i64 = file_write("/tmp/z","a")
fn main() { println(to_string(x)) }'

# --- Type::builtin(..) static-dispatch disguise (backlog #38) ---
want_reject static_dispatch_builtin \
'fn main() { NotAType::file_write("/tmp/z","a") }'

want_reject static_dispatch_env \
'fn main() { let s: str = Bogus::env_get("PATH")  println(s) }'

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

# Two structs with a same-named method: an annotated Logger.write must not
# suppress the unannotated FileSink.write. main declaring BOTH is accepted...
want_pass collision_main_declares_both \
'struct Logger { t: i64 }
impl Logger { @capabilities(process) fn write(self: Logger) -> str { return env_get("PATH") } }
struct FileSink { t: i64 }
impl FileSink { fn write(self: FileSink) { file_write("/tmp/z", "d") } }
@capabilities(process, fs:write)
fn main() { let f: FileSink = FileSink { t: 1 }  f.write() }'

# ...and main UNDER-declaring (only process, but calls FileSink.write which
# needs fs:write) must still be REJECTED -- the collision must not let the
# fs:write requirement vanish (that was an escape).
want_reject collision_main_underdeclares \
'struct Logger { t: i64 }
impl Logger { @capabilities(process) fn write(self: Logger) -> str { return env_get("PATH") } }
struct FileSink { t: i64 }
impl FileSink { fn write(self: FileSink) { file_write("/tmp/z", "d") } }
@capabilities(process)
fn main() { let f: FileSink = FileSink { t: 1 }  f.write() }'

# --- capability ESCAPE via a user function used as a first-class VALUE -------
# A helper wrapping a gated builtin, aliased/passed/stored instead of called
# directly, must NOT slip its authority past an unannotated boundary. (Was a
# critical escape: check passed and the program actually read the file / env.)
want_reject cap_escape_local_alias \
'fn h(p: str) -> str { return file_read(p) }
fn main() { let f = h  let c = f("/tmp/z")  println(c) }'

want_reject cap_escape_fn_arg \
'fn h(p: str) -> str { return file_read(p) }
fn ap(g: fn(str)->str, p: str) -> str { return g(p) }
fn main() { println(ap(h, "/tmp/z")) }'

want_reject cap_escape_returned \
'fn h(p: str) -> str { return file_read(p) }
fn gr() -> fn(str)->str { return h }
fn main() { let f = gr()  println(f("/tmp/z")) }'

want_reject cap_escape_env_alias \
'fn ge(k: str) -> str { return env_get(k) }
fn main() { let f = ge  println(f("PATH")) }'

# ...but a correctly-declared alias is accepted, a pure alias is accepted, and a
# LOCAL merely named like a gated stdlib function (e.g. `query`, `connect`) must
# NOT be spuriously attributed that function's capability.
want_pass cap_alias_declared \
'@capabilities(fs:read)
fn h(p: str) -> str { return file_read(p) }
@capabilities(fs:read)
fn main() { let f = h  println(f("/tmp/z")) }'

want_pass cap_alias_pure \
'fn add(a: i64, b: i64) -> i64 { return a + b }
fn main() { let f = add  println(to_string(f(2, 3))) }'

want_pass cap_local_named_like_gated_fn \
'struct U { host: str, query: str }
fn main() {
    let host = "example.com"
    let query = "a=1"
    let u = U { host: host, query: query }
    println(u.host + "?" + u.query)
}'

# A USER function that SHADOWS a gated builtin's NAME but does pure work must
# NOT be force-gated by the builtin table (the checker gated by name, not by
# resolved symbol). Direct, via-helper, and as-value forms all accepted.
want_pass cap_shadow_builtin_name_direct \
'fn file_write(x: i64) -> i64 { return x * 2 }
fn main() { println(to_string(file_write(3))) }'

want_pass cap_shadow_builtin_name_helper \
'fn http_get(x: i64) -> i64 { return x + 1 }
fn c() -> i64 { return http_get(5) }
fn main() { println(to_string(c())) }'

if [ "$fail" -eq 0 ]; then
  echo "inferred-soundness: all probes correct (leaks rejected, safe code accepted)"
else
  echo "inferred-soundness: $fail probe(s) WRONG"
fi
[ "$fail" -eq 0 ]
