#!/usr/bin/env bash
# fixtures_tracked_gate.sh -- every fixture a gate references must be COMMITTED.
#
# WHY: on 2026-08-19 the first-ever inspection of CI found `security_gate.sh`
# FAILING on the Windows runner while passing locally, with:
#     error: `tests/security/attack_verify_actor_to_actor_message.kry`
#            is not a file or directory
# The file existed on the developer's disk and had never been `git add`ed. It is
# the repro for the LAST capability escape -- an entire session was spent closing
# that bug, and its regression pin was not in the repository. 171 files under
# tests/security/ were untracked; 8 of them were referenced by live gates.
#
# The failure mode is nastier than a missing file: locally the gate PASSES and
# reports the escape as rejected, so the working tree says "0 escapes" while a
# clean checkout cannot even run the check. Every green result this repo
# produced for those 8 fixtures was, on a fresh clone, vacuous.
#
# This gate closes the loop: parse every gate script for tests/** fixture paths
# and assert each one is tracked by git. Cheap, and it fails loudly the moment
# someone writes a fixture and forgets to commit it.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
command -v git >/dev/null 2>&1 || { echo "fixtures-tracked: SKIP (no git)"; exit 0; }
command -v python3 >/dev/null 2>&1 || { echo "fixtures-tracked: SKIP (no python3)"; exit 0; }

python3 - <<'PYEOF'
import os, re, subprocess, sys

# Every path of the form tests/<...>.<kry|lisp|sh|csv|log> mentioned in any
# gate script or the escape-status driver.
scripts = []
for root, _, files in os.walk('tests'):
    for f in files:
        if f.endswith('.sh'):
            scripts.append(os.path.join(root, f))
for extra in ('tools/loop/escape_status.sh', 'tools/loop/check-docs-truth.sh'):
    if os.path.exists(extra):
        scripts.append(extra)

# Must match a path that STARTS at tests/, not one embedded in a longer path:
# ecosystem_check.sh lists 'ecosystem/kryos-secrets/tests/fixtures/leak_compute.kry',
# and a naive match pulled the tests/... suffix out of the middle of it
# and reported a real, tracked file as absent. Require a non-path char before it.
pat = re.compile(r'(?<![A-Za-z0-9_./-])(tests/[A-Za-z0-9_./-]+\.(?:kry|lisp|csv|log))')
refs = set()
for s in scripts:
    try:
        txt = open(s, encoding='utf-8', errors='replace').read()
    except OSError:
        continue
    for m in pat.findall(txt):
        refs.add(m.replace('\\', '/'))

# escape_status.sh names its corpus as bare stems in a table; resolve those too.
esc = 'tools/loop/escape_status.sh'
if os.path.exists(esc):
    txt = open(esc, encoding='utf-8', errors='replace').read()
    for stem in re.findall(r'^\s*[0-9]+[a-z]?\|([A-Za-z0-9_]+)\s*$', txt, re.M):
        refs.add('tests/security/%s.kry' % stem)

if not refs:
    print('fixtures-tracked: SKIP (no fixture references found)')
    sys.exit(0)

tracked = set(subprocess.run(['git', 'ls-files'], capture_output=True, text=True,
                             check=False).stdout.split('\n'))

missing_untracked, missing_absent = [], []
for r in sorted(refs):
    if r in tracked:
        continue
    (missing_untracked if os.path.exists(r) else missing_absent).append(r)

print('  fixtures referenced by gates : %d' % len(refs))
print('  tracked                      : %d' % (len(refs) - len(missing_untracked) - len(missing_absent)))

if not missing_untracked and not missing_absent:
    print('fixtures-tracked: PASS -- every gate fixture is committed')
    sys.exit(0)

for r in missing_untracked:
    print('  UNTRACKED  %s  (exists locally, MISSING from a clean checkout)' % r)
for r in missing_absent:
    print('  ABSENT     %s  (referenced but does not exist at all)' % r)
print('fixtures-tracked: FAIL -- these gates pass locally and cannot run in CI')
sys.exit(1)
PYEOF
