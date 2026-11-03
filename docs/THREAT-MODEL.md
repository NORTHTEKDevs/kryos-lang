# Kryos capability threat model

What Kryos capabilities protect against, what they do not, and which known
holes are still open. Read this before treating `@capabilities`, `deny!` or
`@pure` as a security boundary.

Measured 2026-11-02 on `kryos 1.2.0`, a full-workspace `cargo build --release`
made in place from commit `a9c4680b` (branch `fin/v1.3-finish`, clean tree).
Its compiler and stdlib are identical to master `7cab5799` (PR #40, the
bypass-classes wave): `git diff 7cab5799 -- compiler/crates compiler/stdlib` is
empty on this branch, which adds documents, check scripts and tests, and whose
release commit changes only the version strings (`compiler/Cargo.toml`,
`compiler/Cargo.lock`), so the 1.3.0 build prints `kryos 1.3.0`.
`tools/loop/check-escapes-documented.sh` and `tools/loop/check-docs-truth.sh`
were re-run on that 1.3.0 build on 2026-11-03 and measured the same counts
(80 escaping, 180 rejected). The first version of this document
was measured 2026-11-01 at `dbfb989d` (the R2 slot-rows wave head); since then
PR #38 (R2, merged as `732714ab`), PR #39 (R3, the `deny!` verdict is taken
after the whole module is checked, merged as `e2fbc46c`) and PR #40 (merged as
`7cab5799`) changed the compiler, and this version re-measures everything. Every
statement below is tagged with how it was established:

- **pinned**: a line of `tests/threat_model/claims.tsv` (its id is named) runs a
  committed program through the binary and compares the verdict;
  `tools/loop/check-threat-model-claims.sh` re-runs all of them and was run for
  this revision (section 5). A pinned statement can only go stale by failing a
  check;
- **gate**: another committed script pins it (named), and was run unless noted;
- **executed**: a program was run through that binary on 2026-11-02 and the
  quoted verdict was observed, but nothing pins it (the reason is given);
- **carried**: executed 2026-11-01 at `dbfb989d` and not re-executed. It may be
  stale. It is kept because it has not been shown false;
- **source**: read from code or docs, not executed;
- **not verified**: stated as a boundary of this document, not tested.

Nothing here is a soundness proof. The checker is heuristic code that has been
bypassed in five successive hardening rounds ([docs/10-capabilities.md](10-capabilities.md))
and in the escape corpus below; "no known escape" means "no escape found by the
searches that were run", and the searches were run by the same people who wrote
the checker.

"Known" has two precise meanings here. Section 4.1 is the escape corpus: 260
repros listed by name in `tools/loop/escape_status.sh`, 80 of them still accepted.
Section 4.5 lists accepted zero-authority shapes (B5, B6, B7 and B9) that are
not in that corpus; five more (B1 to B4 and B8) were listed in the first version
of this document and are closed in this build (PR #40). Both tables are checked
by one script (section 5). Neither table is a claim that nothing else exists.

## 1. Scope and attacker model

### What a capability is here

`@capabilities(fs:read)`, `deny!(net:http) { .. }` and `@pure` are **compile-time
checks of program text**. `kryos check`, `kryos run` and `kryos build` run the
checker and refuse to run or build a program it rejects. The build cache is
consulted only after the checker has accepted the program in the mode of this
build (B8 was closed by PR #40; executed, section 2 G9). In the default mode the
checker's entry points are `main` and annotated functions (B5). The compiled
program carries **no runtime capability check**: there is no reference monitor,
no syscall filter, no audit log (`docs/10-capabilities.md`, source). `kryos
audit` runs the checker in inferred mode and prints its violations; its
capability inventory lists declared annotations only (pinned: claims.tsv
S1-audit-inferred, S1-audit-inventory).

Consequences, stated plainly:

- A capability check constrains the source it was run on, with the toolchain and
  flags it was run with. It says nothing about any other binary, any other
  compiler, or the same source compiled with a different mode.
- It is **not a sandbox for untrusted code**. It is a discipline that stops
  honest code from drifting into authority it did not declare, and a reviewable
  statement ("`main` declares `fs:read`, so this program reads files") of what a
  program may do. Against adversarial source it is a strong speed bump with
  known holes (section 4), not a wall.
- If you must run code you do not trust, run it in an OS-level sandbox
  (separate process, container, WASM engine with no imports) and treat the
  capability check as defense in depth.

### Trusted (assumed correct, not checked)

| Component | Why it is trusted | If it is wrong |
|---|---|---|
| The compiler and capability checker (`kryos-capabilities`, `kryos-types`, `kryos-driver`) | They are the enforcement | Every guarantee below is void; section 4 is the known part of this |
| The runtime and native stdlib (`kryos-rt`, `kryos-stdlib-native`) | Compiled programs call into them; `grep -rE 'unsafe\s*\{' compiler/crates --include=*.rs` finds 467 `unsafe {` blocks on 2026-11-02 (executed; a count, not pinned, because it drifts with every edit; `docs/17-unsafe-audit.md` still says about 310, dated May 2026) | A memory-safety bug reachable from safe program text defeats compile-time checks at run time |
| The Kryos stdlib source (`compiler/stdlib/*.kry`) | It is built on raw-memory primitives, so the raw-memory gate exempts it (section 2, G6). Its public API is therefore part of the attack surface: B9 | A malicious stdlib file has unrestricted raw memory access; a stdlib API that hands out a raw address is a pointer source (B9) |
| The directory the toolchain resolves the stdlib from | Which directory counts as "the stdlib" decides what is exempt. The trusted search is, in order: `<exe_dir>/stdlib`, `<exe_dir>/../stdlib`, the `stdlib/` of the build tree (compile-time `CARGO_MANIFEST_DIR` ancestors), a `stdlib/` directory in any ancestor of the importing file, then `KRYOS_STDLIB_DIR` (`resolve.rs`, `find_trusted_stdlib_dir` and `find_stdlib_dir_impl`, source). While one of the first three exists, `KRYOS_STDLIB_DIR` changes where `use std::..` is read from but does not make that directory trusted (gate: `tests/security_gate.sh`, run for this revision with the binary in its build tree: "KRYOS_STDLIB_DIR does not make user modules trusted (still requires `ffi`)" and "control: the real stdlib named by KRYOS_STDLIB_DIR is still trusted" are both `ok`). **When none of the first three exists (a binary run outside its distribution layout and outside its build tree), a `stdlib/` directory above the importing file, or the `KRYOS_STDLIB_DIR` directory, is the trusted one** (source; not executed, because the first three steps could not be made to miss on the build used here) | Anyone who can write the toolchain's own stdlib directory, or, on such a layout, place a `stdlib/` directory above a project's files, can grant raw memory to any code |
| The person or host that invokes the compiler and chooses its flags | The mode (`--capabilities-mode`, `--strict-capabilities`, `[capabilities] mode` in the root `kryos.toml`) is chosen outside the program text | `permissive` turns most of the checking off (section 3, N2) |
| A host that embeds a compiled module or links `kryos-rt` | It supplies the process environment, the WASM imports, the filesystem | Capabilities do not describe or limit the host |
| The package registry index (`NORTHTEKDevs/kryos-registry`) and any explicit `git =` source, on first fetch | Checksums come from the same place as the code (section 3, N8) | A compromised index yields code with a matching checksum |

### Not trusted

- **The program text being checked**, including every `.kry` file it imports
  that is not the stdlib. This is the attacker-controlled input.
- **A dependency's source** (a user module, or a package fetched by `kryos pkg`).
  It is checked as program text. Carried for user modules; packages were not
  probed separately (not verified).

### What the attacker may do

Write any Kryos source that parses, including source that holds some
capabilities itself and tries to use them where `deny!` or a callee's declaration
says it must not. The attacker may not change the compiler, the flags, the
stdlib directory, the registry, or the host.

## 2. What capabilities DO guarantee

Each guarantee is for source accepted by `kryos check` with the stated mode.
**None of them is unconditional.** Each holds except for the 80 accepted
escapes in section 4.1 (each defeats the narrowing of a `deny!` block) and the
four zero-authority shapes in section 4.5 (B5, B6, B7, B9), of which B9 defeats
G6 for the stdlib's API surface and B5 defeats G1 and G2 for library entry points;
the exceptions are repeated under each guarantee. "Default" is `inferred`: no
flag, no project file. Error codes are those printed by the binary above.

**G1. A gated builtin needs its capability, where it is called.**
A direct call to a gated builtin in a function that does not hold the capability
is rejected with `E0505` ("builtin `file_read` requires `fs:read` capability").

- Annotated function missing the capability: rejected in permissive, inferred
  and strict (pinned: claims.tsv G1-annotated-missing-cap-default, -strict,
  -permissive; program `@capabilities(net) fn main() { file_read("x.txt") }`).
- Unannotated `main`: rejected in inferred (default) and strict; accepted in
  permissive (pinned: claims.tsv G1-unannotated-main-default, -strict,
  -permissive; rc 1 / rc 1 / rc 0).
- **Exceptions.** B5 (section 4.5): in the default mode the boundary is `main`
  plus annotated functions, so an unannotated `pub fn` in a file with no `fn
  main`, and an unannotated `@test` fn run by `kryos test`, are accepted and not
  charged (rejected only with `--strict-capabilities`; pinned: claims.tsv
  B5-test-default, B5-test-strict, B5-test-runs, and the B5 row of the
  known-bypasses table). **Closed in this build:** B1 and B2, where an `impl` or
  `trait` method named like a gated builtin made every bare call to that builtin
  uncharged (PR #40; section 4.5; pinned: claims.tsv B1-closed-*, B2-closed-*;
  gate: `tests/bypass_gate.sh`).
- Which builtin needs which capability is a table in
  `kryos-capabilities/src/model.rs`. `tests/authority_surface_gate.sh` checks,
  for every classified builtin, that it is rejected with no capability and
  accepted with exactly the capability the diagnostic names (gate, run for this
  revision: PASS, 82 builtins checked, 77 gated with a named capability, 5
  rejected without naming one, 0 ungated, 0 ungrantable).
  `tests/inferred_soundness.sh` pins the rest (gate, not re-run here).
  `tests/strict_caps_examples.sh` pins the examples: its file filter (every
  `examples/*.kry`, `examples/showcase/*.kry` and `examples/showcase/extra/*.kry`
  that is not `*_overreach.kry` and contains `fn main`) was run by hand on
  2026-11-02: `kryos check --strict-capabilities` accepted 102 of 102 files
  (executed; the script itself was not run, it also kills `kryostokens.exe`).

**G2. Authority propagates up the call graph and never widens.**
A call to a function that needs a capability its caller does not hold is
rejected with `E0507` ("call to `inner` requires capabilities [fs:read] not
granted to caller"). Pinned in default, strict and permissive for an annotated
caller (claims.tsv G2-annotated-caller-*: `@capabilities(fs:write) fn main() {
inner() }` where `inner` is `@capabilities(fs:read)`). In inferred mode an
unannotated helper's authority is the union of what it and its callees need, so
`main` must declare it (pinned: claims.tsv G2-inferred-helper-default,
-strict, -permissive: helper calling `file_read`, `main` declaring only
`fs:write`: `E0507` in default, `E0505` on the helper in strict, accepted in
permissive). The first version listed B1 as an exception inside a callee; it is
closed.

**G3. `deny!(c) { .. }` rejects a block that uses `c`.**
A `deny!` block whose body uses a denied capability, directly or through calls
the checker resolves, is rejected with `E0110` ("capability `fs:read` is denied
in this block, but the block uses it"). Pinned in default, strict and
permissive for the direct case (claims.tsv G3-deny-direct-*). For closures
reaching the block through parameters, locals, containers, struct fields, actor
state, `spawn`, generics, `dyn` and the other shapes in section 2 of
[10-capabilities.md](10-capabilities.md): gate `tests/security_gate.sh`
(gate, run for this revision: PASS, 342 `ok` lines, 0 FAIL) and `tools/loop/escape_status.sh` (180 of 260 repros rejected,
executed 2026-11-02). **Exceptions: the 80 rows in 4.1, all of which defeat
`deny!`.** `deny!` does not bound a callee that is handed a closure (N16, B6).

**G4. `@pure` functions call no gated or impure builtin.**
Rejected with `E0110` ("`@pure` function cannot call non-pure function
`file_read`") in all three modes (pinned: claims.tsv G4-pure-call-*). `@pure`
restricts calls only: it does not forbid mutating an argument container or
module-level state (B7, section 4.5: a `@pure` function pushed onto its argument
and incremented a global; gate: check-escapes-documented.sh, row B7). No
dedicated gate was found for the call restriction beyond the claims.

**G5. Authority carried by a function value is charged at the call that invokes
it, including through containers.**
A call through a closure or fn-typed value is charged the authority the value
carries; when that cannot be shown to be a subset of what the caller holds it
requires `all` rather than nothing (fail closed). Evidence:
`tests/capability_matrix_gate.sh` enumerates 75 shapes (source x container x
transport); run for this revision (gate, PASS): **75 of 75 attacks rejected in
default and strict**. The cost of failing closed is in 4.2.
**Exception:** the receiver of a function-typed or actor-handler parameter is not
charged; the sender is (N16, B6).

**G6. The raw-memory builtins need `ffi`, in every mode, at the call site in user
code, and so does every `std::ffi` export. The stdlib's other public APIs do not
need it, and some of them hand out raw addresses (B9).**
`alloc`, `free_bytes`, `ptr_read_i64`, `ptr_write_i64`, `ptr_byte_at`,
`ptr_set_byte`, `str_to_ptr`, `arr_to_ptr` called from any non-stdlib function
are rejected with `E0505` ("requires `ffi` capability"), in permissive too
(executed 2026-11-02, all eight, unannotated, in default, strict and permissive).
The claims file pins two of the eight, `str_to_ptr` and `ptr_byte_at`
(claims.tsv G6-raw-memory-undeclared-default, -strict, -permissive; accepted with
`@capabilities(ffi)`: G6-raw-memory-declared-*); the other six are gated by
`tests/authority_surface_gate.sh`. The requirement
does not propagate out of the stdlib, so a stdlib user needs no `ffi`. It also
covers an unannotated helper and a user module reached by `use` (gate:
`tests/security_gate.sh` sections 1, 1b, 2, 3, `cap_escape_raw_memory*.kry`). Since
PR #40 every `std::ffi` export is `@capabilities(ffi)` and a program that
imports one needs `ffi` (gate: `tests/bypass_gate.sh` section 3; pinned:
claims.tsv B3-closed-default, B3-closed-strict).

**Exception (B9, open, not an escape-corpus row).** G6 guards the language
builtins and the `std::ffi` exports. It does not stop a zero-capability program
from getting a pointer out of the stdlib's own public API, because the stdlib is
exempt from the gate and some of its structs publish a raw heap address in a
public `i64` field with methods that dereference it. `std::sync::AtomicInt` is a
struct `{ cell: i64, mu: Mutex }`; `load()` and `store()` read and write
whatever address `cell` holds, and a struct literal is unrestricted. The
committed repro `tests/security/bypass_stdlib_raw_address_field.kry` (declares
no capability; `kryos check` rc 0 in default and `--strict-capabilities`;
`kryos run` prints `BYPASS-B9-OK`; executed 2026-11-02, and the row-B9 check of
`check-escapes-documented.sh`) builds an `AtomicInt` around an address it
computed by integer arithmetic from another atomic's cell, reads that atomic's
value through it and overwrites it. A `std::tensor` handle is also a heap address
as a plain `i64` (executed 2026-11-02, not committed because the value read is
not deterministic: an `AtomicInt` built around `tensor_zeros([2, 3])` was
accepted and `load()` returned a nonzero header word). `std::chan::ChanWaitGroup.count_cell`
and the `std::string::StringBuilder` cell have the same shape by reading the
source (LEDGER, "Zero-authority bypass classes B1-B8"; not run). What the repro
does not show: it touches only memory the program allocated itself, makes no
file, process, network or `dlopen` call (it writes to stdout, and the atomic
methods take a mutex) and cannot crash; it is not an exploit of the OS. What it
does show is that an integer the program chose is dereferenced by a stdlib method
with no `ffi`, so "you cannot obtain a pointer without declaring `ffi`" is false
for the stdlib's API surface. That an attacker-chosen address, not just one the
program allocated, is dereferenced the same way is read from `sync.kry`
(`ptr_read_i64(self.cell, 0)` in `load`/`store`; source, not run with a foreign
address because that could crash).
The root is that a handle is a plain `i64` with public fields; the LEDGER records
that closing it needs an opaque handle type or field privacy, and that gating
`AtomicInt.load()` with `ffi` would over-reject every ordinary use of atomics.

**G7. `extern` blocks need a capability.**
`extern "C" { fn getpid() -> i32 }` is rejected with `E0506` (requires `ffi`) in
inferred and strict, and with `E0508` (not a `kryos_*` runtime symbol) in all
three (pinned: claims.tsv G7-extern-c-default, G7-extern-c-strict,
G7-extern-c-E0508-*). A `kryos_*` extern is charged what the native does:
`kryos_env_get` and `kryos_process_exec_simple` externs need `process` (`E0506`,
pinned: claims.tsv G7-kryos_env_get-*, G7-kryos_process_exec-*, default and
strict). The first version's exception (B4) is **closed in this build**: a
`kryos_*` native that the capability table does not map used to need no
capability (the table's default arm was "ambient"); since PR #40 the table
classifies all of the runtime's exported symbols and an unclassified `kryos_*`
name requires `ffi` (gate: `tests/bypass_gate.sh` section 4 and the cargo test
`native_symbols`; pinned: claims.tsv B4-closed-default, B4-closed-strict, an
extern of `kryos_ffi_cstr` is `E0506`).

**G8. `unsafe { .. }` and `comptime { .. }` grant no authority.**
A `file_read` inside `unsafe { }` or `comptime { }` is charged to the enclosing
function like any other call (pinned: claims.tsv G8-unsafe-default,
G8-comptime-default rejected, G8-unsafe-permissive, G8-comptime-permissive
accepted for an unannotated `main`). `comptime` does not evaluate at compile
time today (`docs/11-comptime.md`, source), so `kryos check` and `kryos build`
do not execute program text. `tests/security_gate.sh` pins `unsafe` blocks as
call-containing positions for the reassignment shapes (the comment at line 971
of that script, and the `unsafe_block`, `unsafe_in_match_arm`, `arr_unsafe` and
`map_unsafe` cases, are present on this commit; gate run for this revision).

**G9. The same check gates run, build and the WASM target, except for the project
manifest mode.**
One unannotated `file_read` program was rejected by `kryos check`, `kryos run`,
`kryos build` and `kryos build --backend wasm` (pinned: claims.tsv G9-run,
G9-build, G9-build-wasm, and G1-unannotated-main-default for `check`). The check
runs in the driver before backend selection (`pipeline.rs`, source). The REPL
grants the session only the capabilities passed with `--allow` (carried; gate:
`tests/repl_capability_gate.sh`, not re-run here).

- **Cache (B8), closed in this build.** In the first version `kryos build --cache`
  returned a cached artifact before the checker ran, with a key that omitted the
  capability mode and the imported modules. Since PR #40 the cache is consulted
  after the checker accepted the program in the mode of this build, and the key
  covers the capability mode, the strict flag and the imported modules. Executed
  2026-11-02 on the sequence of the old repro (an unannotated `env_get` program,
  `KRYOS_CACHE_DIR` a scratch directory): `kryos build` rc 1 with `E0505`;
  `build --capabilities-mode=permissive --cache` rc 0; then default-mode
  `build --cache` rc 1 with `E0505` and no artifact written, `build
  --strict-capabilities --cache` rc 1 with `E0505`, and `KRYOS_CACHE=1 kryos
  build` rc 1 with `E0505`. Not pinned by the claims file (it needs a cache
  directory and builds an artifact); gate: `tests/bypass_gate.sh` section 8. The
  cache directory is `$KRYOS_CACHE_DIR/build`, `$XDG_CACHE_HOME/kryos/build` or
  `$HOME/.cache/kryos/build`, with `%USERPROFILE%\AppData\Local\kryos\build` as the
  Windows fallback (source: `build_cache.rs::cache_root`); it is shared by every
  project of the user.
- **Manifest mode.** Only project-mode `kryos build` (no file argument) reads
  `[capabilities] mode` from `kryos.toml`. With `mode = "permissive"` and an
  unannotated `env_get`: `kryos check` rc 1 and `kryos run` rc 1 with `E0505`, and
  `kryos build` rc 0 (pinned: claims.tsv G9-manifest-check, G9-manifest-run,
  G9-manifest-build). `check` and `run` are the stricter ones there.

**G10. Stdlib modules carry their authority to the importer.**
`use std::fs::{exists}` in an unannotated `main` is rejected with `E0507`
("requires capabilities [fs:read]"); with `@capabilities(fs:read)` on `main` it
checks (pinned: claims.tsv G10-import-unannotated, G10-import-declared; the
"and runs" of the first version was not re-executed).

## 3. What capabilities do NOT guarantee

**N1. Nothing is enforced at run time.** See section 1. A compiled binary can be
copied, patched or built by another compiler; none of it is checked.

**N2. `--capabilities-mode=permissive` checks only annotated functions.** An
unannotated function is unconstrained (pinned: claims.tsv G1-unannotated-main-permissive,
G8-unsafe-permissive, G8-comptime-permissive). The mode is chosen by whoever invokes the compiler, or by
`[capabilities] mode` in the root `kryos.toml` ("honoured when the flag is
unset", `kryos build --help`). Only project-mode `kryos build` reads the manifest
(G9). A host that compiles untrusted source must pass the mode itself and must
not read it from a manifest the source controls. Whether a dependency's own
`kryos.toml` can lower the mode: not verified.

**N3. Some authority is ambient and never charged.**
`println`, `print`, `eprintln`, `exit`, `time_now`, `time_millis`, `sleep`,
`args()` and `read_line()` need no capability (source: `model.rs`; pinned:
claims.tsv N3-args-time-sleep, N3-read-line). `abort` is also in the `model.rs`
table as needing none, but `abort()` is `E0102` (undefined variable) on this
build, so it is not callable (executed 2026-11-02). Consequences: any data a
program is allowed to read can leave through stdout or stderr; program arguments
and standard input are readable by everything; timing and stalling are not
bounded. `Capability::Time` exists in the enum and is not enforced (`model.rs`,
source). `file_exists` is not ambient: it needs `fs:read` (pinned: claims.tsv
N3-file-exists-gated).

Also accepted by `kryos check` with no capability: `str_data_ptr`,
`mem_read_byte` and `mem_write_byte`, which read and write memory by address
(pinned: claims.tsv N3-latent-intrinsics-default, -strict). They are the
self-host intrinsics of section 4.4: latent on this build, because they do not
link. The self-hosted compiler (`compiler/self-host`) uses them in its own
runtime source (`runtime.kry`), and contains no capability checker (source: a
grep for "capabilit" in `compiler/self-host/*.kry` finds only annotations on its
own programs); any program built with it is outside this document.

**N4. Capabilities are not information-flow control.** A function holding
`fs:read` and `net:http` can send file contents over HTTP. Nothing tracks what a
value contains or where it goes.

**N5. Capabilities are coarse and `ffi` / `all` are unbounded.** `@capabilities(all)`
removes the restriction. `ffi` gives arbitrary read and write of host process
memory (that is what the raw-memory builtins do; `cap_escape_raw_memory.kry`
reads a string by address). `fs:read` is not scoped to a path, and `net:http`
is not scoped to a host. The coarse names `io`, `fs` and `net` cover their
sub-capabilities.

**N6. Raw memory, `extern` and the C ABI host.** With `ffi` declared, anything
the process can do is reachable. `kryos_*` runtime symbols with raw `i64` ABI
may be declared as `extern` (`docs/13-ffi.md`, source). C-library FFI through an
`extern` block with a non-`kryos_*` name is rejected (`E0508`, G7); through
`std::ffi` and through `kryos_ffi_*` natives it needs `ffi` since PR #40 (G6,
G7). Through the stdlib's raw-address fields it still needs nothing (B9). The
`E0508` rejection is a limitation of the compiler, not a security property: the
check is "not a `kryos_*` name".

**N7. Memory safety of the compiler and runtime is not a capability property.**
`unsafe` blocks in `kryos-rt` and the codegen crates are trusted. The LEDGER
records closed double frees, use-after-frees and miscompiles (for example the
review6 owned-capture series). A reachable memory-corruption bug defeats every
guarantee in section 2 at run time. The compiler has also been crashed by
hostile source more than once (LEDGER item 14, item 47 and the 2026-10-31 wave
"three unguarded recursions crashed `kryos check`"; the committed probe
`tests/security/resource_dos_lexer_nested_interp_stack_overflow.kry`); each fix
closed instances, not the class.

**N8. Build scripts and the package registry trust root.** No package install
script mechanism was found: `kryos-package` shells out only to `git`
(`Command::new("git")` in `fetch.rs`, `registry.rs`, source). `kryos pkg install`
verifies a `sha256:<hex>` content hash of the fetched package against the
registry index entry and re-verifies on a cache hit
(`docs/package-registry.md`, `fetch.rs`, source; not executed here). That is
**integrity against the index, not authenticity**: the index entry and the package
come from the same git registry (the client clones the registry repository's
HEAD), so whoever controls the registry controls both. A dependency given by an
explicit `git =` source has no index to compare against and is trusted on first
fetch; the checksum is then recorded in `kryos.lock`. Nothing here signs
anything. After install, a dependency is ordinary program text (see "Not
trusted"), but the only thing that limits it is the checker.

**N9. Process, environment, argv.** `env_get`, `env_set`, `exec` and
`spawn_process` need `process` (pinned for `env_get`: claims.tsv
N9-env-get-default, -strict). On this build `env_set`, `exec` and
`spawn_process` are also `E0102` (undefined variable) beside `E0505`, the same
resolver defect noted for `abort` in N3 (executed 2026-11-02, not pinned).
`args()` does not need `process` (N3).

**N10. Randomness and time.** `random_bytes` needs `crypto` (source). `std::random`
is a deterministic, seedable PRNG and is ambient (source). Time is ambient (N3).

**N11. Resource exhaustion.** An infinite loop passes `kryos check` with no
declaration (pinned: claims.tsv N11-infinite-loop, `while true { }`).
`@budget(tokens = N, calls = M, usd = X)` wraps a function in a runtime budget
frame that **`std::llm` calls charge** (`kryos-mir/src/lower.rs`,
`inject_budget_frames`, source). It does not bound CPU, memory, disk, sockets,
`spawn`, or any non-`std::llm` call. `@budget` is not a capability and nothing
checks that a function calling the LLM stdlib carries one.

**N12. Side channels** (timing, cache, error text, output size) are not
addressed. Not verified.

**N13. Backends.** The capability check runs once, before backend choice
(G9, source), so `kryos run` and `kryos build` accept and reject the same
programs (apart from the manifest mode, G9). `kryos run` is not an in-process
JIT: it compiles with the Cranelift backend in debug mode, links an executable
under the temp directory and starts it as a child process (`commands/run.rs`,
source; pinned: claims.tsv N3-latent-intrinsics-run, a link error aborts `run`
with a nonzero status). That the backends then *execute* the same authority is not
established: documented value divergences between backends exist and are pinned
by `tests/backend_divergence_pins.sh`, and none was analyzed here for authority
impact. Not verified.

**N14. A host that embeds a module.** Capabilities do not describe what the
embedding process, the WASM host's imports (`docs/wasm-contract.md`: `println`,
`to_string` and string equality come in as host imports), or the filesystem
allow. The WASM target is subject to the same compile-time check (G9) and then
to whatever sandbox the WASM engine provides, which Kryos does not control.
`ecosystem/kryos-embed` ships an `agent.caps.json` sidecar with
`"verified": true` that its host compares with an allow-set before loading a
DLL (`ecosystem/kryos-embed/README.md`, source; not executed). It is an
unsigned JSON file beside the module, written by a build script after a
`--strict-capabilities` check; nothing binds it to the DLL bytes, and the same
README says a native DLL gives "process isolation only; any Windows API callable
in principle". Treat it as a declaration, not a boundary; a DLL built by a
compiler older than PR #40 can carry B1 to B4 and B8, and one built by this compiler can
still carry B9.

**N15. Zero known escapes is not a soundness proof.** The corpus is 260 repros
found by directed search; 75 more shapes are enumerated by a generator. Both
were written by the authors of the checker. Section 4.5 lists four accepted
shapes that are in neither, and the first version of this document listed eight;
they were found by reading the source and probing while that version was
reviewed, and five of them were real defects that PR #40 then closed, which is
itself evidence that the corpus was incomplete. Every previous low-count claim
in this repository was later wrong: on 2026-08-10 the README claimed a single
residual while twelve others were live, and on 2026-10-15 "0 escaping" was true
of the 19 named repros while eighteen other shapes escaped (LEDGER preamble).
It happened again while this document was revised: its first count at master
`7cab5799` was 10 accepted of 190, and the same day it became 80 of 260 when the
findings of one more wave and its two verifiers were recorded (LEDGER: the
release-freeze wave).

**N16. Authority travels with a closure (delegation).** A function-typed
parameter, and an actor-handler parameter, is charged to the *sender* at the
call that passes it; the receiver's declared set bounds nothing. A callee
declared `@capabilities()` runs a closure that needs `process` when a caller
holding `process` hands it over (B6, gate: check-escapes-documented.sh, row B6:
`check` rc 0 in default and strict, `kryos run` printed the marker, executed
2026-11-02). So `deny!` and a callee's `@capabilities(..)` narrow direct and
tracked calls only, never a closure the callee is handed. The repository's own
probe says the same (`tests/security/assault4_actor_handler_map_param_probe_v2.kry`,
header: "DELEGATION, not an escape").

## 4. Known limitations

### 4.1 Capability escapes known and still accepted

Measured 2026-11-02 with `bash tools/loop/escape_status.sh` on the in-place build
(12 minutes): **`STILL ESCAPING: 80    now-rejected: 180    missing: 0`**, 260
repros. Each row below is a program that `kryos check` accepts in default **and**
`--strict-capabilities` mode and whose `kryos run` printed the secret (executed;
`tools/loop/check-escapes-documented.sh` re-runs every row and fails if one is no
longer accepted in either mode, if its run prints no leak marker, or if this
table and the measurement disagree in either direction). The six rows of the
first version (64, 72, 77a, 77b, 112, 114) are closed: PR #39 closed 64, 72, 77a,
77b and 114, and 112 is rejected on this build. Items 119, 121, 122 and 123 (ten
rows) were open on master before this revision. This revision records the other
70 rows (items 124 to 134) on master: items 124 to 126 were found by the
independent verifier of the R3 wave (PR #39), the rest by the item-77 wave
(PR #41) and its two independent verifiers. That PR closes 31 of them on its own
branch (items 128, 130, 132 and 134), but both of its heads were refuted: by its
verifiers' reports, each accepted programs that master rejects, and each newly
rejected legitimate programs. It is deferred past 1.3.0, so the rows stay open here (LEDGER: the
release-freeze wave, and item 134).

<!-- known-escapes:begin -->
| id | repro | shape | root |
|---|---|---|---|
| 119a | tests/security/attack_slot_param_container_in_tuple.kry | a callee stores a closure into a container held inside a tuple parameter | R2 |
| 119b | tests/security/attack_slot_param_container_in_result.kry | the same with the container inside a Result parameter | R2 |
| 119c | tests/security/attack_slot_param_array_of_option.kry | the same with an array of Option closures (a[0] = Some(g)) | R2 |
| 119d | tests/security/attack_slot_param_option_of_nested_array.kry | the same with an Option holding a nested array | R2 |
| 119e | tests/security/attack_slot_param_tuple_of_two_arrays.kry | the same with a tuple of two arrays, the store going to the second | R2 |
| 121 | tests/security/attack_deny_coarse_family_row_blind.kry | deny!(fs), deny!(io) and deny!(all) do not reject a block whose closure reads a file; deny!(fs:read) does | row-layer deny matching |
| 122 | tests/security/attack_deny_trait_default_method_unchecked.kry | the body of a trait default method is never checked, so a deny! block in it is never judged | R4 (missing walk) |
| 123a | tests/security/attack_slot_param_fn_value_factory.kry | a store helper reached through a factory that returns the helper as a function value | R2 |
| 123b | tests/security/attack_slot_param_trait_bound_dispatch.kry | a store helper reached through a generic function with a trait bound | R2 |
| 123c | tests/security/attack_slot_dyn_registrar_store.kry | a store helper reached through dyn dispatch | R2 |
| 124a | tests/security/attack_fwd_param_chain_invoke_in_deny.kry | `tool(r)` inside deny! copies its parameter and hands it to `invoke`, declared below `tool` | F (forward reference) |
| 124b | tests/security/attack_fwd_param_chain_container_in_deny.kry | the same chain for an array of closures: `tool(fns)` hands it to `run_first` | F (forward reference) |
| 124c | tests/security/attack_fwd_param_chain_method_in_deny.kry | the same chain through an impl method, then a free function | F (forward reference) |
| 124d | tests/security/attack_fwd_param_chain_actor_in_deny.kry | the same chain through an actor handler, then a free function | F (forward reference) |
| 124e | tests/security/attack_fwd_rev_hotparam_forward_via_local_alias.kry | a pinned program (a parameter forwarded through a local alias) with its helpers declared in the reverse order | F (forward reference) |
| 124f | tests/security/attack_fwd_rev_slot_param_helper_chain.kry | a pinned program (a two-helper store chain) with the caller declared above its callee | F (forward reference) |
| 124g | tests/security/attack_fwd_mainfirst_verify_actor_to_actor_message.kry | a pinned program (a closure sent from actor to actor) with `main` declared first | F (forward reference) |
| 125a | tests/security/attack_fwd_store_relay_two_hop.kry | `h1` relays to `h2`, declared below it, which pushes the closure into the caller's array | F (forward reference) |
| 125b | tests/security/attack_fwd_store_relay_global_slot.kry | the same relay into a global array read by a function declared above `main` | F (forward reference) |
| 125c | tests/security/attack_fwd_store_relay_method.kry | the same relay through impl methods (`add` calls `put`) | F (forward reference) |
| 125d | tests/security/attack_fwd_builder_relay_returns_container.kry | `guard(b1(r))` where `b1` relays to `b2`, which returns `[safe, x]` | F (forward reference) |
| 125e | tests/security/attack_fwd_fill_relay_struct_field_after_main.kry | a struct field filled through `fill1` then `fill2`, both declared after `main` | F (forward reference) |
| 125f | tests/security/attack_fwd_fill_relay_struct_field_before_main.kry | the same with both helpers declared before `main` | F (forward reference) |
| 126a | tests/security/attack_deny_in_select_arm_callee.kry | a deny! block inside a `select` arm of a callee is never judged by the row layer | R4 (missing walk) |
| 126b | tests/security/attack_deny_in_select_arm_main.kry | the same block inside a `select` arm of `main` | R4 (missing walk) |
| 126c | tests/security/attack_deny_in_select_timeout_arm.kry | the same block inside the `timeout` arm of a `select` | R4 (missing walk) |
| 127a | tests/security/attack_shape_named_closure_forwards_captured_via_helper.kry | a top-level function returns a closure that forwards its captured parameter to a helper that calls it | shape layer (captured parameters) |
| 127b | tests/security/attack_shape_named_closure_two_captured_params.kry | a top-level function returns a closure that uses two captured parameters; only one is charged | shape layer (captured parameters) |
| 128 | tests/security/attack_shape_named_chain_applies_returned_closure.kry | `make_getter(reader)()`: the closure a top-level function returns is applied in the same expression | shape layer (chained call) |
| 129a | tests/security/attack_shape_loop_carried_rebound_callee.kry | `f = op(reader)` in a loop that rebinds `op` to a closure returning its parameter after the call | shape layer (map build order) |
| 129b | tests/security/attack_shape_loop_carried_alias.kry | `g = f` then `f = reader` in a loop | shape layer (map build order) |
| 129c | tests/security/attack_shape_lambda_let_replaces_earlier_binding.kry | a later lambda `let` of the same name replaces the entry of the earlier binding that is called | shape layer (map build order) |
| 130f | tests/security/attack_shape_local_hidden_return_match_stmt.kry | a local closure returns its parameter from a non-final `match` arm | shape layer (return walk) |
| 130g | tests/security/attack_named_fn_hidden_return_direct.kry | a top-level function returns its parameter from a non-final `match` arm | shape layer (return walk) |
| 130h | tests/security/attack_select_arm_return_plain_deny.kry | a local closure returns its parameter from a `select` arm in its own body; the deny! is in ordinary position | shape layer (return walk) |
| 130m | tests/security/attack_accessor_hidden_return.kry | a method with a hidden return is registered as a transparent accessor of `self.items[i]` | shape layer (return walk) |
| 131a | tests/security/attack_shape_local_returns_struct_field_in_place.kry | `mkh(reader).run()`: a fn-typed field of the result of a call through a local closure, invoked in place | shape layer (field of a call result) |
| 131b | tests/security/attack_shape_local_struct_passthrough_in_place.kry | `idh(Holder { run: reader }).run()`: a local closure returns its struct parameter | shape layer (field of a call result) |
| 131c | tests/security/attack_shape_local_result_bound_then_field.kry | the result is bound first: `let h = mkh(reader)`, then `h.run()` | shape layer (field of a call result) |
| 131d | tests/security/attack_shape_local_result_method_in_place.kry | `mkh(reader).go()`, where the method `go` calls the field | shape layer (field of a call result) |
| 131e | tests/security/attack_shape_local_result_nested_field_in_place.kry | `mko(reader).inner.run()`: a nested struct | shape layer (field of a call result) |
| 131f | tests/security/attack_shape_local_result_via_two_closures.kry | `wrap2(reader).run()`, where `wrap2` calls `mkh` | shape layer (field of a call result) |
| 132a | tests/security/attack_shape_container_param_shadows_array.kry | a closure's parameter named like an outer array holds the privileged closure; an element is read through a local identity closure | shape layer (container read) |
| 132b | tests/security/attack_shape_container_inner_let_shadows_array.kry | the same with an inner `let` of the array's name | shape layer (container read) |
| 132c | tests/security/attack_shape_container_registry_holds_privileged.kry | `f = id(reg.items[1])`: a struct registry holding the privileged closure, read through a local identity closure | shape layer (container read) |
| 132d | tests/security/attack_shape_container_lookup_over_privileged.kry | a local lookup closure over an array that holds the privileged closure: `f = get(1)` | shape layer (container read) |
| 132e | tests/security/attack_shape_container_param_element_via_id.kry | a function returns an element of its array parameter through a local identity closure: `pick([harmless, reader], 1)` | shape layer (container read) |
| 132f | tests/security/attack_shape_container_grown_by_callee.kry | a callee pushes the privileged closure into a pure array before an element is read through a local closure | shape layer (container read) |
| 132g | tests/security/attack_shape_container_alias_grown.kry | an alias of a pure array is grown with the privileged closure before an element is read through a local closure | shape layer (container read) |
| 133a | tests/security/attack_shape_name_collision_dead_if_block.kry | `f = id(reader)` calls the top-level `id`; a pure local closure named `id` sits in an `if false` block | shape layer (name lookup ignores scope) |
| 133b | tests/security/attack_shape_name_collision_live_if_block.kry | the same with an `if true` block | shape layer (name lookup ignores scope) |
| 133c | tests/security/attack_shape_name_collision_for_body.kry | the same with the local bound in a `for` body | shape layer (name lookup ignores scope) |
| 133d | tests/security/attack_shape_name_collision_reader_factory.kry | a pure local named like the reader factory shadows the factory itself | shape layer (name lookup ignores scope) |
| 133e | tests/security/attack_shape_name_collision_nested_fn_dead_block.kry | a nested `fn pick` in a dead block shadows the top-level `pick` that returns its second parameter | shape layer (name lookup ignores scope) |
| 133f | tests/security/attack_shape_name_collision_dead_closure_body.kry | the local is bound inside a closure that is never called | shape layer (name lookup ignores scope) |
| 133g | tests/security/attack_shape_name_collision_while_false.kry | the local is bound inside a `while false` body | shape layer (name lookup ignores scope) |
| 133h | tests/security/attack_shape_name_collision_hidden_return_fn.kry | the top-level function returns its parameter from a non-final `match` arm | shape layer (name lookup ignores scope) |
| 133i | tests/security/attack_shape_name_collision_generic_fn.kry | the top-level function is generic | shape layer (name lookup ignores scope) |
| 133j | tests/security/attack_shape_name_collision_bound_result.kry | the result is bound first: `let g = id(reader)`, then `f = g` | shape layer (name lookup ignores scope) |
| 133k | tests/security/attack_shape_name_collision_for_binder.kry | a `for` binder named `id` shadows a pure local closure `id` | shape layer (name lookup ignores scope) |
| 133l | tests/security/attack_shape_name_collision_match_binder.kry | a `match` binder `Some(id)` shadows a pure local closure `id` | shape layer (name lookup ignores scope) |
| 134a | tests/security/attack_shape_local_returns_param.kry | `f = setter(reader)` where the local closure `setter` returns its parameter (item 77a's shape) | shape layer (call through a local closure) |
| 134b | tests/security/attack_shape_nested_fn_returns_second_param.kry | a nested `fn` returns its second parameter (item 77b's shape) | shape layer (call through a local closure) |
| 134c | tests/security/attack_shape_local_returns_captured.kry | a local closure returns a captured privileged closure | shape layer (call through a local closure) |
| 134d | tests/security/attack_shape_local_returns_captured_rebound.kry | the captured local is rebound to the privileged closure after the closure is defined | shape layer (call through a local closure) |
| 134e | tests/security/attack_shape_local_returns_one_of_params.kry | a local closure returns one of several parameters, chosen at run time | shape layer (call through a local closure) |
| 134f | tests/security/attack_shape_local_two_levels.kry | two levels of local closures, applied through a local | shape layer (call through a local closure) |
| 134g | tests/security/attack_shape_local_two_levels_chain.kry | two levels applied in one chain: `mk(harmless)(reader)` | shape layer (call through a local closure) |
| 134h | tests/security/attack_shape_local_result_invoked_in_place.kry | the call result is invoked in place: `setter(reader)()` | shape layer (call through a local closure) |
| 134i | tests/security/attack_shape_local_result_let_init.kry | the call result initializes a `let` | shape layer (call through a local closure) |
| 134j | tests/security/attack_shape_local_result_in_struct_field.kry | the call result is stored in a struct field and called through it | shape layer (call through a local closure) |
| 134k | tests/security/attack_shape_local_calls_other_local.kry | a local closure returns the result of another local closure on its parameter | shape layer (call through a local closure) |
| 134l | tests/security/attack_shape_nested_fn_recursive.kry | a recursive nested `fn` returns its parameter | shape layer (call through a local closure) |
| 134m | tests/security/attack_shape_local_block_alias.kry | a block-bodied local closure binds its parameter to a local and returns the local | shape layer (call through a local closure) |
| 134n | tests/security/attack_shape_local_result_as_hot_arg.kry | the call result is handed to a function that calls it: `run_it(setter(reader))` | shape layer (call through a local closure) |
| 134o | tests/security/attack_shape_local_alias_of_local.kry | the local closure is reached through an alias | shape layer (call through a local closure) |
| 134p | tests/security/attack_shape_local_alias_of_named_fn.kry | a top-level function that returns its second parameter, reached through a local alias | shape layer (call through a local closure) |
| 134q | tests/security/attack_shape_local_two_bindings.kry | one local name bound to two closures, the second returning its parameter | shape layer (call through a local closure) |
| 134r | tests/security/attack_shape_named_getter_through_local.kry | the getter a top-level function returns is bound to a local and called: `let g = make_getter(reader)` | shape layer (call through a local closure) |
| 134s | tests/security/attack_shape_closure_in_closure_captured.kry | a local closure returns an inner closure that returns a captured privileged closure | shape layer (call through a local closure) |
<!-- known-escapes:end -->

What the 80 have in common, as observed in the 80 repros (not a proof about
unknown ones): in every repro the program holds the capability (`main` declares
`fs:read` and builds or receives the privileged closure itself), and what fails
is the **narrowing**: the privileged closure runs where `deny!` says it must not.
None of the 80 makes an unannotated, zero-authority program read a file
(section 4.5 lists shapes that do). They fall in two groups.

- **Row-layer gaps, 26 rows.** Items 119 and 123 (eight rows) are one mechanism:
  a helper stores a closure into a container it was handed and the caller's view
  of that container is not updated. 121 is coarse `deny!` families, 122 the body
  of a trait default method. 124 and 125 (13 rows) are forward references: a
  function declared above the callee it hands a closure to, or a store made by a
  callee declared below its caller. 126 (3 rows) is a `deny!` inside a `select`
  arm, which the row layer never judges.
- **Shape-layer gaps, 54 rows (items 127 to 134).** The shape
  layer (the E0507 check) mis-charges a call through a closure bound to a local
  (134), a name that a local closure or binder also uses elsewhere in the
  function (133), a chained call (128), a captured parameter (127), a loop that
  rebinds a closure name (129), a hidden `return` (130), a fn-typed field of a
  call result (131) or a container element handed through a local closure (132).
  53 of the 54 put the `deny!` block in a `select` arm (item 126), so the row
  layer does not judge it. For each of those 53 a twin with the arm's body in
  place of the `select` was generated for this revision (not committed), and
  every twin is rejected by the row layer in default and strict mode (E0110,
  executed). The 54th, 130h, uses an ordinary `deny!` and hides the `return` in
  a `select` arm of the closure instead.

A variant of a 119 or 123 repro that hands the closure to a callee declared
`@capabilities()` instead of calling it inside `deny!` is accepted and runs, but
that is delegation (N16, B6), not another escape: a callee's declaration never
bounded a closure it was handed, so it has no row here.

Per escape, what not to rely on, and the workaround. The only general
workaround is not to use `deny!` or a callee's declaration as the boundary
against code you did not write. The per-shape advice below only avoids the shape;
it does not make the check sound.

- **119a-e** (LEDGER item 119, root R2). Do not rely on `deny!` when a helper
  stores a closure into an array that sits inside a tuple, `Option`, `Result` or
  enum parameter. The flat form (`fn put(reg: [fn() -> str], g)` with `reg[0] = g`)
  is rejected on this build (row 112). Pass the container itself, not a wrapper
  around it, or have the helper return the new container and assign it at the
  call site (not executed for these shapes).
- **121** (item 121, row-layer deny matching). Write the precise capability in
  `deny!` (`deny!(fs:read)`, not `deny!(fs)`, `deny!(io)` or `deny!(all)`). The
  precise form of the same program is rejected (pinned: claims.tsv
  S41-121-fine-grained-deny-default).
- **122** (item 122, root R4). Do not put a `deny!` block in the default method of
  a trait; put it in an `impl` method, which is checked (the second half is not
  executed). A type error in a default method body is also accepted (section 4.4).
- **123a-c** (item 123, root R2). Do not rely on `deny!` when the store helper is
  reached through a function returned by a factory, a trait-bound generic or a
  `dyn` value.
- **124a-g, 125a-f** (items 124 and 125, root F, forward references). Do not
  rely on `deny!` across a chain of helpers declared out of dependency order: a
  function declared above the callee it hands a closure to, or a callee declared
  below its caller that stores into the caller's container. Declaring every
  callee above its caller avoids the shape: the dependency-order twins of the
  first four 124 probes and of all six 125 probes are rejected (LEDGER items 124
  and 125, measured on master `e2fbc46c` by the wave that recorded them; not
  re-executed for this revision).
- **126a-c** (item 126, root R4). Do not put a `deny!` block inside a `select`
  arm, the `timeout` arm included: the type checker does not walk them, so the
  row layer never judges the block. Put the block outside the `select` (126a and
  126b: the twins with the block out of the arm are rejected, E0110, executed).
- **127 to 134** (shape layer). The advice for 126 covers 53 of the 54 rows: with
  the block out of the `select` arm each was rejected (E0110, executed). For
  130h, also do not `return` a closure from inside a `select` arm. Beyond that,
  do not rely on the shape layer alone to see through a call to a closure bound
  to a local (134), a function name that a local closure, a nested `fn` or a
  `for` / `match` binder also uses anywhere in the same function (133), a
  chained call of a returned closure (128), a returned closure that uses a
  captured parameter through a helper or uses two of them (127), a loop that
  rebinds a closure name (129), a `return` in a non-final `match` arm (130), a
  fn-typed field of a call result (131) or a container element handed through a
  local closure (132). The candidate fix for 128, 130, 132 and 134 (PR #41) is
  deferred, and its verifiers found that it opens the 133 shape when the local
  closure needs authority of its own.

### 4.2 Precision cost: legitimate programs that need `@capabilities(all)`

Failing closed has a price (LEDGER item 41, deliberate, not fixed). Reading a
closure out of a container yields an unknown row, which erases to `all`, so
passing even a pure closure through some shapes demands `@capabilities(all)` of
the caller. Measured 2026-11-02 with `tests/capability_matrix_gate.sh` (gate,
PASS): **34 of 75** pure-closure controls compile, **41 are rejected** with
`E0507` ("requires capabilities [all] not granted to caller"). The count is the
same as at item 41's first measurement (2026-08-14). The R2 wave added a second
cost: a struct field's row is shared by every value of that struct in the
program, so a registry that mixes a pure and a privileged plugin is judged by the
union, and a `deny!` of that capability that only calls the pure one is rejected
(carried). A function with no declared capabilities cannot call a closure it
takes out of a container parameter (pinned: claims.tsv S42-container-param-default:
`@capabilities() fn run(fns: [fn() -> str]) { for f in fns { f() } }` is rejected
with `E0507` "call through a function value requires capabilities [all]"). A
plain `fn() -> str` or actor-handler parameter is the opposite case: it is
delegation and is accepted (N16). The cost is an over-rejection, never an escape;
the workaround is a more specific signature or `@capabilities(all)` on the
dispatcher, with the cascade that `tools/loop/ESCAPE-ROUTING.md` measured for
annotating stdlib dispatchers.

### 4.3 Deferred alternate roots R6 and R7

Two closures of the shape layer's fail-open defaults were prototyped and not
shipped, because each rejects a program the repository ships as the working
example (LEDGER, wave of 2026-10-15; the prototypes were reverted and are not
re-measured here):

- **R6**: an unresolvable receiver in `resolve_method_field_invoke_caps` returns
  the empty set. Failing closed there rejects `examples/showcase/repo_auditor.kry`
  (the `deny!(net:http)` registry-dispatch example) because the shape layer cannot
  tell which element a loop variable holds.
- **R7**: a deferred own-parameter charge is lowered to the empty set when no
  taker is registered. Failing closed rejects `ecosystem/kryos-tool-broker`.

Both programs are accepted today in default and strict (pinned for
`repo_auditor.kry`: claims.tsv S43-repo-auditor-default, -strict; `kryos-tool-broker`
pinned by `tests/security_gate.sh`, carried). The row layer (R2, R3) is the
intended way to close the same items without that cost; both waves are merged
(PR #38 as `732714ab`, PR #39 as `e2fbc46c`) and what remains of the row layer is
items 119, 121, 122 and 123 (4.1).

### 4.4 Other limitations verified for this document

- **Escapes found after this table was written** are added to
  `tools/loop/escape_status.sh` and to the table above. They do not block a
  release unless they are undocumented or reopen a closed repro (see
  `tools/loop/GRAPH.md`, failure policy).
- **`kryos audit`'s capability inventory lists declared annotations only**; its
  "Capability violations" section does run the inferred-mode checker (pinned:
  claims.tsv S1-audit-inferred, S1-audit-inventory).
- **Unannotated mains in permissive mode are unchecked** (N2); `kryos new`
  scaffolds `inferred`.
- **The stdlib directory is read from disk at check time** (section 1); the
  raw-memory exemption follows the directory, not a hash of its contents.
- **Latent ambient self-host intrinsics.** `syscall1`, `syscall2`, `syscall3`,
  `syscall6`, `mem_read_byte`, `mem_write_byte`, `mem_read_i64`, `mem_write_i64`,
  `mem_copy`, `str_data_ptr`, `str_byte_len` and `str_from_bytes` are registered
  by the type checker as ambient self-host intrinsics (LEDGER, "Found while
  fixing"; source for the list). `kryos check` accepts a zero-capability program
  that calls them (pinned: claims.tsv N3-latent-intrinsics-default and -strict, for
  `str_data_ptr`, `mem_read_byte`, `mem_write_byte`), and on this build `kryos
  run` of it fails to link (`LNK2001: unresolved external symbol mem_write_byte`
  and `LNK1120`, executed 2026-11-02; pinned as a nonzero status plus the
  symbol name `str_data_ptr`, the rest of the message is linker specific:
  claims.tsv N3-latent-intrinsics-run), so nothing
  runs. They are a limitation, not a bypass today: a toolchain that resolved the
  symbols (the self-hosted compiler's runtime does) would give a zero-capability
  program raw memory and syscalls, and the Rust toolchain's checker would still
  accept it. The other nine names were not executed.
- **The body of a trait default method is not type-checked** (LEDGER item 122,
  root of escape 122). `let x: i64 = "not an int"` in one is accepted in default
  and strict (pinned: claims.tsv S44-trait-default-body-unchecked-default, -strict).
  The capability checker does charge such a body: a default method calling
  `env_get` made its caller need `process` (executed 2026-11-02 on a scratch
  program, `E0507` in default and `E0505` in strict; not committed).

### 4.5 Accepted zero-authority shapes outside the escape corpus

These were found by reading the source and probing while the first version of
this document was reviewed (2026-11-01). They are not rows of
`tools/loop/escape_status.sh`, so the measurement in 4.1 does not see them. Each
has a committed repro under `tests/security/bypass_*.kry`, and
`tools/loop/check-escapes-documented.sh` re-runs every row and fails if one stops
behaving as documented (a fixed bypass must be removed from this table in the
same change) or if a `bypass_*.kry` file is not listed. "verified by": `runs`
means `kryos check` rc 0 in default **and** `--strict-capabilities`, and `kryos
run` printed `BYPASS-<id>-OK`; `inferred-only` means rc 0 in default and rc 1 in
strict, not run; `cache` meant the build sequence of the B8 repro, which was
deleted when PR #40 closed B8 (no row uses it any more). Binary: `kryos 1.2.0`, the 2026-11-02 build of `a9c4680b`;
executed 2026-11-02 for every row (B9 added by this revision), and re-run 2026-11-03
by `check-escapes-documented.sh` on the `kryos 1.3.0` build (same compiler and stdlib).

<!-- known-bypasses:begin -->
| id | repro | shape | verified by |
|---|---|---|---|
| B5 | tests/security/bypass_inferred_library_entry_no_main.kry | in default mode an unannotated pub fn in a file with no main (or an unannotated @test fn) is not a boundary | inferred-only |
| B6 | tests/security/bypass_delegation_zero_cap_callee.kry | by design: a closure parameter is charged to the sender, so a callee declared with no capabilities runs it | runs |
| B7 | tests/security/bypass_pure_mutates_argument.kry | @pure restricts calls only; it still mutates arguments and globals | runs |
| B9 | tests/security/bypass_stdlib_raw_address_field.kry | std::sync::AtomicInt publishes its raw heap address as a public i64 field and load/store dereference it: an integer chosen by a zero-capability program is read and written (own memory only; G6) | runs |
<!-- known-bypasses:end -->

**Closed in this release (PR #40, merged as `7cab5799`).** B1, B2, B3, B4 and B8
were re-executed on this build and are fixed: the five repros were deleted from
`tests/security/` and are no longer in the table above (`check-escapes-documented.sh`
fails on a stale row). Their pinned successors are the programs under
`tests/bypass/`, run by `bash tests/bypass_gate.sh` (run for this revision: PASS,
including its cargo section), and the claims B1-closed-*, B2-closed-*,
B3-closed-*, B4-closed-* in `tests/threat_model/claims.tsv`. The root fixed for
each, from the LEDGER:

- **B1** (an `impl` method named like a gated builtin made bare calls to it
  uncharged): `defined_fns` in `kryos-capabilities` now holds free functions only;
  a static call `Type::name(..)` on a declared type is that type's own method.
  The callee-side twin B1b (a fn-typed parameter named like a method, free
  function or builtin turned an `E0507` into an accepted program) is fixed with
  it.
- **B2** (the same with only a bodiless trait method declaration): the same fix.
- **B3** (`std::ffi` had no capability annotation and the stdlib is exempt from
  the raw-memory gate): every `std::ffi` export is `@capabilities(ffi)`, a user
  module that imports one is gated by the pre-merge raw-memory pass, and the
  stdlib helpers that dereference a caller-supplied address and `std::wasm`'s four
  network wrappers are gated too.
- **B4** (an `extern` naming a `kryos_*` native the table did not map needed no
  capability): `native_symbols.rs` classifies all runtime and stdlib-native
  symbols and an unclassified `kryos_*` name requires `ffi`.
- **B8** (`build --cache` served an artifact before the checker ran): the cache
  is consulted after the checker accepted the program in the mode of this build,
  and its key covers the capability mode, the strict flag and every imported
  module.

What not to rely on, and the workaround. B5, B6 and B7 are by design (B6 and B7)
or a deliberate scope choice (B5); B9 is an open defect.

- **B5.** Compile libraries and test files with `--strict-capabilities`, or give
  every entry point an explicit `@capabilities(..)`. `kryos test` runs unannotated
  `@test` functions with whatever they call (pinned: claims.tsv B5-test-default,
  B5-test-strict, B5-test-runs: an unannotated `@test` fn calling `env_get` passed
  `kryos check` and printed its marker under `kryos test`; `--strict-capabilities`
  rejected it).
- **B6.** Read any `fn`-typed or actor-handler parameter as carrying its
  sender's authority (N16). Do not use a callee's empty `@capabilities()` as a
  statement about what the callee may run.
- **B7.** `@pure` is a rule about calls. Do not read it as "no side effects".
- **B9.** Treat any use of `std::sync`, `std::chan`, `std::string` builders and
  `std::tensor` handles in code you did not write as if it held `ffi`: a struct
  literal of one of these types with a computed `i64` is a raw pointer. Review for
  struct literals of stdlib types with hand-written `i64` fields. No flag
  removes this.

## 5. How this document is kept true

Three scripts keep it true. Two of them run the binary and each starts with a
canary: `check-escapes-documented.sh` and `check-threat-model-claims.sh` refuse to
run on a toolchain that cannot build and run a pure program and reject a direct
`file_read` inside `deny!(fs:read)`, because a broken toolchain would otherwise
make every "accepted" check pass and every escape look "fixed".
`check-threat-model.sh` is a text check (required headings, README link, no
unqualified headline) and never runs the binary. The measured fact (2026-11-02, executed):
removing `kryos_rt.lib` alone from beside the binary does not trigger the canary,
because a pure program still links and runs, and a pure program also still ran
with `kryos_stdlib_native.lib` alone removed; removing both does (`LNK2001`,
`LNK1120`).

`bash tools/loop/check-escapes-documented.sh` is the acceptance for the graph
nodes `stage2-rows` and `escape-root` (`tools/loop/GRAPH.md`). It first runs the
canary, then runs `tools/loop/escape_status.sh` and fails unless the set of
accepted repros equals the set in the table between the `known-escapes` markers,
id for id and path for path. A stale entry (fixed but still listed), an
undocumented escape (accepted but not listed), a duplicate id, a mismatched repro
path, an accepted repro whose leak did not reproduce (no leak marker in its run
output), a row that `kryos check --strict-capabilities` rejects, or fewer than 260 measured
rows (`MIN_ROWS`: rows are only ever added, so a lower count means one was
deleted) each fail the check. It then re-runs every row of the `known-bypasses`
table (4.5) with the same binary and fails when a row no longer behaves as
documented, and when a `tests/security/bypass_*.kry` file is not listed. A fixed
escape or bypass must be removed from its table in the same change.

`bash tools/loop/check-threat-model-claims.sh` re-executes the verdicts this file
pins. `tests/threat_model/claims.tsv` has one line per claim (id, kryos command and
flags, program, expected exit status `0` or `nz`, an expected substring such as an
error code, and the guarantee it backs); the programs are under
`tests/threat_model/` or are existing repo programs. The script prints one `ok` or
`FAIL` line per claim and exits 1 on any `FAIL` or if fewer than 73 claims are
present (`MIN_CLAIMS`), and it runs the same canary as the escape check before the
first claim. It is the `accept` of the graph node `threat-model`, after
`tools/loop/check-threat-model.sh`, which checks that this file has the three
required headings and that the README links it. Non-vacuity, executed 2026-11-02:
with one expectation flipped in a one-line scratch manifest (G3-deny-direct-default
changed from `nz` to `0`, run with `--manifest`) the script printed `FAIL
G3-deny-direct-default`, `claims: 1, failed: 1` and exited 1; with both runtime
libraries renamed away it printed `check-threat-model-claims: FAIL -- canary:
kryos run of a pure program did not exit 0 and print its marker (rc=1)` followed by
`LNK2001 ... kryos_actor_wait_all` and `LNK1120: 60 unresolved externals`, ran no
claim, and exited 1 (both libraries were restored immediately and their hashes
re-checked).

What the checks cannot do: they cannot find an escape nobody has written down. The
corpus is a fixed list of names inside `escape_status.sh`; a program accepted by
`kryos check` that is in neither table is invisible to it (4.5 exists because
such programs were found). A claim is a verdict on one program in one mode, not a
property of the checker. The promise is "every known escape is documented with
a committed repro, and no closed escape that `escape_status.sh` measures reopens
undetected": `escape_status.sh`, through the check, re-measures all 180 closed
rows on every run, and `tests/security_gate.sh` (part of the `escape-root`
accept) additionally pins most of them as must-reject. By a literal search of
`tests/*.sh` for the repro stem (the file name without `.kry`), 52 of the 180
closed rows are named in no gate script (executed 2026-11-02; this ignores globs
and generated lists inside `security_gate.sh`, which were not checked), so for those the escape check may be
the only thing pinning them. It is not "there are no escapes". Not done: a sweep
that requires every `tests/security/*.kry` accepted by `kryos check` to be
classified as a control, a corpus row or a bypass row; on 2026-11-02
`tests/security/` held 587 `.kry` programs, 225 of them accepted by `kryos check` in the
default mode; 80 are corpus escapes, 4 are the `bypass_*` repros of 4.5, and 141
are in neither (mostly must-accept controls and delegation probes, not
individually classified).

Measurement recorded for this revision, in two steps, both on in-place builds
whose compiler and stdlib equal master `7cab5799`. First, on the binary built from
`a9c4680b` (clean tree) with `cargo build --release -j 8`, run on 2026-11-02:
`escape_status.sh` `STILL ESCAPING: 10  now-rejected: 180  missing: 0` (190 repros);
`tests/bypass_gate.sh` PASS (57 ok lines, including the cargo section);
`tests/authority_surface_gate.sh` PASS (82 builtins, 77 gated, 5 rejected without
a named capability); `tests/capability_matrix_gate.sh` PASS (0 escapes across 75
shapes in both modes, pure-closure controls accepted 34 of 75);
`tests/security_gate.sh` PASS (342 `ok` lines, 0 FAIL; the one environment-caused
FAIL recorded in the first version, "the real stdlib named by KRYOS_STDLIB_DIR is
still trusted", does not occur on a binary that sits in its build tree, as
predicted there);
`tools/loop/check-escapes-documented.sh` PASS (canaries ok; 10 documented escapes,
10 accepted, 0 undocumented, 0 stale; 4 documented bypasses, 4 verified, 0
problems); `tools/loop/check-threat-model-claims.sh` PASS (73 claims, 0 failed);
`tools/loop/check-threat-model.sh` PASS. These three were run on the tree that
became the commit containing this paragraph (the paragraph is the only
difference). Second, after the 70 rows of items 124 to 134 were recorded, on the
same binary (2026-11-02): `escape_status.sh` `STILL ESCAPING: 80  now-rejected:
180  missing: 0` (260 repros, 12 minutes); `tools/loop/check-escapes-documented.sh`
PASS (canaries ok; 80 documented escapes, 80 accepted, 0 undocumented, 0 stale; 4
documented bypasses, 4 verified, 0 problems; 16 minutes);
`tools/loop/check-docs-truth.sh` PASS (the README and the LEDGER preamble state
80); `tools/loop/check-routing-complete.sh` PASS;
`tools/loop/check-threat-model-claims.sh` PASS (73 claims, 0 failed);
`tools/loop/check-threat-model.sh` PASS. They ran on the tree with the rows, the
table and the counts in place; the edits made after them change wording only. The
gates of the first step were not re-run for the second, because none of them
reads the added programs. Third, after the revise round of the release PR, on the
`kryos 1.3.0` build (2026-11-03; same compiler and stdlib):
`tools/loop/check-escapes-documented.sh` PASS, which now also requires the
`--strict-capabilities` verdict of every documented row (canaries ok; `STILL
ESCAPING: 80  now-rejected: 180  missing: 0`; 80 documented escapes, 80 accepted,
0 undocumented, 0 stale; 4 documented bypasses, 4 verified, 0 problems; 20
minutes); `tools/loop/check-docs-truth.sh` PASS (it now also pins the README
capabilities bullet and the rejected counts); `tools/loop/check-routing-complete.sh`
PASS; `tools/loop/check-threat-model-claims.sh` PASS (73 claims, 0 failed);
`tools/loop/check-threat-model.sh` PASS. Not run for this revision: `tests/ir_signature_gate.sh` (part of the
`escape-root` accept), `tests/inferred_soundness.sh`, `tests/repl_capability_gate.sh`,
`tests/backend_divergence_pins.sh` and `tools/loop/graph-run.sh`, so the graph node
`escape-root` is not recorded green as a whole by this document.

## 6. Reporting

To report a capability escape, a way around any guarantee in section 2, or a
statement in this file that is false, follow [SECURITY.md](../SECURITY.md)
(private report to the address listed there, with a minimal `.kry` reproduction
and the output of `kryos --version`). Capability-system escapes are in scope.
