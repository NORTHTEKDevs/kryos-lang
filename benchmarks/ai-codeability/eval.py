#!/usr/bin/env python3
"""Deterministic evaluator for the AI-codeability benchmark.

Scans solutions/<task>__<lang>__s<n>.<ext>, compiles+runs each, and compares
trimmed stdout to the task's expected output. Emits results.json and prints a
summary. The AI generation step is separate (a workflow); this step is fully
deterministic so the pass numbers are trustworthy, not self-reported.

Usage: python3 eval.py [solutions_dir]
"""
import json, os, subprocess, sys, tempfile, shutil, re

HERE = os.path.dirname(os.path.abspath(__file__))
SOL = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "solutions")
TASKS = {t["name"]: t for t in json.load(open(os.path.join(HERE, "tasks.json"), encoding="utf-8"))}
KRYOS = os.environ.get("KRYOS_BIN", "C:/Users/Krist/projects/active/kryos-lang/compiler/target/release/kryos.exe")
EXT = {"kryos": ".kry", "python": ".py", "rust": ".rs", "go": ".go"}
TIMEOUT = 60


def run(cmd, cwd=None, stdin_in=None):
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=TIMEOUT)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -9, "", "TIMEOUT"
    except Exception as e:
        return -1, "", f"{type(e).__name__}: {e}"


def strip_fences(code):
    # Be tolerant if a model wrapped output in markdown fences.
    m = re.search(r"```[a-zA-Z]*\n(.*?)```", code, re.DOTALL)
    return m.group(1) if m else code


def compile_run(lang, src_path):
    """Return (compile_ok, ran, stdout, err)."""
    work = tempfile.mkdtemp(prefix="aibench-")
    try:
        if lang == "python":
            rc, out, err = run(["python3", src_path])
            return True, rc == 0, out, err
        if lang == "kryos":
            rc, out, err = run([KRYOS, "run", src_path])
            # kryos run does compile+execute together; a compile error is rc!=0 with no stdout.
            compiled = not ("error" in err.lower() and out == "")
            return compiled, rc == 0, out, err
        if lang == "rust":
            exe = os.path.join(work, "a.exe")
            rc, out, err = run(["rustc", "-O", "--edition", "2021", src_path, "-o", exe])
            if rc != 0:
                return False, False, "", err
            rc2, out2, err2 = run([exe])
            return True, rc2 == 0, out2, err2
        if lang == "go":
            # go run needs the file to look like a main package file; copy into a temp dir
            dst = os.path.join(work, "main.go")
            shutil.copyfile(src_path, dst)
            rc, out, err = run(["go", "run", dst], cwd=work)
            compiled = "cannot" not in err and "syntax error" not in err and "undefined" not in err
            return compiled or rc == 0, rc == 0, out, err
        return False, False, "", f"unknown lang {lang}"
    finally:
        shutil.rmtree(work, ignore_errors=True)


def main():
    cells = []
    for fn in sorted(os.listdir(SOL)):
        base, ext = os.path.splitext(fn)
        parts = base.split("__")
        if len(parts) != 3:
            continue
        task, lang, sample = parts
        if task not in TASKS or lang not in EXT:
            continue
        path = os.path.join(SOL, fn)
        raw = open(path, encoding="utf-8", errors="replace").read()
        cleaned = strip_fences(raw)
        if cleaned != raw:
            open(path, "w", encoding="utf-8").write(cleaned)
        compile_ok, ran, out, err = compile_run(lang, path)
        expected = TASKS[task]["expected"]
        passed = ran and out.strip().replace("\r\n", "\n") == expected.strip().replace("\r\n", "\n")
        cells.append({
            "task": task, "lang": lang, "sample": sample,
            "compile_ok": compile_ok, "ran": ran, "passed": passed,
            "got": out.strip()[:200], "expected": expected.strip()[:200],
            "err": err.strip()[:300] if not passed else "",
        })

    # Aggregate per language.
    langs = sorted({c["lang"] for c in cells})
    tasks = sorted({c["task"] for c in cells})
    agg = {}
    for lang in langs:
        lc = [c for c in cells if c["lang"] == lang]
        n = len(lc)
        passed = sum(1 for c in lc if c["passed"])
        compiled = sum(1 for c in lc if c["compile_ok"])
        # pass-any per task (does >=1 sample pass).
        any_pass = 0
        for t in tasks:
            tc = [c for c in lc if c["task"] == t]
            if tc and any(c["passed"] for c in tc):
                any_pass += 1
        agg[lang] = {
            "samples": n,
            "pass_at_1_pct": round(100 * passed / n, 1) if n else 0,
            "compile_pct": round(100 * compiled / n, 1) if n else 0,
            "tasks_solved": any_pass,
            "tasks_total": len(tasks),
        }

    out = {"cells": cells, "aggregate": agg, "n_tasks": len(tasks)}
    json.dump(out, open(os.path.join(HERE, "results.json"), "w", encoding="utf-8"), indent=1)

    print(f"\n=== AI-codeability results ({len(tasks)} tasks, {len(cells)} samples) ===")
    print(f"{'lang':<8} {'pass@1':>8} {'compiles':>9} {'tasks_solved':>13}")
    for lang in langs:
        a = agg[lang]
        print(f"{lang:<8} {a['pass_at_1_pct']:>7}% {a['compile_pct']:>8}% {a['tasks_solved']:>6}/{a['tasks_total']}")
    print("\nPer-task pass-any (>=1 of the samples passed):")
    print(f"{'task':<20} " + " ".join(f"{l:<8}" for l in langs))
    for t in tasks:
        row = []
        for lang in langs:
            tc = [c for c in cells if c["lang"] == lang and c["task"] == t]
            mark = "ok" if (tc and any(c["passed"] for c in tc)) else ("." if tc else "-")
            row.append(f"{mark:<8}")
        print(f"{t:<20} " + " ".join(row))


if __name__ == "__main__":
    main()
