#!/usr/bin/env python3
"""Docs-examples gate: every self-contained ```kryos block must type-check.

Extracts fenced ```kryos code blocks from the manual (docs/learn/**, the
numbered chapters, QUICKSTART.md, repo CLAUDE.md). Blocks that contain a
`fn main(` are treated as complete programs and run through `kryos check`.
Fragments (no main) are wrapped in a `fn main() { ... }` shell when they
look like statement lists, else skipped. A block can opt out with a
`<!-- docs-example: skip -->` HTML comment on the line directly above the
fence (for pseudo-code, error-demos, and roadmap sketches).

Exit 0 only when every checked block passes. This exists because the
manual's examples ARE the contract -- three real compiler bugs were found
in one day by running examples the docs promised worked.
"""

import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
KRYOS = REPO / "compiler" / "target" / "release" / ("kryos.exe" if sys.platform == "win32" else "kryos")

DOC_GLOBS = [
    "docs/learn/*.md",
    "docs/learn/cookbook/*.md",
    "docs/learn/book/*.md",
    "docs/0*.md",
    "docs/1[0-9]-*.md",
    "QUICKSTART.md",
    "CLAUDE.md",
]

FENCE_RE = re.compile(r"```kryos[ \t]*\n(.*?)```", re.DOTALL)
SKIP_MARK = "docs-example: skip"

# Fragments containing these are declarations and safe to check as a module
# without a main() (kryos check accepts main-less modules for libraries).
DECL_HINT = re.compile(r"^\s*(fn|struct|enum|trait|impl|use|extern|const|type|@)", re.MULTILINE)


def blocks_of(path: Path):
    text = path.read_text(encoding="utf-8", errors="replace")
    out = []
    for m in FENCE_RE.finditer(text):
        start = m.start()
        # Look at the two lines above the fence for a skip marker.
        above = text[:start].rsplit("\n", 3)[-3:]
        if any(SKIP_MARK in ln for ln in above):
            continue
        line_no = text[:start].count("\n") + 1
        out.append((line_no, m.group(1)))
    return out


def classify_skip(code: str) -> str | None:
    """Classes of block that are documentation devices, not programs."""
    if "..." in code or "…" in code:
        return "placeholder"
    if re.search(r"//\s*(ERROR|error\[)", code):
        return "deliberate-error"
    # Mixed module decls + top-level statements is a teaching device that
    # has no compilable form; require full programs for those instead.
    has_decl = DECL_HINT.search(code) is not None
    if has_decl and "fn main(" not in code:
        for ln in code.splitlines():
            stripped = ln.strip()
            if not stripped or stripped.startswith("//"):
                continue
            if not re.match(r"(fn|struct|enum|trait|impl|use|extern|const|@|\}|/\*|\*)", stripped):
                return "mixed-decl-stmt"
    return None


def main() -> int:
    if not KRYOS.exists():
        print(f"docs-examples: kryos binary not found at {KRYOS}; build first", file=sys.stderr)
        return 2

    files = []
    for g in DOC_GLOBS:
        files.extend(sorted(REPO.glob(g)))

    total = passed = skipped = 0
    failures = []
    with tempfile.TemporaryDirectory() as td:
        for f in files:
            for line_no, code in blocks_of(f):
                total += 1
                reason = classify_skip(code)
                if reason is not None:
                    skipped += 1
                    total -= 1
                    continue
                src = code
                if "fn main(" not in src:
                    if DECL_HINT.search(src):
                        pass  # module-style fragment: check as-is
                    else:
                        # Statement-list fragment: wrap in main.
                        body = "\n".join("    " + ln for ln in src.splitlines())
                        src = "fn main() {\n" + body + "\n}\n"
                tmp = Path(td) / "ex.kry"
                tmp.write_text(src, encoding="utf-8")
                # Illustrative doc fragments are checked for TYPE correctness,
                # not capability hygiene, so pin permissive rather than the
                # compiler default (now inferred). A snippet showing `http_get`
                # should not have to carry an `@capabilities` annotation to make
                # its teaching point.
                r = subprocess.run(
                    [str(KRYOS), "check", "--capabilities-mode=permissive", str(tmp)],
                    capture_output=True,
                    text=True,
                    timeout=120,
                )
                if r.returncode == 0:
                    passed += 1
                else:
                    rel = f.relative_to(REPO)
                    failures.append((rel, line_no, (r.stdout + r.stderr).strip().splitlines()[:4]))

    print(f"docs-examples: {passed}/{total} kryos blocks type-check ({skipped} skipped)")
    for rel, line_no, err in failures:
        print(f"\nFAIL {rel}:{line_no}")
        for ln in err:
            print(f"  {ln}")
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
