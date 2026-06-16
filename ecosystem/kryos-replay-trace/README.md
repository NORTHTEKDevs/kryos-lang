# kryos-replay-trace

Deterministic, capability-bounded **effect-level** record/replay for debugging.

[rr](https://rr-project.org/) (Mozilla record/replay) records at the
syscall/hardware level and replays the whole process bit-for-bit. It is the gold
standard for *determinism* -- but it has no notion of *which capabilities* a
recorded segment exercised, and it cannot bound a replay's authority. You cannot
hand someone an rr trace and a guarantee that replaying it touches nothing the
original did not.

kryos-replay-trace does the complementary thing. It records the sequence of
**capability-bearing builtin calls** a program made -- `file_read` (io),
`env_get` (process), `time_now` (time), `http_get` (net, mocked) -- with their
arguments and results, to a JSON **tape**. Replay serves the recorded results
for matching calls and **refuses any capability-bearing call not present in the
tape**. The replay engine is itself `@capabilities()` -- the empty capability
set -- so the compiler *proves* the replay performs no real effect at all.

> "Re-run this incident safely, with a guarantee it cannot touch anything the
> original did not." The guarantee is the authority ceiling, not bit-level
> determinism. **This is strictly weaker than rr on determinism and stronger on
> the authority bound** (see [Determinism boundary](#determinism-boundary)).

## The guarantee

A **tape** is an ordered list of **events**. Each event records one
capability-bearing call: its capability class, the builtin name, a content hash
of its arguments, the canonical arguments, and the recorded result.

- **Faithful replay** re-issues the recorded calls in order and reproduces the
  original's output **byte-for-byte**, served entirely from the tape.
- **A divergent replay that issues an unrecorded call** -- a different argument,
  a different builtin at that position, or one call too many -- is **refused**:
  it throws a catchable error naming the offending call, and **no result is
  served**, so no unrecorded effect can leak through.
- **The replay path is `@capabilities()` (the empty set).** The compiler refuses
  (`E0505`) any attempt to make a replay function perform a real effect. The
  replay's authority is provably a subset of every envelope, including the
  recorded one -- it can touch *nothing* the original did not.

## How it works

### The tape (`src/tape.kry`)

```
struct Event { cap: str, builtin: str, args_hash: str, args: str, result: str }
struct Tape  { events: [Event] }
```

`args_hash` is a polynomial rolling hash (mod a ~1e16 prime, kept strictly inside
`i64` so it never overflows under the debug JIT's checked arithmetic). Equal
argument bytes always produce an equal digest, on both backends. The tape stores
**no timestamps of its own**, so a faithful replay is byte-identical run to run.

Tapes serialize to JSON by hand-built string concatenation and parse back with
the native `json_*` builtins (building `std::json` *values* loses heap payloads
under LLVM/AOT and the float formatter cannot lower under the `kryos test` JIT --
hand concatenation sidesteps both). The round trip preserves quotes, backslashes,
newlines and tabs (`tests/test_replay.kry::json_round_trip_preserves_specials`).

### Recording (`src/record.kry`)

Each shim performs the **real** effect, then appends an event and returns both
the result and the grown tape (value-threaded -- there is no hidden global
recorder). The shims carry the real capability they exercise, enforced by the
compiler:

| shim | builtin | `@capabilities` |
|------|---------|-----------------|
| `rec_file_read` | `file_read` | `io` |
| `rec_env_get`   | `env_get`   | `process` (note: `env_get` is `process`, not `env`, in the Kryos taxonomy) |
| `rec_time_now`  | `time_now`  | `time` |
| `rec_http_get`  | `http_get_mock` | *(pure)* -- network capture is out of scope; the mock is deterministic and the event is classed `net` to show the authority class |

### Replay (`src/replay.kry`) -- the pure engine

Matching is **ordered by a cursor**: the Nth replayed call must match the Nth
recorded event by `(builtin, args_hash)`. Ordered (not set-membership) matching
is required so that repeated identical-argument calls -- e.g. two `time_now()`
calls that recorded *different* instants -- replay in the right order. A mismatch
or a call past the end of the tape throws a divergence and serves nothing.

Every function here is `@capabilities()`. That is the whole authority story:
`fixtures/escalation_attempt.kry` is a tampered shim that tries a real
`file_read`; `kryos check --strict-capabilities` refuses it (`E0505`). The real
`rep_*` functions go further -- because they are explicitly `@capabilities()`, a
real effect inside one is refused **even without** `--strict-capabilities`
(verified during the build by transiently adding `file_read` to `rep_file_read`:
`error[E0505]: builtin file_read requires io capability`).

## Determinism boundary

Effect-level replay is deterministic **only for single-threaded, effect-driven
programs** -- programs whose observable behavior is a function of the results of
their capability calls. It is **strictly weaker than rr**:

- **No instruction-level determinism.** Two runs that compute different values
  from the *same* recorded effects (e.g. reading uninitialized memory, depending
  on allocator addresses) are not captured. This is effect-level, not
  instruction-level.
- **Single-threaded only.** Thread/actor scheduling nondeterminism is out of
  scope; only single-threaded effect order is recorded.
- **No real network capture.** `http_get` is a deterministic mock.
- **No replay across source changes.** The tape matches on `(builtin, args)`, not
  on program structure.

What it gives you that rr does not: a **compiler-proven authority ceiling** on
the replay. That is the entire point -- safety, not perfect reproduction.

## Running

```bash
KRYOS="…/compiler/target/release/kryos.exe"   # use the current build

# Tests (both the @test gates and the file-test smoke compile)
"$KRYOS" test --path ecosystem/kryos-replay-trace

# End-to-end: record an incident, replay it, refuse a divergent run
"$KRYOS" run ecosystem/kryos-replay-trace/demo_record_replay.kry

# Replay a persisted tape from disk (run from the repo root)
"$KRYOS" run ecosystem/kryos-replay-trace/demo_replay_from_file.kry

# Prove the escalation fixture is refused
"$KRYOS" check --strict-capabilities ecosystem/kryos-replay-trace/fixtures/escalation_attempt.kry   # E0505, exit 1
```

Both backends are supported: every gate above also passes under
`kryos build --release` (LLVM AOT), including the array-of-struct tape parse and
the real record -> JSON -> replay round trip.

### Test evidence (actual output)

```
$ kryos test --path ecosystem/kryos-replay-trace
running 7 file tests
  PASS demo_record_replay      PASS demo_replay_from_file   PASS fixtures/escalation_attempt
  PASS src/record   PASS src/replay   PASS src/tape   PASS tests/test_replay
Tests: 7 passed, 0 failed, 0 skipped, 7 total

running 10 @test functions
  PASS exact_byte_replay                  PASS ordered_matching_repeats
  PASS json_round_trip_preserves_specials PASS divergence_unrecorded_args
  PASS divergence_wrong_builtin           PASS divergence_past_end
  PASS record_mock_roundtrip              PASS authority_ceiling_classes
  PASS deterministic_replay               PASS hash_is_deterministic_and_args_sensitive
Tests: 10 passed, 0 failed, 0 skipped, 10 total
```

The byte-identical replay and the divergence refusal, from `demo_record_replay`:

```
original output: incident-payload-42|32|1781594934|HTTP/1.1 200 OK …
replay output:   incident-payload-42|32|1781594934|HTTP/1.1 200 OK …
OK: replay is byte-identical to the original (served from the tape)

refused: replay diverged: unrecorded file_read call (args=/etc/shadow) at
position 0 -- the recording has file_read(args=…incident.txt) there, a
different argument; refused (replay performs no new real effects)

recorded effect classes (authority ceiling): {net, io, process, time}
replay engine actual authority:              {}   (every replay fn is @capabilities())
```

## Tape format

```json
{"events":[
  {"cap":"io","builtin":"file_read","args_hash":"6387643513256319","args":"/var/log/incident.log","result":"ERROR disk full at 03:14:07"},
  {"cap":"process","builtin":"env_get","args_hash":"9410077742459434","args":"DEPLOY_ENV","result":"production"},
  {"cap":"time","builtin":"time_now","args_hash":"1125899906842597","args":"","result":"1781594934"},
  {"cap":"net","builtin":"http_get","args_hash":"6977155384070794","args":"https://status.example/api/incident/42","result":"{\"degraded\":true,\"region\":\"us-west\"}"}
]}
```

(See `fixtures/incident_tape.json`, replayed by `demo_replay_from_file.kry`.)

## Layout

```
src/tape.kry        Event/Tape model, content hashing, JSON ser/de, load/save, authority ceiling
src/record.kry      record shims (real effects) -> tape
src/replay.kry      pure (@capabilities()) replay engine: serve-or-refuse
demo_record_replay.kry      record -> replay -> divergence, end to end
demo_replay_from_file.kry   load + replay a persisted tape
fixtures/incident_tape.json sample tape
fixtures/escalation_attempt.kry  compile-fail (under --strict-capabilities) escalation fixture
tests/test_replay.kry       10 @test gates + a main() that also drives a REAL record/replay
```

## Composes with

- **23 kryos-eval-replay** -- budget-bounded transcript replay. Same "re-run an
  incident safely" thesis, on the token/call envelope axis rather than the
  capability axis.
- **kryos-audit-trail** (merged) -- the tape is an append-only effect log; an
  audit trail is its provenance-signed cousin.

No compiler edits. Apache-2.0.
