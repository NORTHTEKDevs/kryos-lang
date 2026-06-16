# kryos-syscall-caps

An strace-style audit that maps observed Linux syscalls back to Kryos
capability classes and diffs them against the program's declared manifest.

## The wedge

`strace` and `seccomp` can see (and limit) the syscalls a binary issues, but
they have no idea which *function* in the program issued them, and no notion of
the program's intended capability surface. Deno enforces permissions at runtime
process-wide. Go and Rust ship a native binary and ask you to trust it.

Kryos has both halves no one else has together: a compiler-computed
per-function capability map **and** a single native binary. This tool closes
the loop empirically. Run a Kryos binary under `strace`, bucket the observed
syscalls into the same 11-variant capability taxonomy the compiler uses
(`net`, `io`, `ffi`, `compute`, `crypto`, `process`, `env`, `term`, `db`,
`time`, `all`), and **diff the observed set against the declared manifest**. Any
capability class the program exercised that no annotated function declared is
flagged. It is the runtime complement to a pre-execution capability check: "did
the static capability claim actually hold at runtime?"

## Why this catches closure-hidden effects

The Kryos manifest does **not** walk closures. An effect reached from inside a
closure can be absent from the statically declared capability set while still
happening at runtime. That is normally a blind spot. Here it is the headline
feature: the syscall trace records the effect regardless of how it was reached,
so a program that declares only `io` but beacons out from inside a closure shows
`net` in its observed set and is flagged `observed-not-declared`. The
`closure_leak` fixture and `demo_violation.kry` demonstrate exactly this.

## What it does (MVP scope)

- `parse.kry` -- extract syscall names from `strace -f` text (handles bare PID
  columns, `[pid N]` prefixes, `<unfinished ...>`/`<... resumed>` splits, and
  signal/exit lines).
- `taxonomy.kry` -- bucket a syscall name into a capability class.
- `diff.kry` -- union the observed classes into a set and diff against the
  declared manifest; the first `execve` (the program's own image load) is not
  counted as a `process` spawn.
- `report.kry` -- read the declared manifest from a `caps.json` sidecar and
  render the audit summary.
- `demo_audit.kry` -- file-driven audit with the spec's exit-code contract
  (`0` clean, `1` violation).
- `demo_violation.kry` -- the closure-hidden-net case, self-contained.

## Run it

The library and tests do **not** require strace -- they operate on trace text,
so the audit logic is testable anywhere:

```
kryos test --path ecosystem/kryos-syscall-caps
```

Audit a captured trace against a manifest:

```
kryos run demo_audit.kry tests/fixtures/honest.strace       tests/fixtures/honest.caps.json
kryos run demo_audit.kry tests/fixtures/closure_leak.strace tests/fixtures/closure_leak.caps.json
```

The first prints `verdict: OK` and exits 0; the second prints
`verdict: VIOLATION` (undeclared `net`) and exits 1.

### Capturing a live trace (Linux + strace only)

```
./strace-capture.sh ./your_prog your_prog.caps.json
```

This runs `strace -f -e trace=%network,%file,%process -o trace.txt -- ./your_prog`
and feeds the trace plus the manifest to `demo_audit.kry`.

## Manifest format

A small JSON sidecar holding the unioned (flat) declared capability set -- the
same badge convention `kryos-plugin-sandbox` and `kryos-audit-trail` use:

```json
{"version":1,"declared":["io"]}
```

`"all"` is a wildcard that grants every class.

## Honest limitations

This is an **audit**, not a sandbox. It observes; it does not enforce. Positioned
as a security boundary it would overclaim -- a malicious program can still do
whatever it likes; this only tells you afterward that it did.

- **Linux-only.** Depends on `strace`. macOS (`dtruss`) and Windows (ETW) are
  out of scope for the MVP.
- **Bucketing is approximate.** We bucket by syscall *name* only, not by the
  arguments. `openat("/etc/resolv.conf")` is really part of DNS (`net`) but
  looks like `io` at the syscall layer, so it lands in `io`. Conversely, a few
  capability classes have no dedicated syscall: `env` (getenv is a userspace
  read), `compute`, `crypto`, `db`, and `term` are largely library/userspace
  concerns and are **not derivable** from a syscall trace. Unmapped syscalls
  contribute nothing to the observed set.
- **No per-function attribution.** Attributing a syscall to a specific Kryos
  function needs symbolized stack unwinding -- out of scope. We report at the
  whole-program granularity (observed set vs declared union).
- **strace output is mildly version-dependent.** The parser targets common
  `strace -f` formatting; exotic libc/kernel formatting variants may need
  tweaks.

Its real value is as a CI smoke check: assert that a binary's runtime syscall
behavior stays within the capability surface its manifest declared, and catch
drift -- including closure-hidden effects the static manifest cannot see.

## Layout

```
kryos.toml
README.md
LICENSE                     Apache-2.0
strace-capture.sh           live-capture wrapper (Linux + strace)
src/taxonomy.kry            syscall name -> capability class
src/parse.kry               strace -f text -> syscall names
src/diff.kry                observed set, undeclared diff, audit()
src/report.kry              manifest reader + report renderer
demo_audit.kry              file-driven audit, exit 0/1
demo_violation.kry          closure-hidden-net case
tests/test_audit.kry        5 @test functions
tests/fixtures/             honest + closure_leak traces and manifests
```

## License

Apache-2.0.
