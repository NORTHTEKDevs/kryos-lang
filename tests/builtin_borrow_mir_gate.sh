#!/usr/bin/env bash
# builtin_borrow_mir_gate.sh -- structural pin for BORROWING_CALL_ARGS
# (kryos-mir/src/lower.rs). A runtime builtin that only READS its str / array
# arguments must leave the caller's scope-end drop of a fresh local argument in
# place; an unlisted builtin is a "consume" and the local is never dropped
# (one buffer leaked per call). For every builtin audited onto the list on
# 2026-11-01 this emits the MIR of a call with fresh str / array locals and
# requires a `drop(_N)` for each of them. Covers the network / pg / tls / dom
# builtins too, which the peak-RSS probe (tests/mem/agg_ops_leak.kry bb_*
# modes) cannot run. The leak itself is pinned by mem_agg_ops_gate.sh; the
# other direction (nothing the call returned was freed with the argument) by
# tests/conformance/conf_readonly_builtin_args.kry.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0; n=0

# name|statement(s); s1 s2 s3 are fresh computed strings, ks / vs fresh arrays,
# h a regex handle, b a byte buffer, nd a json node.
CASES='
index_of|let r = index_of(s1, s2)
file_read|let r = file_read(s1)
read_file|let r = read_file(s1)
write_file|let r = write_file(s1, s2)
append_file|let r = append_file(s1, s2)
file_size|let r = file_size(s1)
create_dir|let r = create_dir(s1)
buf_write_str|buf_write_str(b, s1)
buf_write_to_file|let r = buf_write_to_file(b, s1)
hmac_sha256|let r = hmac_sha256(s1, s2)
sha512|let r = sha512(s1)
sha1_hex|let r = sha1_hex(s1)
sha1_base64|let r = sha1_base64(s1)
hex_to_base64url|let r = hex_to_base64url(s1)
base64url_to_hex|let r = base64url_to_hex(s1)
ed25519_public|let r = ed25519_public(s1)
ed25519_sign|let r = ed25519_sign(s1, s2)
ed25519_verify|let r = ed25519_verify(s1, s2, s3)
pbkdf2_sha256|let r = pbkdf2_sha256(s1, s2, 1)
regex_new|let r = regex_new(s1)
regex_match|let r = regex_match(h, s1)
regex_find|let r = regex_find(h, s1)
regex_find_pos|let r = regex_find_pos(h, s1, 0)
regex_find_end|let r = regex_find_end(h, s1, 0)
regex_replace_all|let r = regex_replace_all(h, s1, s2)
json_parse|let r = json_parse(s1)
json_string|let r = json_string(s1)
json_get|let r = json_get(nd, s1)
json_object|let r = json_object(ks, vs)
json_array|let r = json_array(vs)
http_request|let r = http_request(s1, s2, s3, s1, 5)
https_get|let r = https_get(s1)
http2_get|let r = http2_get(s1)
http2_post|let r = http2_post(s1, s2)
http2_request|let r = http2_request(s1, s2, s3, s1)
fetch_text|let r = fetch_text(s1)
tcp_connect|let r = tcp_connect(s1, 80)
tcp_listen|let r = tcp_listen(s1, 80)
tls_server_config|let r = tls_server_config(s1, s2)
tls_send|let r = tls_send(1, s1)
uds_connect|let r = uds_connect(s1)
uds_bind|let r = uds_bind(s1)
uds_send|let r = uds_send(1, s1)
pg_connect|let r = pg_connect(s1)
pg_exec|let r = pg_exec(1, s1)
pg_query|let r = pg_query(1, s1)
ws_accept_key|let r = ws_accept_key(s1)
ws_encode_text|let r = ws_encode_text(s1)
ws_encode_binary|let r = ws_encode_binary(s1)
ws_encode_ping|let r = ws_encode_ping(s1)
ws_encode_pong|let r = ws_encode_pong(s1)
ws_unmask|let r = ws_unmask(s1, 0, 0, 0)
dom_set_text|dom_set_text(s1, s2)
dom_get_value|let r = dom_get_value(s1)
alert|alert(s1)
canvas_fill_rect|canvas_fill_rect(s1, 0, 0, 1, 1, s2)
canvas_clear|canvas_clear(s1)
'

while IFS='|' read -r name stmt; do
  [ -z "$name" ] && continue
  n=$((n+1))
  f="$TMP/$name.kry"
  cat > "$f" <<KRY
@capabilities(all)
fn main() {
    let s1 = "a" + "b"
    let s2 = "c" + "d"
    let s3 = "e" + "f"
    let ks = ["k1", "k2"]
    let vs = [1, 2]
    let h = regex_new("a")
    let b = buf_new(8)
    let nd = json_null()
    $stmt
}
KRY
  mir="$("$KRYOS" build --emit-mir "$f" 2>&1)"
  line="$(printf '%s\n' "$mir" | grep -E "call $name\(" | head -1)"
  if [ -z "$line" ]; then
    echo "  NO-CALL  $name (MIR has no call to it):"; printf '%s\n' "$mir" | head -5
    fail=$((fail+1)); continue
  fi
  args="$(printf '%s' "$line" | sed -E "s/.*call $name\((.*)\).*/\1/" | tr ',' '\n' | tr -d ' ')"
  bad=""
  for a in $args; do
    case "$a" in _[0-9]*) ;; *) continue ;; esac
    ty="$(printf '%s\n' "$mir" | sed -nE "s/^ *let $a: ([^;]*);.*/\1/p" | head -1)"
    case "$ty" in str|\[*) ;; *) continue ;; esac
    printf '%s\n' "$mir" | grep -qE "drop\($a\)" || bad="$bad $a($ty)"
  done
  if [ -n "$bad" ]; then
    echo "  NOT-DROPPED  $name: argument local(s)$bad have no drop (builtin is missing from BORROWING_CALL_ARGS)"
    fail=$((fail+1))
  fi
done <<< "$CASES"

if [ "$fail" -eq 0 ]; then
  echo "builtin-borrow-mir: PASS ($n builtins: every fresh str / array argument is dropped by the caller)"
else
  echo "builtin-borrow-mir: FAIL ($fail of $n)"
fi
exit $((fail>0))
