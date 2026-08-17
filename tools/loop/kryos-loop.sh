#!/usr/bin/env bash
# kryos-loop - the production-hardening driver.
#
# Encodes the traps that actually cost time on this codebase, so a fresh
# session cannot rediscover them. Every guard below exists because something
# went wrong without it.
#
#   preflight     build freshness + machine health. RUN THIS FIRST, ALWAYS.
#   repro <f>     characterize a program: both backends, leak diag, determinism
#   diff <f>      differential JIT vs AOT (the shape of most real bugs here)
#   gates [tier]  the gate ladder, in the order that avoids false failures
#   soak <f>      4x-scale memory check (a leak scales, a fixed shape does not)
#   ledger        the ranked queue with what has been RULED OUT
#
# Usage:  tools/loop/kryos-loop.sh <command> [args]
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
K="compiler/target/release/kryos.exe"
[ -x "$K" ] || K="compiler/target/release/kryos"
TMP="${TMPDIR:-/tmp}/kryos-loop"
mkdir -p "$TMP"

red()  { printf "\033[31m%s\033[0m\n" "$*"; }
grn()  { printf "\033[32m%s\033[0m\n" "$*"; }
ylw()  { printf "\033[33m%s\033[0m\n" "$*"; }

# ---------------------------------------------------------------------------
# preflight - never skip. Two of these cost hours each.
# ---------------------------------------------------------------------------
cmd_preflight() {
    local bad=0

    # TRAP 1: `cargo build -p kryos-cli` does NOT regenerate kryos_rt.lib /
    # kryos_stdlib_native.lib -- the STATICLIB archives an AOT-compiled program
    # links. It only rebuilds the rlibs the compiler itself uses. Measuring a
    # runtime change after a -p build silently tests the OLD runtime. This cost
    # a full wrong diagnosis: an allocator fix measured as "no effect" and a
    # socket-duplication theory "ruled out", both against a 10-hour-old archive.
    echo "== staticlib freshness =="
    local newest_src archive
    newest_src=$(find compiler/crates/kryos-rt/src compiler/crates/kryos-stdlib-native/src \
                      -name '*.rs' -newer compiler/target/release/kryos_rt.lib 2>/dev/null | head -3)
    if [ -n "$newest_src" ]; then
        red "  STALE: runtime sources are newer than kryos_rt.lib"
        echo "$newest_src" | sed 's/^/    /'
        red "  -> run a FULL 'cargo build --release' (no -p) before measuring anything"
        bad=1
    else
        grn "  ok: staticlib archives are current"
    fi

    # TRAP 2: this machine degrades under sustained compilation. The bootstrap
    # gate then fails with rc=127 on ROTATING modules -- pure contention, not a
    # regression. Chasing one of those is a guaranteed dead end.
    echo "== stray test processes =="
    local strays
    strays=$(tasklist 2>/dev/null | grep -icE "^(srv|rest|leak|sal|tal|panicky|mp_)" | head -1); strays=${strays:-0}
    if [ "$strays" -gt 0 ]; then
        ylw "  $strays stray test process(es) -- kill before gating (they skew memory + cause rc=127)"
    else
        grn "  ok: no strays"
    fi

    # TRAP 3: the `kryos` on PATH can be MONTHS behind this repo while reporting a
    # HIGHER version than the source. On 2026-08-16 the installed binary said
    # "1.0.0-beta.1" against a repo at 0.9.0 and rejected a valid 315-line program
    # with 72 errors that all read as language bugs (undefined `None`, `Dict<K,V>`
    # mismatch, "unknown capability `fs:read`"). Three separate agents hit it while
    # simply trying to USE the language. No other gate catches this, because every
    # other gate invokes compiler/target/release/kryos.exe by path.
    echo "== installed toolchain vs this repo =="
    if ! bash tests/installed_toolchain_check.sh 2>&1 | sed 's/^/  /'; then
        bad=1
    fi

    echo "== tree =="
    if [ -n "$(git status --porcelain)" ]; then
        ylw "  dirty working tree:"; git status --porcelain | head -5 | sed 's/^/    /'
    else
        grn "  ok: clean"
    fi
    echo "  HEAD: $(git log --oneline -1)"
    echo "  vs origin: $(git rev-list --count origin/master..HEAD 2>/dev/null || echo '?') commit(s) unpushed"

    return $bad
}

# ---------------------------------------------------------------------------
# repro - characterize BEFORE theorizing. This is the discipline that was
# missing every time an attribution turned out wrong: three in a row this
# session, each from reasoning about code instead of measuring it.
# ---------------------------------------------------------------------------
cmd_repro() {
    local f="${1:?usage: repro <file.kry>}"
    local base; base=$(basename "$f" .kry)

    echo "== $f =="
    echo "-- JIT (Cranelift) x3, checking determinism --"
    local codes=""
    for _ in 1 2 3; do
        timeout 200 "$K" run "$f" >"$TMP/$base.jit" 2>&1
        codes="$codes $?"
    done
    echo "   rc:$codes"
    case "$(echo $codes | tr -d ' 0')" in
        "") grn "   deterministic (all rc=0)";;
        *)  if [ "$(echo $codes | tr ' ' '\n' | sort -u | wc -l)" -gt 1 ]; then
                red "   NONDETERMINISTIC -- rerun alone before drawing any conclusion"
            else
                ylw "   deterministic failure"
            fi;;
    esac
    tail -3 "$TMP/$base.jit" | sed 's/^/   | /'

    echo "-- AOT (LLVM) --"
    if timeout 400 "$K" build --release "$f" -o "$TMP/$base.exe" >"$TMP/$base.build" 2>&1; then
        (cd "$TMP" && timeout 200 "./$base.exe" >"$base.aot" 2>&1); echo "   rc=$?"
        tail -3 "$TMP/$base.aot" | sed 's/^/   | /'
        if diff -q "$TMP/$base.jit" "$TMP/$base.aot" >/dev/null 2>&1; then
            grn "   backends AGREE -> defect is in MIR/shared lowering, not a backend"
        else
            red "   backends DIVERGE -> compare the emitted IR, not the source"
        fi
    else
        red "   AOT build failed"; tail -4 "$TMP/$base.build" | sed 's/^/   | /'
    fi

    # KRYOS_FREE_DIAG never deallocates and reports every free of an
    # already-released header. Two things it is uniquely good for: proving a
    # crash is memory corruption rather than logic (a program that dies
    # normally but COMPLETES under diag is corrupt, not buggy), and giving a
    # COUNT so a candidate fix can be shown to change it rather than to hide it.
    echo "-- KRYOS_FREE_DIAG (double-free census) --"
    KRYOS_FREE_DIAG=1 timeout 200 "$K" run "$f" >"$TMP/$base.diag" 2>&1
    local drc=$? n
    n=$(grep -c "DOUBLE-FREE" "$TMP/$base.diag" 2>/dev/null || echo 0)
    echo "   double-frees=$n  rc-under-diag=$drc"
    grep "DOUBLE-FREE" "$TMP/$base.diag" 2>/dev/null | head -3 | sed 's/^/   | /'
    if [ "$n" -gt 0 ] && [ "$drc" = "0" ]; then
        red "   COMPLETES under diag but fails normally => the failure IS the corruption."
        red "   Do not debug the reported error; it is a symptom."
    fi
}

# ---------------------------------------------------------------------------
# diff - differential execution. Most real defects here were AOT-only.
# ---------------------------------------------------------------------------
cmd_diff() {
    local f="${1:?usage: diff <file.kry>}"
    local base; base=$(basename "$f" .kry)
    timeout 200 "$K" run "$f" >"$TMP/d.jit" 2>&1; local j=$?
    timeout 400 "$K" build --release "$f" -o "$TMP/d.exe" >/dev/null 2>&1 || { red "AOT build failed"; return 1; }
    (cd "$TMP" && timeout 200 ./d.exe >d.aot 2>&1); local a=$?
    if [ "$j" = "$a" ] && diff -q "$TMP/d.jit" "$TMP/d.aot" >/dev/null 2>&1; then
        grn "IDENTICAL (rc=$j)"
    else
        red "DIVERGE jit_rc=$j aot_rc=$a"; diff "$TMP/d.jit" "$TMP/d.aot" | head -12 | sed 's/^/  /'
    fi
}

# ---------------------------------------------------------------------------
# gates - ORDER MATTERS. Bootstrap runs alone and last: run it beside the
# examples/strict-caps sweeps and it fails spuriously on rotating modules.
# ---------------------------------------------------------------------------
cmd_gates() {
    local tier="${1:-2}" fail=0
    run() { printf "  %-26s " "$1"; shift; if timeout 2400 "$@" >"$TMP/g.log" 2>&1; then grn PASS; else red FAIL; tail -4 "$TMP/g.log" | sed 's/^/     /'; fail=1; fi; }

    echo "== tier 1: fast correctness =="
    printf "  %-26s " "conformance"
    if bash tests/conformance/run_conformance.sh 2>&1 | tee "$TMP/c.log" | grep -q "PASS$"; then
        grn "$(grep -oE '[0-9]+/[0-9]+ PASS' "$TMP/c.log" | tail -1)"
    else
        red FAIL; grep -viE "^  OK" "$TMP/c.log" | head -5 | sed 's/^/     /'; fail=1
    fi
    run "no_double_free"      bash tests/no_double_free.sh
    run "type_soundness"      bash tests/type_soundness.sh
    run "inferred_soundness"  bash tests/inferred_soundness.sh
    run "match_exhaustiveness" bash tests/match_exhaustiveness.sh
    run "concurrency_smoke"   bash tests/concurrency_smoke.sh
    run "module_case_gate"    bash tests/module_case_gate.sh
    run "docs_status_gate"    bash tests/docs_status_gate.sh
    run "utf8_invalid_string" bash tests/utf8_invalid_string_gate.sh
    run "backend_divergence_pins" bash tests/backend_divergence_pins.sh
    run "diagnostics"         bash tests/diagnostics_gate.sh
    run "assert_shadow"       bash tests/assert_shadow_gate.sh
    run "parser_nesting"      bash tests/parser_nesting_gate.sh
    run "stdlib_compile"      bash tests/stdlib_compile_gate.sh
    run "cli_smoke"           bash tests/cli_smoke_gate.sh
    run "minilisp"            bash tests/minilisp_gate.sh
    run "authority_surface"   bash tests/authority_surface_gate.sh
    run "jit_symbols"         bash tests/jit_symbols_gate.sh
    run "selfhost_regressions" bash compiler/self-host/test_regressions.sh
    [ "$tier" -lt 2 ] && { [ $fail -eq 0 ] && grn "tier1 GREEN" || red "tier1 RED"; return $fail; }

    echo "== tier 2: behaviour, not just compilation =="
    run "wasm_differential"   bash tests/wasm_differential_gate.sh
    run "capability_matrix"   bash tests/capability_matrix_gate.sh
    # These exist because "it compiles" was the whole bar and three real
    # defects shipped through a gate that only compiled the examples.
    run "examples"            bash tests/run_examples_gate.sh
    run "strict_caps"         bash tests/strict_caps_examples.sh
    run "examples_e2e"        bash tests/run_examples_e2e.sh
    run "ir_signatures"       bash tests/ir_signature_gate.sh
    # Whole-program check of the self-host compiler under a time ceiling.
    # Tier 2 rather than tier 1: it costs ~50s, and it is a behaviour gate
    # (does the front end still SCALE), not a fast correctness one. See the
    # script's header + LEDGER item 39 -- this is the only gate that would
    # have caught a superlinear front-end change before it broke bootstrap.
    run "selfhost_wholeprogram" bash tests/selfhost_wholeprogram_gate.sh
    [ "$tier" -lt 3 ] && { [ $fail -eq 0 ] && grn "tier2 GREEN" || red "tier2 RED"; return $fail; }

    echo "== tier 3: bootstrap (ALONE -- never beside other gates) =="
    run "bootstrap"           bash compiler/self-host/test_bootstrap.sh
    if [ $fail -ne 0 ]; then
        ylw "  a bootstrap failure with rc=127 on a rotating module is CONTENTION."
        ylw "  rerun it alone before investigating; only a solo failure is real."
    fi
    [ $fail -eq 0 ] && grn "ALL GATES GREEN" || red "GATES RED"
    return $fail
}

# ---------------------------------------------------------------------------
# soak - a leak scales with iterations; a fixed shape does not. Always compare
# two scales. A single number proves nothing.
# ---------------------------------------------------------------------------
cmd_soak() {
    local f="${1:?usage: soak <file.kry> [mode-env] [small] [large]}"
    local envk="${2:-}" small="${3:-250000}" large="${4:-1000000}"
    local base; base=$(basename "$f" .kry)
    timeout 400 "$K" build --release "$f" -o "$TMP/$base.exe" >/dev/null 2>&1 || { red "build failed"; return 1; }
    for n in "$small" "$large"; do
        local peak
        peak=$(cd "$TMP" && ${envk:+env $envk} LEAK_ITERS=$n N=$n powershell -NoProfile -Command \
            "\$p=Start-Process -FilePath './$base.exe' -PassThru -NoNewWindow -RedirectStandardOutput 'soak.txt'; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 30}; [math]::Round(\$m/1MB,1)" 2>/dev/null | tr -d '\r')
        printf "  %-10s -> %s MB\n" "$n" "$peak"
    done
    ylw "  4x iterations with ~flat peak = fixed shape. Peak scaling with N = leak."
}

cmd_ledger() { sed -n '1,80p' "$ROOT/tools/loop/LEDGER.md"; }

case "${1:-}" in
    preflight) shift; cmd_preflight "$@";;
    repro)     shift; cmd_repro "$@";;
    diff)      shift; cmd_diff "$@";;
    gates)     shift; cmd_gates "$@";;
    soak)      shift; cmd_soak "$@";;
    ledger)    shift; cmd_ledger "$@";;
    *) sed -n '2,18p' "$0"; exit 2;;
esac
