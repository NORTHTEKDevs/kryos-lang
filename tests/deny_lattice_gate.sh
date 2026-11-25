#!/usr/bin/env bash
# deny_lattice_gate.sh -- the full deny! x used-capability lattice, both layers, both modes.
#
# For every (DENIED, USED) pair over {every coarse family, every fine capability, io/fs, all} the verdict of
# `kryos check` must be REJECT iff the two capabilities OVERLAP (equal, one is `all`, or one is the coarse
# family of the other), else ACCEPT. The expectation is written out here from the lattice, not read from the
# compiler. LEDGER 121: deny!(fs) / deny!(io) / deny!(all) accepted a block using fs:read in the row layer
# while deny!(fs:read) rejected it; a fine deny must still allow a sibling (deny!(fs:read) with fs:write).
#
# Two program shapes per pair:
#   direct  -- the block calls a declared `@capabilities(USED)` fn. The shape layer (kryos-capabilities) sees it.
#   blinded -- the block calls a closure that came out of an array, under a harmless outer `f`. The shape layer
#              cannot see the callee, so only the ROW layer (kryos-types) can reject it. The row is charged from a
#              real builtin the closure calls, so only capabilities some builtin maps to can be USED here
#              (fs:read fs:write net:http net:tcp net process crypto); the rest of the used axis is pinned at
#              the row-verdict function by the `denied_caps_used` unit test in kryos-types/src/ty.rs.
# Each shape runs in default and --strict-capabilities mode. Every REJECT must carry the deny code (E0110 for
# blinded; direct is rejected by either layer, so only the nonzero rc is asserted there).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

CAPS="net net:http net:tcp io fs fs:read fs:write ffi compute crypto process env term db time all"
canon() { [ "$1" = fs ] && echo io || echo "$1"; }
fam() { case "$1" in net:*) echo net ;; fs:*) echo io ;; *) echo "$1" ;; esac; }
# overlap D U: 0 when they share authority
overlap() {
    local d u; d="$(canon "$1")"; u="$(canon "$2")"
    [ "$d" = all ] || [ "$u" = all ] || [ "$d" = "$u" ] && return 0
    [ "$d" = "$(fam "$u")" ] && return 0
    [ "$u" = "$(fam "$d")" ] && return 0
    return 1
}

# the builtin call that charges each capability in the blinded shape
use_expr() {
    case "$1" in
        fs:read) echo 'file_read("tests/security/secret_for_closure_launder.txt")' ;;
        fs:write) echo 'file_write("deny_lattice_never_written.txt", "x")' ;;
        net:http) echo 'https_get("http://127.0.0.1:1/")' ;;
        net:tcp) echo 'tcp_connect("127.0.0.1", 1)' ;;
        net) echo 'pg_connect("x")' ;;
        process) echo 'env_get("DENY_LATTICE_UNSET")' ;;
        crypto) echo 'sha256("x")' ;;
        *) echo "" ;;
    esac
}

bad=0; n=0
for d in $CAPS; do
  for u in $CAPS; do
    if overlap "$d" "$u"; then want=reject; else want=accept; fi
    cat > "$TMP/direct.kry" <<EOF
@capabilities($u)
fn use_u() -> i64 {
    return 1
}

@capabilities($u)
fn main() {
    deny!($d) {
        let x = use_u()
        println(x)
    }
}
EOF
    cat > "$TMP/blinded.kry" <<EOF
@capabilities($u)
fn use_u() -> i64 {
    let v = $(use_expr "$u")
    return 1
}

@capabilities($u)
fn mk() -> fn() -> i64 {
    return || use_u()
}

@capabilities($u)
fn main() {
    let f = || 0
    let r = mk()
    let fns = [r]
    deny!($d) {
        for f in fns {
            println(f())
        }
    }
}
EOF
    for shape in direct blinded; do
      [ "$shape" = blinded ] && [ -z "$(use_expr "$u")" ] && continue
      for mode in "" --strict-capabilities; do
        n=$((n+1))
        out="$(timeout 90 "$K" check $mode "$TMP/$shape.kry" 2>&1)"; rc=$?
        if [ $rc -eq 0 ]; then got=accept; else got=reject; fi
        why=""
        if [ "$got" != "$want" ]; then why="expected $want, got $got"
        elif [ "$want" = reject ] && [ "$shape" = blinded ] && ! printf '%s' "$out" | grep -q E0110; then why="rejected without E0110"; fi
        if [ -n "$why" ]; then bad=$((bad+1)); echo "  FAIL deny!($d) used=$u shape=$shape mode=${mode:-default}: $why"; fi
      done
    done
  done
done
echo "deny-lattice: $n verdicts checked, $bad wrong"
[ "$bad" -eq 0 ]
