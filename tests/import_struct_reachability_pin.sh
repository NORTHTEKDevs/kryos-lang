#!/usr/bin/env bash
# import_struct_reachability_pin.sh -- import-struct-reachability: selective
# imports always include a module's types, so `use ma::{fa}` + `use mb::{fb}`
# with an `Item` struct in BOTH modules was rejected with E0205 although no
# program text names `Item`. A type name nothing mentions must not collide;
# a name that IS mentioned (struct literal, signature) must still report E0205
# because which `Item` is meant cannot be told.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
bad=0

cat > "$W/ma.kry" <<'K'
struct Item { a: i64 }
impl Item { fn get(self) -> i64 { return self.a } }
fn fa() -> i64 { return 1 }
K
cat > "$W/mb.kry" <<'K'
struct Item { b: i64 }
fn fb() -> i64 { return 2 }
K

ok() { # name, source, expected stdout
  local out rc
  printf '%s\n' "$2" > "$W/$1.kry"
  out="$(cd "$W" && timeout 60 "$K" run "$1.kry" 2>&1)"; rc=$?
  if [ "$rc" != 0 ] || [ "$out" != "$3" ]; then
    echo "FAIL $1: rc=$rc want [$3] got [$(printf '%s' "$out" | head -c 200 | tr '\n' '|')]"
    bad=1
  fi
}
rej() { # name, source: must still be E0205
  local out
  printf '%s\n' "$2" > "$W/$1.kry"
  out="$(cd "$W" && timeout 60 "$K" run "$1.kry" 2>&1)"
  if ! grep -q 'E0205' <<<"$out"; then
    echo "FAIL $1: want E0205, got [$(printf '%s' "$out" | head -c 200 | tr '\n' '|')]"
    bad=1
  fi
}

ok dead $'use ma::{fa}\nuse mb::{fb}\nfn main() { println(to_string(fa() + fb())) }' "3"
rej lit $'use ma::{fa}\nuse mb::{fb}\nfn main() {\n let x = Item { a: 1 }\n println(to_string(fa() + fb() + x.a))\n}'
rej sig $'use ma::{fa}\nuse mb::{fb}\nfn g(x: Item) -> i64 { return 0 }\nfn main() { println(to_string(fa() + fb())) }'
rej named $'use ma::{fa, Item}\nuse mb::{fb}\nfn main() { println(to_string(fa() + fb())) }'

[ "$bad" = 0 ] && echo "PASS import_struct_reachability_pin (dead lit sig named)" || exit 1
