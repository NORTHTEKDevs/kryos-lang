#!/usr/bin/env bash
# check-threat-model.sh -- the capability claim must be qualified by a written
# threat model.
#
# Acceptance for graph node `threat-model`.
#
# WHY: "capability-safe" was the headline claim while twelve reproducible
# bypasses were open. The claim was not dishonest so much as unqualified -- there
# was no document saying what capabilities do and do not guarantee, so there was
# nothing for the implementation to be measured against. A language that gates
# authority at compile time needs to state its threat model the way every
# serious security-relevant project does.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT" || exit 1
TM="docs/THREAT-MODEL.md"
fail=0

if [ -f "$TM" ]; then echo "  ok   $TM exists"; else
    echo "  FAIL: $TM does not exist"; fail=1; fi

if [ -f "$TM" ]; then
    for section in "What capabilities DO guarantee" "What capabilities do NOT guarantee" "Known limitations"; do
        if grep -qiF "$section" "$TM"; then echo "  ok   section: $section"
        else echo "  FAIL: $TM is missing the section: $section"; fail=1; fi
    done
fi

if grep -q "THREAT-MODEL.md" README.md; then echo "  ok   README links the threat model"
else echo "  FAIL: README does not link $TM"; fail=1; fi

# The unqualified claim must not reappear. "capability-safe" is only acceptable
# next to a qualifier or a link to the threat model, never bare in a headline.
if grep -nE "^\*\*Kryos is a .*capability-safe" README.md >/dev/null 2>&1; then
    echo "  FAIL: README headline makes an unqualified capability-safe claim"; fail=1
else
    echo "  ok   no unqualified capability-safe headline"
fi

[ $fail -eq 0 ] && echo "threat-model: PASS" || echo "threat-model: FAIL"
exit $fail
