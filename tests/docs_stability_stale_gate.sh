#!/usr/bin/env bash
# Pin: STABILITY.md / LAUNCH-READINESS.md must not carry stale claims
# (closed items 3/51 as live leaks, "fmt drops comments") and must name 137/138/139.
# Usage: tests/docs_stability_stale_gate.sh [repo-root]
root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
python - "$root" <<'PY'
import re, sys
root = sys.argv[1]
rd = lambda p: open(f"{root}/{p}", encoding="utf-8").read()
S, L = rd("STABILITY.md"), rd("docs/LAUNCH-READINESS.md")
bad = []
for name, text, rx, want in [
    ("STABILITY", S, r"items 3, 51", False),
    ("STABILITY", S, r"(?m)^\|[^\n]*\|\s*(3|51)\s*\|\s*$", False),
    ("STABILITY", S, r"drops its ordinary comments|does not preserve non-doc comments", False),
    ("STABILITY", S, r"\b137\b", True),
    ("STABILITY", S, r"\b138\b", True),
    ("STABILITY", S, r"\b139\b", True),
    ("LAUNCH", L, r"Item 3 \(struct-argument leak[^\n]*\n?[^\n]*still open", False),
    ("LAUNCH", L, r"Item 3 - struct-argument leak, ~86MB per 1M calls", False),
    ("LAUNCH", L, r"`kryos fmt` drops", False),
    ("STABILITY", S, r"tuple, closure or trait-object payload", False),
    ("STABILITY", S, r"Heap ownership edge case adjacent", False),
    ("STABILITY", S, r"(?i)memory.safety", True),
    ("LAUNCH", L, r"LEDGER items 3, 51 and 41", False),
    ("LAUNCH", L, r"\*\*Item 3\*\* -- struct-argument leak, ~86MB per 1M calls", False),
    ("LAUNCH", L, r"DESIGN NOTE, NOT\s+FIXED - 8 distinct fix attempts", False),
    ("book04", rd("docs/learn/book/04-functions.md"), r"an open design note\s+in this compiler|LEDGER item 3\), not a bug in your code", False),
    ("book10", rd("docs/learn/book/10-ownership-and-arc.md"), r"item 3, open\)|item 3 -- eight", False),
    ("bookREADME", rd("docs/learn/book/README.md"), r"open design\s+note, no fix shipped", False),
    ("capsound", rd("docs/capability-soundness.md"), r"LEDGER OPEN item 3", False),
    ("RESUME", rd("docs/RESUME-PLAN.md"), r"call-boundary leak \(~86MB/1M\s+calls, LEDGER item 3\)", False),
]:
    if bool(re.search(rx, text)) != want:
        bad.append(f"{name}: {'missing' if want else 'stale'} {rx}")
print("\n".join("FAIL " + b for b in bad) or "PASS docs_stability_stale_gate")
sys.exit(1 if bad else 0)
PY
