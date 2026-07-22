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

# --- capability ESCAPE via an ACTOR message handler -------------------------
# An @capabilities-annotated actor whose handler reaches a gated builtin must
# NOT be invokable from an unannotated boundary: `w.dump(..)` exercises the
# actor authority, so the caller must declare it. (Was a critical escape: the
# cap-collection passes never recursed into actor handlers, so the handler name
# never entered fn_capabilities and the call site was ungated -- check passed
# and the program actually wrote the file, in BOTH inferred and strict modes.)
want_reject actor_handler_escape \
'@capabilities(io)
actor Writer { tag: i64,
  fn dump(self, path: str, content: str) { file_write(path, content) } }
fn main() { let w = Writer()  w.dump("/tmp/z", "a") }'

want_reject actor_handler_escape_via_helper \
'@capabilities(io)
actor Writer { tag: i64,
  fn dump(self, path: str, content: str) { do_write(path, content) } }
fn do_write(p: str, c: str) { file_write(p, c) }
fn main() { let w = Writer()  w.dump("/tmp/z", "a") }'

# ...but a caller that DECLARES the actor authority is accepted, and a
# no-authority actor invoked from an unannotated main is accepted.
want_pass actor_caller_declares \
'@capabilities(io)
actor Writer { tag: i64,
  fn dump(self, path: str, content: str) { file_write(path, content) } }
@capabilities(io)
fn main() { let w = Writer()  w.dump("/tmp/z", "a") }'

want_pass actor_pure_handler \
'actor Counter { n: i64,
  fn bump(self, x: i64) { println(to_string(x + 1)) } }
fn main() { let c = Counter()  c.bump(41) }'

# --- capability ESCAPE via a hand-declared RAW NATIVE extern -----------------
# `extern { fn kryos_<native> }` lets code call the runtime symbols behind the
# fs/process/net builtins directly, marshalling args through the ungated
# str_to_ptr/len primitives. The native symbol names differ from the builtin
# names (_ks suffix, builtin_ prefix, verb order: dir_create vs create_dir), so
# mapping only builtin names left them AMBIENT -- an unannotated main reached
# arbitrary process exec / fs write / outbound HTTP with zero caps. Each MUST be
# rejected now (gated to the same authority as the builtin it backs).
want_reject native_extern_process_exec \
'extern { fn kryos_process_exec_simple(c: i64, n: i64) -> i64 }
fn main() { let c = "x"  let r = kryos_process_exec_simple(str_to_ptr(c), len(c))  println(to_string(r)) }'

want_reject native_extern_dir_create \
'extern { fn kryos_dir_create(p: i64, n: i64) -> i64 }
fn main() { let c = "x"  let r = kryos_dir_create(str_to_ptr(c), len(c))  println(to_string(r)) }'

want_reject native_extern_file_remove \
'extern { fn kryos_file_remove(p: i64, n: i64) -> i64 }
fn main() { let c = "x"  let r = kryos_file_remove(str_to_ptr(c), len(c))  println(to_string(r)) }'

want_reject native_extern_https_get_ks \
'extern { fn kryos_https_get_ks(u: i64) -> i64 }
fn main() { let r = kryos_https_get_ks(0)  println(to_string(r)) }'

want_reject native_extern_builtin_env_get \
'extern { fn kryos_builtin_env_get(a: i64) -> i64 }
fn main() { let r = kryos_builtin_env_get(0)  println(to_string(r)) }'

want_reject native_extern_tcp_connect_ks \
'extern { fn kryos_tcp_connect_ks(a: i64, b: i64) -> i64 }
fn main() { let r = kryos_tcp_connect_ks(0, 0)  println(to_string(r)) }'

# ...but declaring the matching capability is accepted, and a genuinely pure
# native (an allocator/pointer helper) stays ambient (not over-gated).
want_pass native_extern_declared \
'extern { fn kryos_dir_create(p: i64, n: i64) -> i64 }
@capabilities(fs:write)
fn main() { let c = "x"  let r = kryos_dir_create(str_to_ptr(c), len(c))  println(to_string(r)) }'

want_pass native_extern_pure_alloc \
'extern { fn kryos_arc_alloc_i64(n: i64) -> i64 }
fn main() { let r = kryos_arc_alloc_i64(8)  println(to_string(r)) }'

# --- capability ESCAPE via SELF-SHADOW of an annotated function -------------
# `let leaker = leaker` (and the laundering variant `let x = leaker; let leaker
# = x`, and a sibling-block shadow) made an annotated function name look like a
# plain local, so the value-authority gate was skipped and its gated builtins
# ran from an unannotated main. The locals set is now ORDER-SENSITIVE + block-
# scoped, so a self-shadow RHS still resolves to the function and is gated.
want_reject cap_self_shadow_direct \
'@capabilities(process)
fn leaker() -> str { return env_get("PATH") }
fn main() { let leaker = leaker  let f = leaker  println(f()) }'

want_reject cap_self_shadow_field \
'@capabilities(process)
fn leaker() -> str { return env_get("PATH") }
struct Box { f: fn() -> str }
fn main() { let leaker = leaker  let b = Box { f: leaker }  println(b.f()) }'

want_reject cap_self_shadow_launder \
'@capabilities(process)
fn leaker() -> str { return env_get("PATH") }
fn main() { let x = leaker  let leaker = x  println(leaker()) }'

want_reject cap_self_shadow_sibling_block \
'@capabilities(process)
fn leaker() -> str { return env_get("PATH") }
fn main() { let c = true  if c { let leaker = leaker  println(leaker()) } else { println("no") } }'

# ...but self-shadowing a PURE (unannotated, no-authority) function is fine, and
# a local merely NAMED like a stdlib function is still not force-gated.
want_pass cap_self_shadow_pure_fn \
'fn add(a: i64, b: i64) -> i64 { return a + b }
fn main() { let add = add  println(to_string(add(2, 3))) }'

# --- std::fs write helpers must be CALLABLE with fs:write ---------------------
# write_file / append_file open the file with a WRITE mode via the neutral
# kryos_file_open extern. Mapping file_open to fs:read made them demand fs:read,
# so a caller declaring fs:write could not use them at all (uncallable under
# every declaration). Opening is neutral; the write authority is kryos_file_write.
want_pass stdlib_write_file_fs_write \
'use std::fs::{write_file}
@capabilities(fs:write)
fn main() { write_file("/tmp/kx_probe", "d") }'

want_pass stdlib_append_file_fs_write \
'use std::fs::{append_file}
@capabilities(fs:write)
fn main() { append_file("/tmp/kx_probe", "d") }'

# ...and read_file still requires fs:read (opening for read is neutral, but the
# actual kryos_file_read carries the authority).
want_reject stdlib_read_file_needs_read \
'use std::fs::{read_file}
fn main() { let s = read_file("/tmp/kx_probe")  println(s) }'

if [ "$fail" -eq 0 ]; then
  echo "inferred-soundness: all probes correct (leaks rejected, safe code accepted)"
else
  echo "inferred-soundness: $fail probe(s) WRONG"
fi
[ "$fail" -eq 0 ]
