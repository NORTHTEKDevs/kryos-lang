# kryos-microctl

A capability-scoped control-plane CLI + daemon **scaffold**. It is a small
`kubectl`-shaped tool -- `status`, `get`, `apply` subcommands plus a `serve`
daemon exposing a few HTTP control routes -- whose point is not breadth but
**least privilege by construction**:

> In a control plane, "this subcommand can only read config, that one hits the
> API, only `apply` writes the filesystem" is exactly the property that bounds
> blast radius. In Go that is a code-review convention. In Kryos it is a
> **compiler-checked per-subcommand `@capabilities` contract** you can read off
> `kryos manifest --caps`.

This is an authority claim, not a scale claim. The server is a deliberately
blocking, one-request-per-connection accept loop -- the right shape for a
low-traffic control plane (high-concurrency serving is explicitly out of scope).

Built on two merged ecosystem packages:

- **kryos-cli-args** (project 14) -- the declarative CLI front-end.
- **kryos-policy** (project 20) -- the manifest capability-contract linter that
  is the CI gate and the model for the daemon's startup self-audit.

## The headline: a per-subcommand capability map

Each operator verb carries the minimal `@capabilities` the compiler will accept,
and the compiler enforces it. Drop `net` from `cmd_status` and the direct call
to the `net`-annotated `std::http::get` fails to type-check. Here is the real
`kryos manifest --caps --format pretty src` output (committed as `caps.manifest`):

```
src/client.kry:
fn cmd_apply: [io]      <- the ONLY client verb that touches the filesystem
fn cmd_get: [net]       <- net only; physically cannot read your disk
fn cmd_status: [net]    <- net only
src/daemon.kry:
fn serve: [net]
src/handlers.kry:
fn handle_healthz: []   <- request handlers are pure compute: touch nothing
fn handle_status: []
fn handle_config: []
fn dispatch: []
src/selfcheck.kry:
fn assert_within_policy: [io]
src/main.kry:
fn main: [io, net]
```

`status` and `get` can never write a file; `apply` can never open a socket. The
audit question "which subcommand can touch what" is answered by the compiler,
not by a comment that rots.

## Subcommands

```
kryos-microctl status [--addr URL]        # GET <daemon>/status            (net)
kryos-microctl get <key> [--addr URL]     # GET <daemon>/config?key=<key>  (net)
kryos-microctl apply <file> [--out PATH]  # render desired-state to disk   (io)
kryos-microctl serve [--port N]           # run the control endpoint       (net)
```

## The control endpoint

`serve` runs a blocking accept loop over the TCP builtins, using `std::http`'s
real HTTP/1.1 request parser and response serializer (`parse_request`,
`serialize_response`, `text_response`, `json_response`) and dispatching by path.
Three GET routes:

| Route                | Response |
| -------------------- | -------- |
| `/healthz`           | `ok` (liveness) |
| `/status`            | `{"status":"running","name":"kryos-microctl","version":"0.1.0"}` |
| `/config?key=<k>`    | one config value, or the whole map when `key` is absent |

### Verified, end to end

Daemon (via `kryos run`, see "Known limitations") + `curl` + the AOT client:

```
$ curl -s http://127.0.0.1:8131/healthz
ok
$ curl -s http://127.0.0.1:8131/status
{"status":"running","name":"kryos-microctl","version":"0.1.0"}
$ curl -s 'http://127.0.0.1:8131/config?key=replicas'
{"replicas":"3"}

$ ./microctl.exe status --addr http://127.0.0.1:8131
status 200: {"status":"running","name":"kryos-microctl","version":"0.1.0"}
$ ./microctl.exe get image --addr http://127.0.0.1:8131
{"image":"kryos-microctl:0.1.0"}

$ ./microctl.exe apply desired.yaml --out applied.conf
apply: wrote 61 bytes to applied.conf
```

## Daemon-start self-audit ("assert actual subset of declared")

Before binding the socket, `serve` runs `assert_within_policy()`: it reads the
compiler-computed `caps.manifest` and the declared `[capabilities] allowed` from
`kryos.toml`, and refuses to start if the actual surface is **not** a subset of
the declared allowlist. This is the same subset contract kryos-policy enforces
in CI, executed once more at runtime as a last line of defense.

```
$ ./microctl serve --port 8080
self-check: surface ["io", "net"] is within declared ["io", "net"]
kryos-microctl daemon listening on 0.0.0.0:8080
routes: GET /healthz   GET /status   GET /config?key=<k>
```

## The policy gate (kryos-policy, wired into CI)

The whole point only holds if a subcommand that *quietly grows* a capability is
caught. `demo_microctl.kry` runs the kryos-policy contract check over two
packages using their real compiler output:

```
$ kryos run ecosystem/kryos-microctl/demo_microctl.kry

--- kryos-microctl (the scaffold) ---
  declared allowlist: ["io", "net"]
  computed surface:   ["io", "net"]
  verdict: OK: declared allowlist exactly matches the computed surface
  GATE: pass

--- over_privileged fixture ---
  declared allowlist: ["net"]
  computed surface:   ["io", "net"]
  verdict: VIOLATION: code uses capabilities not in the allowlist: io
  GATE: FAIL (CI exits non-zero)

RESULT: as expected -- the scaffold is within policy; the
        over-privileged handler is caught by the gate.
```

`fixtures/over_privileged/` is an honest, compiling package whose `kryos.toml`
declares `allowed = ["net"]` but whose handler also calls `file_write` (`io`).
The gate catches the undeclared `io` -- a contract violation no other package
manager can verify, because none computes a real capability set from source.

### CI snippet

```bash
cd ecosystem/kryos-microctl

# 1. (Re)generate the compiler-computed capability surface.
kryos manifest --caps --format pretty src -o caps.manifest

# 2. Fail the build if the surface drifted from what is committed/reviewed.
kryos manifest --caps --format pretty src -o /tmp/fresh.manifest
diff caps.manifest /tmp/fresh.manifest   # non-zero exit on drift

# 3. Hard-deny capability escalation (a control plane must never gain `process`).
kryos manifest --caps src --deny process # exit 1 if any fn gains `process`

# 4. Run the subset contract gate over this package + the fixture.
kryos run demo_microctl.kry
```

(Step 3 demonstrated: `kryos manifest --caps fixtures/over_privileged/src --deny io`
exits 1 with `denied capability 'io' found in: exfiltrate`.)

## Build, test, run

```bash
# From the repository root:
kryos test --path ecosystem/kryos-microctl          # 10 file tests + 8 @test, all green
kryos build --release ecosystem/kryos-microctl/src/main.kry -o microctl.exe
kryos run ecosystem/kryos-microctl/demo_microctl.kry # policy-gate demo

# From the project root (so the self-check finds kryos.toml + caps.manifest):
cd ecosystem/kryos-microctl
kryos run src/main.kry -- serve --port 8080          # start the daemon
kryos run src/main.kry -- --help
./microctl.exe status --addr http://127.0.0.1:8080   # AOT client subcommands
```

Test output:

```
running 10 file tests ... Tests: 10 passed, 0 failed
running 8 @test functions ... Tests: 8 passed, 0 failed
```

## Layout

```
kryos.toml            package manifest; [capabilities] allowed = ["io", "net"]
                      + path deps on kryos-cli-args and kryos-policy
caps.manifest         committed compiler-computed capability surface (the contract)
.kryos/deps/*.redirect  path-dependency redirects (relative, portable)
src/
  cli.kry             the CLI surface as data, via kryos-cli-args (pure compute)
  handlers.kry        route handlers + path dispatch (pure compute)
  daemon.kry          serve(): the blocking accept loop (net)
  client.kry          cmd_status/cmd_get (net), cmd_apply (io) -- the witnesses
  selfcheck.kry       assert_within_policy(): startup subset audit (io)
  main.kry            argv parse + dispatch
demo_microctl.kry     the policy-contract gate over the scaffold + fixture
fixtures/
  over_privileged/    declares ["net"], uses io too -- the gate must catch it
tests/
  test_cli.kry        the CLI surface is what we declared (4 @test)
  test_policy_gate.kry  the split + the gate catching over-privilege (4 @test)
```

## Known limitations (honest)

This is a scaffold, and it surfaced real toolchain limitations that are worked
around here without editing the compiler or stdlib:

- **`std::http::http_serve` is not used.** It spawns a thread per connection,
  and in this toolchain that `spawn` closure miscompiles: the AOT server
  segfaults after its first request. The scaffold instead owns a blocking accept
  loop using `std::http`'s pure HTTP machinery (parser/serializer/responses).
  The spec scopes serving to exactly this blocking shape.
- **Run the daemon with `kryos run`, not the AOT binary.** `kryos build
  --release` builds successfully and the AOT binary's *client* subcommands
  (`status`/`get`/`apply`, verified above) work, but the AOT backend miscompiles
  the blocking TCP accept loop in this multi-module program (serves one request,
  then segfaults). The Cranelift JIT (`kryos run src/main.kry -- serve`) serves
  all routes correctly across many requests. This is a backend codegen gap
  (reproduced minimally), not a logic error.
- **The daemon's self-audit is a self-contained copy of the subset check,** not
  a direct `kryos-policy` import. Importing kryos-policy into the daemon also
  links `std::string`, whose `trim` collides with kryos-policy's `trim` in the
  flat module namespace, and the mis-link segfaults the accept loop. The CI gate
  (`demo_microctl.kry`, `tests/`) uses kryos-policy directly; the daemon keeps a
  tiny equivalent. Same contract, two enforcers.
- **Route handlers are dispatched by name** (a path `if`/`elif`), not via
  `std::http`'s `Router` + `match_route(route.handler)`. A handler stored as an
  `fn`-value in an array-of-struct does not execute correctly when called back
  out of the array in this toolchain (returns an empty body).

## License

Apache-2.0. See `LICENSE`.
