#!/usr/bin/env python3
"""One pass of tools/loop/verdict_diff.sh: the verdict of every program under one binary.

usage: verdict_pass.py <kryos-binary> <list-file> <out.tsv> [jobs]

For every program named in <list-file> (one repository-relative path per line) this
runs `kryos check <file>` and `kryos check --strict-capabilities <file>` from the
current directory and writes one TSV row per (program, mode):

    <program> TAB <mode: default | --strict-capabilities> TAB <exit code> TAB <first error code or ->

The rows are exactly what the shell loop in verdict_diff.sh writes (it is the
fallback when python3 is missing), sorted bytewise. This exists because the shell
loop starts a `bash -c` per program, and on Windows that costs about a second each
(a pass over the 1343-program corpus took half an hour against under a minute
here); the output was compared row for row against the shell loop's before this
helper was trusted.
"""
import concurrent.futures as cf
import os
import re
import subprocess
import sys


def main():
    if len(sys.argv) < 4:
        sys.stderr.write(__doc__)
        return 2
    binary, list_file, out = sys.argv[1], sys.argv[2], sys.argv[3]
    jobs = int(sys.argv[4]) if len(sys.argv) > 4 else 8
    with open(list_file, encoding="utf-8") as fh:
        files = [line.rstrip("\r\n") for line in fh if line.strip()]
    code_re = re.compile(rb"E[0-9]{4}")

    def one(job):
        path, mode = job
        cmd = [binary, "check"] + ([mode] if mode else []) + [path]
        try:
            proc = subprocess.run(cmd, capture_output=True, timeout=120)
            rc = proc.returncode
            match = code_re.search(proc.stdout + proc.stderr)
            code = match.group(0).decode() if match else "-"
        except subprocess.TimeoutExpired:
            rc, code = 124, "-"
        return (path, mode or "default", rc, code)

    work = [(f, m) for m in ("", "--strict-capabilities") for f in files]
    with cf.ThreadPoolExecutor(max_workers=jobs) as pool:
        rows = list(pool.map(one, work))
    rows.sort(key=lambda r: (r[0].encode(), r[1].encode()))
    with open(out, "w", newline="\n", encoding="utf-8") as fh:
        for path, mode, rc, code in rows:
            fh.write("%s\t%s\t%d\t%s\n" % (path, mode, rc, code))
    return 0


if __name__ == "__main__":
    sys.exit(main())
