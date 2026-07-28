#!/usr/bin/env bash
# End-to-end examples gate — RUNS example programs and checks what they produce.
#
# Why this exists (it is not a duplicate of run_examples_gate.sh):
# that gate type-checks and AOT-COMPILES every example but never RUNS one, and
# never compares the two backends against each other. Three real defects shipped
# through it because "it compiles" was the whole bar:
#
#   * json_bool(false) built a JSON `true` on --release, so rest_api served
#     "done":true for every todo (the JIT was correct -- a backend divergence
#     visible only in a served response body).
#   * calling a struct-returning function through a fn value returned garbage
#     on --release, the exact shape a router uses to store handlers.
#   * tiny_kv self-recursed on its first command and died with a stack
#     overflow -- a shipped app that never worked at all.
#
# Layers:
#   1. Differential stdout: run compute-heavy showcase apps on BOTH backends and
#      require byte-identical output and exit status.
#   2. Stdin-driven REPLs: feed a fixed script, diff both backends.
#   3. Servers: start them and assert on real response BODIES over the socket --
#      the only layer that can see a corrupt serialized payload.
#
# Deliberately excluded from layer 1: programs whose output embeds a clock, a
# random value, or live network results (http2_demo, websocket_client,
# agent_runtime, http_client, agent, mcp_server) -- they are not differentially
# comparable. Exclusions are LISTED at the end of the run, never silent.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KRYOS="$REPO/compiler/target/release/kryos"
[[ "${OS:-}" == "Windows_NT" || "$(uname -s)" =~ MINGW|MSYS|CYGWIN ]] && KRYOS="$KRYOS.exe"
[[ -x "$KRYOS" ]] || { echo "examples-e2e: build compiler first ($KRYOS missing)"; exit 2; }

if ! command -v clang >/dev/null 2>&1; then
    for c in "/c/Program Files/LLVM/bin" "/c/Program Files (x86)/LLVM/bin"; do
        [[ -x "$c/clang.exe" || -x "$c/clang" ]] && export PATH="$c:$PATH" && break
    done
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"; kill_servers' EXIT
fail=0
skipped=()

kill_servers() {
    for n in rest_api_e2e web_server_e2e; do
        if [[ "${OS:-}" == "Windows_NT" ]]; then
            taskkill //F //IM "$n.exe" //T >/dev/null 2>&1
        else
            pkill -f "$n" >/dev/null 2>&1
        fi
    done
    return 0
}

# build <src> <out>  -> 0 on success
build_aot() {
    "$KRYOS" build --release "$1" -o "$2" >"$TMP/build.log" 2>&1
}

# ---------------------------------------------------------------------------
# Layer 1: differential stdout (JIT vs AOT), deterministic programs only.
# ---------------------------------------------------------------------------
echo "examples-e2e: layer 1 — differential stdout (both backends)"
DIFFERENTIAL=(
    bytecode_vm parser markdown stats_pipeline
    budget_analyst kdoc dir_walker cli_tool kvdb ssg
)
l1_ok=0
for name in "${DIFFERENTIAL[@]}"; do
    src="$REPO/examples/showcase/$name.kry"
    [[ -f "$src" ]] || { skipped+=("$name (no such example)"); continue; }

    jdir="$TMP/$name-jit"; adir="$TMP/$name-aot"
    mkdir -p "$jdir" "$adir"

    ( cd "$jdir" && timeout 90 "$KRYOS" run "$src" >out.txt 2>&1 ); jrc=$?
    if ! build_aot "$src" "$adir/$name.exe"; then
        echo "  BUILD FAIL $name"; sed -n '1,5p' "$TMP/build.log"; fail=1; continue
    fi
    ( cd "$adir" && timeout 90 "./$name.exe" >out.txt 2>&1 ); arc=$?

    if [[ "$jrc" != "$arc" ]]; then
        echo "  DIVERGE $name: exit status jit=$jrc aot=$arc"; fail=1
    elif ! diff -q "$jdir/out.txt" "$adir/out.txt" >/dev/null 2>&1; then
        echo "  DIVERGE $name: stdout differs between backends"
        diff "$jdir/out.txt" "$adir/out.txt" | head -12 | sed 's/^/    /'
        fail=1
    else
        l1_ok=$((l1_ok+1))
    fi
done
echo "  layer 1: $l1_ok/${#DIFFERENTIAL[@]} byte-identical across backends"

# ---------------------------------------------------------------------------
# Layer 2: stdin-driven REPLs, differential.
# ---------------------------------------------------------------------------
echo "examples-e2e: layer 2 — stdin-driven REPLs (both backends)"
printf '3 4 +\n10 2 /\n5 1 2 + 4 * + 3 -\nquit\n'                       >"$TMP/rpn.in"
printf 'set a 1\nset b hello\nset c 3\nlist\ndel a\nlist\nget a\nquit\n' >"$TMP/kv.in"
l2_ok=0; l2_n=0
for pair in "rpn_calc:$TMP/rpn.in" "tiny_kv:$TMP/kv.in"; do
    name="${pair%%:*}"; input="${pair#*:}"
    src="$REPO/examples/showcase/$name.kry"
    [[ -f "$src" ]] || { skipped+=("$name (no such example)"); continue; }
    l2_n=$((l2_n+1))

    timeout 90 "$KRYOS" run "$src" <"$input" >"$TMP/$name.jit" 2>&1; jrc=$?
    if ! build_aot "$src" "$TMP/$name.exe"; then
        echo "  BUILD FAIL $name"; fail=1; continue
    fi
    ( cd "$TMP" && timeout 90 "./$name.exe" <"$input" >"$name.aot" 2>&1 ); arc=$?

    # A REPL that dies on its first command (tiny_kv's read_line self-recursion)
    # exits non-zero on BOTH backends, so a pure diff would call it "identical".
    # Require success too.
    if [[ "$jrc" != 0 || "$arc" != 0 ]]; then
        echo "  RUN FAIL $name: jit=$jrc aot=$arc"; sed -n '1,6p' "$TMP/$name.jit" | sed 's/^/    /'; fail=1
    elif ! diff -q "$TMP/$name.jit" "$TMP/$name.aot" >/dev/null 2>&1; then
        echo "  DIVERGE $name: stdout differs between backends"
        diff "$TMP/$name.jit" "$TMP/$name.aot" | head -12 | sed 's/^/    /'; fail=1
    else
        l2_ok=$((l2_ok+1))
    fi
done
echo "  layer 2: $l2_ok/$l2_n identical across backends"

# ---------------------------------------------------------------------------
# Layer 3: servers — assert on real response bodies.
# ---------------------------------------------------------------------------
echo "examples-e2e: layer 3 — servers (response bodies over a socket)"
if ! command -v curl >/dev/null 2>&1; then
    skipped+=("layer 3 servers (curl not installed)")
    echo "  SKIP: curl not available"
else
    kill_servers

    l3_ok=0; l3_n=0
    # want_body <label> <url> <substring> [curl-args...]
    want_body() {
        local label="$1" url="$2" want="$3"; shift 3
        l3_n=$((l3_n+1))
        local got; got="$(curl -s --max-time 5 "$@" "$url" 2>/dev/null)"
        if [[ "$got" == *"$want"* ]]; then l3_ok=$((l3_ok+1)); echo "  ok   $label"; return 0; fi
        echo "  FAIL $label: expected to contain '$want'"
        echo "       got: $(printf '%s' "$got" | head -c 200)"
        fail=1; return 1
    }

    for backend in aot jit; do
        # ---- rest_api: the app whose served JSON was silently corrupt ----
        src="$REPO/examples/showcase/rest_api.kry"
        if [[ -f "$src" ]]; then
            if [[ "$backend" == aot ]]; then
                if build_aot "$src" "$TMP/rest_api_e2e.exe"; then
                    ( cd "$TMP" && ./rest_api_e2e.exe >rest.log 2>&1 & )
                else
                    echo "  BUILD FAIL rest_api"; fail=1
                fi
            else
                ( timeout 300 "$KRYOS" run "$src" >"$TMP/rest_jit.log" 2>&1 & )
            fi
            # Wait for the port. `kryos run` COMPILES before it serves (~20s),
            # so this poll must outlast the compile -- a 90s process cap used to
            # kill the JIT server midway through the assertions, which showed up
            # as intermittent failures on the later checks rather than the first.
            for _ in $(seq 1 60); do
                curl -s --max-time 2 http://127.0.0.1:7878/todos >/dev/null 2>&1 && break
                sleep 1
            done

            if curl -s --max-time 3 http://127.0.0.1:7878/todos >/dev/null 2>&1; then
                # `done` MUST be false: the store pushes 0. This is the exact
                # assertion the json_bool i1/i64 ABI bug violated on --release.
                want_body "rest_api[$backend] GET /todos"    "http://127.0.0.1:7878/todos" '"done":false'
                want_body "rest_api[$backend] GET /todos/1"  "http://127.0.0.1:7878/todos/1" '"id":1'
                curl -s --max-time 5 -X POST http://127.0.0.1:7878/todos -d '{"title":"e2e"}' >/dev/null 2>&1
                want_body "rest_api[$backend] POST persists" "http://127.0.0.1:7878/todos" '"id":3'
                want_body "rest_api[$backend] 404"           "http://127.0.0.1:7878/todos/99" 'no such todo'
            else
                skipped+=("rest_api[$backend] (port 7878 unavailable)")
                echo "  SKIP rest_api[$backend]: could not reach 127.0.0.1:7878"
            fi
            kill_servers; sleep 1
        fi

        # ---- web_server: serves only 3 requests, so assert within that budget ----
        src="$REPO/examples/showcase/web_server.kry"
        if [[ -f "$src" ]]; then
            if [[ "$backend" == aot ]]; then
                if build_aot "$src" "$TMP/web_server_e2e.exe"; then
                    ( cd "$TMP" && ./web_server_e2e.exe >web.log 2>&1 & )
                else
                    echo "  BUILD FAIL web_server"; fail=1
                fi
            else
                ( timeout 300 "$KRYOS" run "$src" >"$TMP/web_jit.log" 2>&1 & )
            fi
            for _ in $(seq 1 60); do
                curl -s --max-time 2 http://127.0.0.1:8080/hello >/dev/null 2>&1 && break
                sleep 1
            done
            # /hello consumed one of the 3; two remain.
            if [[ -s "$TMP/web.log" || -s "$TMP/web_jit.log" ]] || curl -s --max-time 3 http://127.0.0.1:8080/json >/dev/null 2>&1; then
                want_body "web_server[$backend] /json" "http://127.0.0.1:8080/json" '"ok":true'
                want_body "web_server[$backend] /count" "http://127.0.0.1:8080/count" 'request #'
            else
                skipped+=("web_server[$backend] (port 8080 unavailable)")
                echo "  SKIP web_server[$backend]: could not reach 127.0.0.1:8080"
            fi
            kill_servers; sleep 1
        fi
    done
    echo "  layer 3: $l3_ok/$l3_n response-body assertions passed"
    # A gate that asserts nothing must not report success.
    if (( l3_n == 0 )); then
        echo "  FAIL: layer 3 ran no assertions at all (servers never reachable)"
        fail=1
    fi
fi

# ---------------------------------------------------------------------------
if (( ${#skipped[@]} > 0 )); then
    echo "examples-e2e: skipped (${#skipped[@]}):"
    for s in "${skipped[@]}"; do echo "  - $s"; done
fi

if (( fail )); then
    echo "examples-e2e: FAIL"
    exit 1
fi
echo "examples-e2e: PASS"
