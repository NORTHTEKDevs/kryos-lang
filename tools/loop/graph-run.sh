#!/usr/bin/env bash
# graph-run.sh -- the Kryos completion loop runner.
#
# Reads tools/loop/GRAPH.md, runs acceptance commands, records what actually
# happened in tools/loop/STATE.json. See tools/loop/COMPLETION-LOOP-DESIGN.md.
#
#   status                    what is green / ready / blocked / stale
#   next                      the id of the next node to work on (or DONE)
#   verify <id>               run one node's acceptance, record the result
#   verify-all [--except id]  run every node in dependency order
#
# THE INVARIANT: a node is green only if its acceptance command exited 0 at the
# CURRENT HEAD. Nothing is green by assertion. If HEAD moved since a node was
# verified, that node is `stale` and must be re-verified. This exists because
# every documentation failure in this project's history was a status line that
# nobody re-ran.
set -u

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GRAPH="$ROOT/tools/loop/GRAPH.md"
STATE="$ROOT/tools/loop/STATE.json"
cd "$ROOT" || exit 1

HEAD_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"

c_grn=$'\033[32m'; c_red=$'\033[31m'; c_yel=$'\033[33m'; c_dim=$'\033[2m'; c_off=$'\033[0m'
[ -t 1 ] || { c_grn=""; c_red=""; c_yel=""; c_dim=""; c_off=""; }

# ---------------------------------------------------------------- graph parse
# Emits: <id>\t<deps csv>\t<accept cmd>
parse_graph() {
  awk '
    /^### / { if (id != "") print id "\t" deps "\t" accept; id=$2; deps="-"; accept=""; next }
    /^deps: / { deps=substr($0, 7); next }
    /^accept: / { accept=substr($0, 9); next }
    END { if (id != "") print id "\t" deps "\t" accept }
  ' "$GRAPH"
}

node_ids()  { parse_graph | cut -f1; }
node_deps() { parse_graph | awk -F'\t' -v n="$1" '$1==n {print $2}'; }
node_cmd()  { parse_graph | awk -F'\t' -v n="$1" '$1==n {print $3}'; }

# ---------------------------------------------------------------- state layer
# STATE.json is written ONLY here, wholesale, from observed runs. Hand-editing it
# is the one thing that defeats the system, so it is never partially patched.
state_get() { # <id> <field>
  [ -f "$STATE" ] || return 1
  python - "$STATE" "$1" "$2" <<'PY' 2>/dev/null
import json,sys
try: d=json.load(open(sys.argv[1]))
except Exception: sys.exit(1)
n=d.get("nodes",{}).get(sys.argv[2])
if not n or sys.argv[3] not in n: sys.exit(1)
print(n[sys.argv[3]])
PY
}

state_put() { # <id> <status> <rc> <commit>
  python - "$STATE" "$1" "$2" "$3" "$4" <<'PY'
import json,os,sys
p,nid,st,rc,commit=sys.argv[1:6]
d={"nodes":{}}
if os.path.exists(p):
    try: d=json.load(open(p))
    except Exception: d={"nodes":{}}
d.setdefault("nodes",{})[nid]={"status":st,"rc":int(rc),"commit":commit}
json.dump(d,open(p,"w"),indent=2,sort_keys=True)
open(p,"a").write("\n")
PY
}

# green only if recorded green AND recorded at the current HEAD
is_green() {
  local st sha
  st="$(state_get "$1" status 2>/dev/null)" || return 1
  sha="$(state_get "$1" commit 2>/dev/null)" || return 1
  [ "$st" = "green" ] && [ "$sha" = "$HEAD_SHA" ]
}

is_stale() {
  local st sha
  st="$(state_get "$1" status 2>/dev/null)" || return 1
  sha="$(state_get "$1" commit 2>/dev/null)" || return 1
  [ "$st" = "green" ] && [ "$sha" != "$HEAD_SHA" ]
}

deps_green() { # <id>
  local d
  d="$(node_deps "$1")"
  [ "$d" = "-" ] && return 0
  for dep in ${d//,/ }; do
    dep="$(echo "$dep" | tr -d ' ')"
    [ -z "$dep" ] && continue
    is_green "$dep" || return 1
  done
  return 0
}

# ------------------------------------------------------------------- commands
cmd_status() {
  printf '%s\n' "graph @ $HEAD_SHA"
  local total=0 green=0
  while IFS= read -r id; do
    [ -z "$id" ] && continue
    total=$((total+1))
    local label detail
    detail="$(node_deps "$id")"
    if is_green "$id"; then
      label="${c_grn}green${c_off}  "; green=$((green+1)); detail="verified at $HEAD_SHA"
    elif is_stale "$id"; then
      label="${c_yel}stale${c_off}  "; detail="was green at $(state_get "$id" commit); HEAD moved -- re-verify"
    elif deps_green "$id"; then
      label="${c_yel}ready${c_off}  "; detail="deps satisfied"
    else
      label="${c_red}blocked${c_off}"; detail="needs: $detail"
    fi
    printf '  %b %-20s %s%s%s\n' "$label" "$id" "$c_dim" "$detail" "$c_off"
  done < <(node_ids)
  printf '\n%s/%s nodes green at this commit\n' "$green" "$total"
  [ "$green" -eq "$total" ] && printf '%bDONE -- every node green at %s%b\n' "$c_grn" "$HEAD_SHA" "$c_off"
  return 0
}

cmd_next() {
  while IFS= read -r id; do
    [ -z "$id" ] && continue
    is_green "$id" && continue
    if deps_green "$id"; then echo "$id"; return 0; fi
  done < <(node_ids)
  echo "DONE"
}

cmd_verify() { # <id>
  local id="$1" cmd
  cmd="$(node_cmd "$id")"
  if [ -z "$cmd" ]; then
    echo "graph-run: no such node: $id" >&2
    return 2
  fi
  if ! deps_green "$id"; then
    printf '%bblocked%b %s -- dependencies not green: %s\n' "$c_red" "$c_off" "$id" "$(node_deps "$id")"
    state_put "$id" blocked 1 "$HEAD_SHA"
    return 1
  fi
  printf '%b>>%b %s\n%b   %s%b\n' "$c_dim" "$c_off" "$id" "$c_dim" "$cmd" "$c_off"
  set +e
  bash -c "cd '$ROOT' && $cmd"
  local rc=$?
  set -e
  if [ $rc -eq 0 ]; then
    state_put "$id" green 0 "$HEAD_SHA"
    printf '%bgreen%b   %s\n' "$c_grn" "$c_off" "$id"
  else
    state_put "$id" failed "$rc" "$HEAD_SHA"
    printf '%bFAILED%b  %s (exit %s)\n' "$c_red" "$c_off" "$id" "$rc"
  fi
  return $rc
}

cmd_verify_all() {
  local except="" fail=0
  [ "${1:-}" = "--except" ] && except="${2:-}"
  while IFS= read -r id; do
    [ -z "$id" ] && continue
    [ "$id" = "$except" ] && continue
    cmd_verify "$id" || fail=1
  done < <(node_ids)
  if [ $fail -eq 0 ]; then
    printf '\n%bverify-all GREEN at %s%b\n' "$c_grn" "$HEAD_SHA" "$c_off"
  else
    printf '\n%bverify-all RED at %s%b\n' "$c_red" "$c_off" "$HEAD_SHA"
  fi
  return $fail
}

case "${1:-status}" in
  status)     cmd_status ;;
  next)       cmd_next ;;
  verify)     shift; cmd_verify "${1:?usage: graph-run.sh verify <node>}" ;;
  verify-all) shift; cmd_verify_all "$@" ;;
  *) echo "usage: graph-run.sh [status|next|verify <id>|verify-all [--except <id>]]" >&2; exit 2 ;;
esac
