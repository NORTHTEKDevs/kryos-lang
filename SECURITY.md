# Security Policy

## Supported Versions

Kryos is on a fast-moving release cadence. Only the latest minor version
on the `master` branch receives security fixes.

| Version | Supported |
| ------- | --------- |
| 2.2.x   | ✅        |
| < 2.2   | ❌        |

## Reporting a Vulnerability

If you believe you have found a security vulnerability in Kryos —
including the compiler, runtime (`kryos-rt`), or any standard-library
crate — **please report it privately** so we can fix it before public
disclosure.

**Preferred channel:** email [info@northtek.io](mailto:info@northtek.io)
with the subject line `[kryos security] <short description>`.

When reporting, please include:

- A clear description of the issue and its potential impact.
- A minimal reproduction (a `.kry` program, a compile command, or a Rust
  unit test that exercises the bug).
- The Kryos version (`kryos --version`), host OS, and toolchain.
- Whether the issue is exploitable in a sandboxed (WASM) context, in
  the native AOT build, in the JIT (`kryos run`), or in all of the above.

You will receive an acknowledgement within **72 hours**. We aim to ship
a fix or a documented mitigation within **14 days** for confirmed
high-severity issues. Lower-severity findings are folded into the
regular release cadence.

## Scope

In scope:

- Memory safety violations in the runtime or compiler-emitted code that
  are reachable from safe Kryos source.
- Capability-system escapes: code that obtains a capability it was never
  granted (e.g. unrestricted file or network access from a `@pure`
  function).
- Type-checker soundness bugs that allow well-typed Kryos programs to
  corrupt memory.
- Sandbox escapes in the WASM target.

Out of scope:

- Crashes or panics from clearly malformed input that do not affect
  memory safety or capability boundaries.
- Issues that require an attacker to already have arbitrary code
  execution on the host.
- Performance regressions, unless they enable a denial-of-service path
  on the JIT (`kryos run`) that a hostile input could exploit.

## Disclosure

Once a fix is shipped, we will publish a brief advisory in the
[CHANGELOG](CHANGELOG.md) and credit the reporter unless they prefer
to remain anonymous.
