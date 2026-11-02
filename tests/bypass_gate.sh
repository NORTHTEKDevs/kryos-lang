#!/usr/bin/env bash
# Bypass gate: zero-authority capability bypass classes B1, B2, B3, B4 and B8.
#
# Kryos is deployed as capability-attenuated infrastructure for agent tooling.
# These are programs that declare NO capability (or fewer than they use) and
# still read the environment or files, read or write process memory, call C
# functions, or were handed an artifact the checker had rejected -- in every
# capability mode. Each class was found by running it on 2026-11-01 against
# master dcf72e93; the repros live in tests/bypass/ and every one is pinned in
# BOTH directions here: the bypass is rejected (check rc and error code, default
# and --strict-capabilities, plus permissive for the classes that hold in every
# mode), and the legitimate neighbours still compile and run (so the fix cannot
# be "reject everything").
#
#   B1  a builtin name shadowed by a METHOD (impl method, imported impl, raw
#       memory builtins) -- and B1b, the same name-confusion on the callee side:
#       a fn-typed parameter named like a method / free fn / builtin
#   B2  the same with only a bodiless TRAIT method declaration
#   B3  std::ffi needs no capability (+ stdlib helpers that read an address the
#       caller supplies, + std::wasm network wrappers)
#   B4  an `extern` naming a kryos_* native the capability table did not map
#   B8  `kryos build --cache` served an artifact before the checker ran
#
# Part 9 runs the cargo tests that keep the tables honest: every exported
# kryos_* symbol is classified, std::ffi's exports are all declared, no stdlib
# function declares authority it does not. Set BYPASS_GATE_SKIP_CARGO=1 to skip
# them (no toolchain on the box); CI does not.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
# Honor KRYOS_BIN like the other gates do (the CI Windows job passes it).
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
EXE=""; case "$K" in *.exe) EXE=".exe";; esac
# absolute path of the binary, for the sections that run it from a scratch directory
case "$K" in /*|[A-Za-z]:*) KABS="$K";; *) KABS="$ROOT/$K";; esac
fail=0
B=tests/bypass

strip() { sed 's/\x1b\[[0-9;]*m//g'; }

modeflag() {
    case "$1" in
        default) echo "";;
        strict) echo "--strict-capabilities";;
        permissive) echo "--capabilities-mode=permissive";;
    esac
}

# reject <program> <egrep that the diagnostics must match> <mode>...
#   `kryos check` must exit non-zero in every listed mode AND say why.
reject() {
    local prog="$1" want="$2"; shift 2
    local ok=1 mode out
    for mode in "$@"; do
        out="$("$K" check $(modeflag "$mode") "$prog" 2>&1)"; local rc=$?
        out="$(printf '%s' "$out" | strip)"
        if [ $rc -eq 0 ]; then
            echo "  FAIL: $prog passed \`check\` in $mode mode -- the bypass is open"; ok=0; fail=1
        elif ! printf '%s' "$out" | grep -qE "$want"; then
            echo "  FAIL: $prog rejected in $mode mode, but not for the capability reason ($want):"
            printf '%s\n' "$out" | head -4 | sed 's/^/        /'; ok=0; fail=1
        fi
    done
    [ $ok -eq 1 ] && echo "  ok   $(basename "$prog") rejected [$want] in: $*"
}

# run_silent <program> <marker> : `kryos run` must NOT print the marker as a
# line of program output. (A rejected program's diagnostic can quote the source
# line holding the marker's text, so only a line that STARTS with the marker
# counts -- a quoted source excerpt starts with a gutter, never with the marker.)
run_silent() {
    local prog="$1" marker="$2" out
    out="$("$K" run "$prog" 2>&1 | strip)"
    if printf '%s
' "$out" | grep -qE "^$marker"; then
        echo "  FAIL: \`kryos run $(basename "$prog")\` printed $marker -- it executed"; fail=1
    else
        echo "  ok   $(basename "$prog") no longer runs (no $marker)"
    fi
}

# accept <program> <mode>... : `kryos check` must exit 0 in every listed mode.
accept() {
    local prog="$1"; shift
    local ok=1 mode out
    for mode in "$@"; do
        if ! out="$("$K" check $(modeflag "$mode") "$prog" 2>&1)"; then
            echo "  FAIL: $(basename "$prog") rejected in $mode mode -- a legitimate program over-rejected:"
            printf '%s\n' "$out" | strip | head -4 | sed 's/^/        /'; ok=0; fail=1
        fi
    done
    [ $ok -eq 1 ] && echo "  ok   $(basename "$prog") accepted in: $*"
}

# prints <program> <expected line> : `kryos run` output must contain it.
prints() {
    local prog="$1" want="$2" out
    out="$("$K" run "$prog" 2>&1 | strip)"
    if printf '%s' "$out" | grep -qF -- "$want"; then
        echo "  ok   $(basename "$prog") runs and prints: $want"
    else
        echo "  FAIL: $(basename "$prog") did not print '$want':"
        printf '%s\n' "$out" | head -3 | sed 's/^/        /'; fail=1
    fi
}

echo "binary: $K ($("$K" --version 2>&1 | head -1))"

echo "== 1. B1/B2: a builtin name is shadowed only by a FREE function"
for p in b1_env_get_shadowed_by_method b2_env_get_shadowed_by_trait_decl b1_env_get_shadowed_by_imported_impl; do
    reject "$B/$p.kry" 'E0505.*`env_get` requires `process`' default strict
done
run_silent "$B/b1_env_get_shadowed_by_method.kry" BYPASS-B1-OK
run_silent "$B/b2_env_get_shadowed_by_trait_decl.kry" BYPASS-B2-OK
run_silent "$B/b1_env_get_shadowed_by_imported_impl.kry" BYPASS-B1-IMPORTED-OK
# the raw-memory gate holds in EVERY mode, so the decoy must not open it in any
reject "$B/b1_raw_memory_shadowed_by_method.kry" 'E0505.*requires `ffi`' default strict permissive
run_silent "$B/b1_raw_memory_shadowed_by_method.kry" 'recovered: TOPSECRET-B1-RAWMEM'
accept "$B/ctl_free_fn_shadows_builtin.kry" default strict
prints "$B/ctl_free_fn_shadows_builtin.kry" 'CTL-FREEFN-OK!'
accept "$B/ctl_method_call_named_like_builtin.kry" default strict
prints "$B/ctl_method_call_named_like_builtin.kry" 'CTL-METHOD-OK?'
accept "$B/ctl_static_method_named_like_builtin.kry" default strict
prints "$B/ctl_static_method_named_like_builtin.kry" 'ran: CTL-STATIC-OK'

echo "== 1b. B1b: a fn-typed LOCAL shadows every global of its name in call position"
for p in b1b_param_named_like_method b1b_param_named_like_free_fn b1b_param_named_like_builtin; do
    reject "$B/$p.kry" 'E0507.*call through a function value requires capabilities \[all\]' default strict
    run_silent "$B/$p.kry" B1B-SECRET-MARKER
done
accept "$B/ctl_local_fn_value_named_like_builtin.kry" default strict
prints "$B/ctl_local_fn_value_named_like_builtin.kry" 'CTL-LOCAL-OK:5:1003'

echo "== 3. B3: std::ffi (and what hands out raw memory like it) needs ffi at the caller"
for p in b3_std_ffi_zero_cap b3_std_ffi_qualified b3_std_ffi_c_call b3_std_ffi_under_deny; do
    reject "$B/$p.kry" 'E050[57].*(requires `ffi`|requires capabilities \[ffi\])' default strict permissive
done
# whole-module / glob imports bring in all 32 exports (checked only: on master that form
# passes `check` and fails at codegen, so there is no marker to look for)
reject "$B/b3_std_ffi_glob_import.kry" 'E050[57].*(requires `ffi`|requires capabilities \[ffi\])' default strict permissive
run_silent "$B/b3_std_ffi_zero_cap.kry" BYPASS-B3-OK
run_silent "$B/b3_std_ffi_qualified.kry" BYPASS-B3-QUALIFIED-OK
run_silent "$B/b3_std_ffi_c_call.kry" BYPASS-B3-CCALL-OK
reject "$B/b3_stdlib_helper_reads_caller_address.kry" 'E0505.*`arr_data_ptr` requires `ffi`' default strict permissive
reject "$B/b3_wasm_fetch_undeclared.kry" 'E0507.*requires capabilities \[net:http\]' default strict
accept "$B/ctl_declared_std_ffi.kry" default strict permissive
prints "$B/ctl_declared_std_ffi.kry" 'CTL-FFI-OK'
accept "$B/ctl_stdlib_zero_cap.kry" default strict permissive
prints "$B/ctl_stdlib_zero_cap.kry" 'CTL-STDLIB-OK'

echo "== 4. B4: an extern naming a kryos_* native is classified; the unclassified fail closed"
reject "$B/b4_extern_ffi_cstr.kry" 'E0506.*`kryos_ffi_cstr` requires `ffi`' default strict permissive
run_silent "$B/b4_extern_ffi_cstr.kry" BYPASS-B4-OK
reject "$B/b4_extern_unknown_native.kry" 'E0506.*requires `ffi`' default strict permissive
reject "$B/b4_extern_raw_pointer_natives.kry" 'E0506.*requires `ffi`' default strict permissive
reject "$B/b4_extern_db_open.kry" 'E0506.*`kryos_db_open_memory` requires `db`' default strict
reject "$B/b4_extern_tls_connect.kry" 'E0506.*`kryos_tls_connect` requires `net:tcp`' default strict
reject "$B/b4_extern_term_raw_enable.kry" 'E0506.*`kryos_term_raw_enable` requires `term`' default strict
reject "$B/b4_extern_fs_read.kry" 'E0506.*`kryos_fs_read` requires `fs:read`' default strict
reject "$B/b4_extern_file_open_truncate.kry" 'E0506.*`kryos_file_open` requires `fs:write`' default strict
reject "$B/b4_extern_file_open_write_with_read_cap.kry" 'E0506.*`kryos_file_open` requires `fs:write`' default strict
accept "$B/ctl_declared_extern_ffi.kry" default strict permissive
prints "$B/ctl_declared_extern_ffi.kry" 'CTL-B4-FFI-OK'
accept "$B/ctl_ambient_natives.kry" default strict
prints "$B/ctl_ambient_natives.kry" 'CTL-B4-AMBIENT-OK'
accept "$B/ctl_file_open_read_mode.kry" default strict
prints "$B/ctl_file_open_read_mode.kry" 'CTL-B4-OPEN-READ-OK'

echo "== 8. B8: a cached artifact is never served for a build the checker would reject"
# NO subshells in this section: a failure recorded inside one would be lost.
b8() { echo "  FAIL: $*"; b8_bad=1; fail=1; }
cache_build() { # cache_build <cache dir> <build args...> : run kryos build with that cache dir
    local dir="$1"; shift
    env KRYOS_CACHE_DIR="$dir" "$KABS" build "$@"
}
unset KRYOS_CACHE
CACHE="$TMP/cache"; OUT="$TMP/b8out"; mkdir -p "$CACHE" "$OUT"
cp "$B/b8_cache_target.kry" "$OUT/x.kry"
cd "$OUT" || { echo "  FAIL: cannot enter $OUT"; fail=1; }
b8_bad=0
# step 1: the default build is rejected (the verdict the cache must not override)
if cache_build "$CACHE" x.kry -o s1$EXE >/dev/null 2>&1; then b8 "step 1: default build of x.kry succeeded"; fi
# step 2: the same source under permissive is accepted and cached
if ! cache_build "$CACHE" -v --capabilities-mode=permissive --cache x.kry -o s2$EXE >s2.log 2>&1; then
    b8 "step 2: permissive --cache build failed"; fi
grep -q 'cache: stored' s2.log || b8 "step 2: nothing was cached"
# step 3: default, strict and KRYOS_CACHE=1 must STILL be rejected, not handed the permissive artifact
rm -f s3$EXE s3b$EXE s3c$EXE
if cache_build "$CACHE" --cache x.kry -o s3$EXE >s3.log 2>&1; then b8 "step 3: default --cache build was served"; fi
[ -e "s3$EXE" ] && b8 "step 3: default --cache wrote an artifact"
grep -q 'E0505' s3.log || b8 "step 3: default --cache was not rejected with E0505"
if cache_build "$CACHE" --strict-capabilities --cache x.kry -o s3b$EXE >s3b.log 2>&1; then b8 "step 3b: strict --cache build was served"; fi
[ -e "s3b$EXE" ] && b8 "step 3b: strict --cache wrote an artifact"
if env KRYOS_CACHE=1 KRYOS_CACHE_DIR="$CACHE" "$KABS" build x.kry -o s3c$EXE >s3c.log 2>&1; then b8 "step 3c: KRYOS_CACHE=1 build was served"; fi
[ -e "s3c$EXE" ] && b8 "step 3c: KRYOS_CACHE=1 wrote an artifact"
# control: the cache still works for the mode that built the entry
if ! cache_build "$CACHE" -v --capabilities-mode=permissive --cache x.kry -o s4$EXE >s4.log 2>&1; then
    b8 "step 4: permissive --cache rebuild failed"; fi
grep -q 'cache: hit' s4.log || b8 "step 4 (control): the permissive rebuild did not hit the cache"
cd "$ROOT"
[ $b8_bad -eq 0 ] && echo "  ok   default/strict/KRYOS_CACHE builds are rejected after a permissive --cache build; permissive still hits"

IMP="$TMP/b8imp"; mkdir -p "$IMP"
cp "$B/b8_cache_imported_module.kry" "$IMP/m.kry"; cp "$B/b8_cache_imported_main.kry" "$IMP/prog.kry"
cd "$IMP" || { echo "  FAIL: cannot enter $IMP"; fail=1; }
b8_bad=0
cache_build "$TMP/cache2" --cache prog.kry -o a$EXE >/dev/null 2>&1 || b8 "imported: cold build failed"
first="$(./a$EXE 2>&1 | tr -d '\r')"
[ "$first" = "answer-v1" ] || b8 "imported: first build printed '$first', not answer-v1"
sed 's/answer-v1/answer-v2/' m.kry > m.kry.new && mv m.kry.new m.kry
cache_build "$TMP/cache2" -v --cache prog.kry -o c$EXE >c.log 2>&1 || b8 "imported: rebuild after editing the module failed"
grep -q 'cache: hit' c.log && b8 "imported: an edited imported module HIT the cache (stale artifact)"
second="$(./c$EXE 2>&1 | tr -d '\r')"
[ "$second" = "answer-v2" ] || b8 "imported: after editing m.kry the program printed '$second', not answer-v2"
cd "$ROOT"
[ $b8_bad -eq 0 ] && echo "  ok   an edited imported module misses the cache and the new artifact runs"

# the project's kryos.toml [capabilities] mode is an input of the verdict too
PRJ="$TMP/b8proj"; mkdir -p "$PRJ/src"; cp "$B/b8_cache_target.kry" "$PRJ/src/main.kry"
printf '[package]
name = "cacheproj"
version = "0.1.0"

[capabilities]
mode = "permissive"
' > "$PRJ/kryos.toml"
cd "$PRJ" || { echo "  FAIL: cannot enter $PRJ"; fail=1; }
b8_bad=0
cache_build "$TMP/cache3" --cache . >/dev/null 2>&1 || b8 "manifest: the permissive project --cache build failed"
printf '[package]
name = "cacheproj"
version = "0.1.0"
' > kryos.toml
rm -f "cacheproj$EXE"
if cache_build "$TMP/cache3" --cache . >p.log 2>&1; then b8 "manifest: dropping the manifest's permissive mode still got the cached artifact"; fi
[ -e "cacheproj$EXE" ] && b8 "manifest: the rejected build wrote an artifact"
grep -q 'E0505' p.log || b8 "manifest: the build was not rejected with E0505"
cd "$ROOT"
[ $b8_bad -eq 0 ] && echo "  ok   dropping a manifest's permissive mode is rejected, not served the cached artifact"

echo "== 9. the tables stay honest (cargo tests)"
if [ "${BYPASS_GATE_SKIP_CARGO:-0}" = "1" ]; then
    echo "  skip BYPASS_GATE_SKIP_CARGO=1"
elif ! command -v cargo >/dev/null 2>&1; then
    echo "  FAIL: cargo not found (set BYPASS_GATE_SKIP_CARGO=1 to skip these)"; fail=1
else
    J="${BYPASS_GATE_JOBS:-2}"
    if ( cd compiler && cargo test --release -j "$J" -p kryos-capabilities --test native_symbols --test stdlib_raw_exports >"$TMP/cargo1.log" 2>&1 ); then
        echo "  ok   every exported kryos_* native is classified; std::ffi exports and raw-address helpers are gated ($(grep -h 'test result' "$TMP/cargo1.log" | sed 's/test result: ok. //' | awk '{s+=$1} END {print s}') tests)"
    else
        echo "  FAIL: native_symbols / stdlib_raw_exports:"; grep -E 'panicked|FAILED|error' "$TMP/cargo1.log" | head -8 | sed 's/^/        /'; fail=1
    fi
    if ( cd compiler && cargo test --release -j "$J" -p kryos-driver --test stdlib_authority >"$TMP/cargo2.log" 2>&1 ); then
        echo "  ok   no stdlib function exercises authority it does not declare"
    else
        echo "  FAIL: stdlib_authority:"; grep -E 'panicked|FAILED|error|std::' "$TMP/cargo2.log" | head -8 | sed 's/^/        /'; fail=1
    fi
fi

echo
if [ $fail -eq 0 ]; then
    echo "bypass-gate: PASS"
    exit 0
fi
echo "bypass-gate: FAIL"
exit 1
