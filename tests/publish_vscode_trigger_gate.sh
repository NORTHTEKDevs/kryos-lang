#!/usr/bin/env bash
# A23 gate: publish-vscode.yml must not carry a `release:` trigger. release.yml cuts the
# release with the default GITHUB_TOKEN, and GitHub starts no workflow from an event
# that token raises, so that trigger can never fire. Also: no claim that it fires
# automatically, no v1.0.0 default tag, and the tag must come from the dispatch input.
# Usage: bash tests/publish_vscode_trigger_gate.sh [tree-root]   (default: this tree)
set -uo pipefail
ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
W="$ROOT/.github/workflows/publish-vscode.yml"
R="$ROOT/.github/workflows/release.yml"
fail=0
bad() { echo "  FAIL: $1"; fail=1; }
[ -f "$W" ] || { echo "  FAIL: $W missing"; exit 1; }
if grep -Eq '^[[:space:]]*release:' "$W"; then
    # allowed only if release.yml passes a token: within 12 lines of action-gh-release, or publishes the extension itself
    if ! { grep -Pzoq 'action-gh-release@[^\n]*\n(?:[^\n]*\n){0,12}?\s*token:\s*\S' "$R" 2>/dev/null || grep -Eq 'publish-vscode|vsce publish' "$R" 2>/dev/null; }; then
        bad "release: trigger present but GITHUB_TOKEN-raised release events start no workflow"
    fi
fi
grep -qi 'fires automatically when a release is published' "$W" && bad "stale 'fires automatically' header"
grep -q 'v1\.0\.0' "$W" && bad "v1.0.0 default tag"
grep -q 'event\.release\.tag_name' "$W" && ! grep -Eq '^[[:space:]]*release:' "$W" && bad "tag resolves from a release event that cannot occur"
[ "$fail" -eq 0 ] && echo "  ok   publish-vscode.yml: no dead release trigger"
exit "$fail"
