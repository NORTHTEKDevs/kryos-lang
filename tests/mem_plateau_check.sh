#!/usr/bin/env bash
# mem_plateau_check.sh -- regression gate for the memory model.
# Builds a heavy long-running churn program (scaled so the leak, if
# reintroduced, is unmissable) and asserts its peak resident memory stays
# under a hard ceiling. A 2026-07-04 soak of the pre-fix build grew to
# ~10.7 GB on this workload; the fixed build plateaus near 4 MB. The
# ceiling is deliberately generous (well above steady state, far below any
# real leak) so it flags reintroduced unbounded growth without flaking.
#
# Linux/macOS only (uses GNU/BSD `time -v`/`-l` for max-RSS). Skips with a
# clear message where neither is available (e.g. the Windows CI job, whose
# own memory story is covered by the local PowerShell soak harness).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"

CEIL_MB=250   # steady state ~4MB; pre-fix build blew past this in seconds

prog="$(mktemp --suffix=.kry 2>/dev/null || echo /tmp/mem_plateau.kry)"
cat > "$prog" <<'KRY'
// Helpers returning a heap value, consumed INLINE below (never bound to a
// local). That result used to be classified non-owning, so it was never
// dropped -- one leaked buffer per call, O(n) growth in any hot loop.
// Both helpers bind their result to a named local before returning, so this
// exercises the caller-side drop only.
fn mk_str(i: i64) -> str {
    let s = "row-" + to_string(i)
    return s
}

fn mk_arr(i: i64) -> [str] {
    let a = ["x", "y"]
    return a
}

// Returns an expression with an UNNAMED intermediate. Those temps had no
// drop path on the return route -- the statement-end cleanup runs after the
// return terminator, so its drops were dead code -- and leaked one buffer
// per call on top of the caller-side leak above.
fn mk_expr(i: i64) -> str {
    return "row-" + to_string(i)
}

// Takes a heap ARGUMENT and only reads it. The caller used to mark the
// argument consumed (assuming the callee took ownership) while the callee
// treated params as caller-owned and never dropped them, so nobody freed it:
// one leaked buffer per call, on both backends. Parameters are borrows now.
fn reads_arg(s: str, a: [str]) -> i64 {
    return len(s) + len(a)
}

struct Holder { name: str }

fn main() {
    let mut slots = {}
    slots["k"] = "seed"
    let mut cells: [str] = ["c0", "c1"]
    let mut rec = Holder { name: "seed" }
    let mut r = 0
    let mut total = 0
    while r < 40 {
        let mut i = 0
        let mut acc = 0
        while i < 200000 {
            let s = "payload-" + to_string(i) + "-" + to_string(r)
            acc = acc + len(s)
            let mut xs: [i64] = [i, i + 1]
            xs = push(xs, i * 2)
            acc = acc + xs[2]
            acc = acc + len(mk_str(i))
            acc = acc + len(mk_arr(i))
            acc = acc + len(mk_expr(i))
            let held = mk_arr(i)
            acc = acc + reads_arg(s, held)
            // Overwriting an occupied heap SLOT must release what it replaced:
            // map value, array element and struct field each leaked one buffer
            // per assignment. The element READ below also used to leak -- both
            // backends give the reader its own ownership (LLVM retains,
            // Cranelift clones a str) and nothing balanced it.
            slots["k"] = mk_str(i)
            cells[0] = mk_str(i)
            rec.name = mk_str(i)
            acc = acc + len(slots["k"]) + len(cells[0]) + len(rec.name)
            i = i + 1
        }
        total = total + acc
        r = r + 1
    }
    println(to_string(total))
}
KRY

bin="$(mktemp -u).bin"
"$KRYOS" build --release "$prog" -o "$bin" >/dev/null 2>&1 || { echo "mem-plateau: build failed"; exit 1; }

# Measure peak RSS. GNU time reports "Maximum resident set size (kbytes)";
# BSD/macOS `time -l` reports "maximum resident set size" in bytes.
peak_kb=""
if command -v /usr/bin/time >/dev/null 2>&1 && /usr/bin/time -v true >/dev/null 2>&1; then
  m=$(/usr/bin/time -v "$bin" 2>&1 >/dev/null | grep -i "Maximum resident" | grep -oE '[0-9]+' | tail -1)
  peak_kb="$m"
elif /usr/bin/time -l true >/dev/null 2>&1; then
  b=$(/usr/bin/time -l "$bin" 2>&1 >/dev/null | grep -i "maximum resident" | grep -oE '[0-9]+' | head -1)
  peak_kb=$(( b / 1024 ))
elif command -v powershell >/dev/null 2>&1; then
  # Windows: no `time -v`, so poll PeakWorkingSet64 while the process runs.
  # This path is not a nicety -- it is why this gate exists on the platform
  # most of this compiler is developed on. Without it the check SKIPped
  # locally and only spoke through CI, and a 614MB leak sat undiagnosed
  # because every local reading had to be hand-rolled. Windows peak working
  # set tracks Linux max RSS closely enough here to use the same ceiling
  # (measured 617.6MB against CI's 614MB on the leaking build, and 4.5MB
  # against CI's steady state after the fix).
  b=$(powershell -NoProfile -Command \
    "\$p=Start-Process -FilePath '$(cygpath -w "$bin" 2>/dev/null || echo "$bin")' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mp_out.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 40}; \$m" 2>/dev/null | tr -d '\r')
  case "$b" in ''|*[!0-9]*) b="" ;; esac
  [ -n "$b" ] && peak_kb=$(( b / 1024 ))
  [ -n "$peak_kb" ] || { echo "mem-plateau: SKIP (powershell RSS probe returned nothing)"; rm -f "$bin" "$prog"; exit 0; }
else
  echo "mem-plateau: SKIP (no GNU/BSD time -v/-l for RSS measurement)"
  rm -f "$bin" "$prog"; exit 0
fi
rm -f "$bin" "$prog"

[ -n "$peak_kb" ] || { echo "mem-plateau: could not read peak RSS"; exit 1; }
peak_mb=$(( peak_kb / 1024 ))
echo "mem-plateau: peak RSS ${peak_mb}MB (ceiling ${CEIL_MB}MB)"
if [ "$peak_mb" -gt "$CEIL_MB" ]; then
  echo "mem-plateau: FAIL -- churn workload exceeded ${CEIL_MB}MB; a memory leak was reintroduced"
  exit 1
fi
echo "mem-plateau: PASS"
