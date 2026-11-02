# Kryos capability threat model

What Kryos capabilities protect against, what they do not, and which known
holes are still open. Read this before treating `@capabilities`, `deny!` or
`@pure` as a security boundary.

Measured 2026-11-01 on `kryos 1.2.0` built from commit `dbfb989d` (the R2
slot-rows wave). Every statement below is tagged with how it was established:

- **executed**: a program was run through that binary on 2026-11-01 and the
  quoted verdict was observed;
- **gate**: a committed script pins it (named), and was run unless noted;
- **source**: read from code or docs, not executed;
- **not verified**: stated as a boundary of this document, not tested.

Nothing here is a soundness proof. The checker is heuristic code that has been
bypassed in five successive hardening rounds ([docs/10-capabilities.md](10-capabilities.md))
and in the escape corpus below; "no known escape" means "no escape found by the
searches that were run", and the searches were run by the same people who wrote
the checker.

"Known" has two precise meanings here. Section 4.1 is the escape corpus: 99
repros listed by name in `tools/loop/escape_status.sh`, 6 of them still accepted.
Section 4.5 lists accepted zero-authority shapes (B1 to B8) that were found by
reading the source and probing while this document was reviewed; none of them
is in that corpus. Both tables are checked by one script (section 5). Neither
table is a claim that nothing else exists.

## 1. Scope and attacker model

### What a capability is here

`@capabilities(fs:read)`, `deny!(net:http) { .. }` and `@pure` are **compile-time
checks of program text**. `kryos check` and `kryos run` run the checker and
refuse to run a program it rejects; `kryos build` does too, except when
`--cache` (or `KRYOS_CACHE=1`) returns a cached artifact before the checker
runs (section 4.5, B8). In the default mode the checker's entry points are
`main` and annotated functions (B5). The compiled program carries **no runtime
capability check**: there is no reference monitor, no syscall filter, no audit
log (`docs/10-capabilities.md`, source). `kryos audit` runs the checker in
inferred mode and prints its violations; its capability inventory lists
declared annotations only (executed: an unannotated `env_get` program printed
an `E0505` violation under "Capability violations").

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
| The runtime and native stdlib (`kryos-rt`, `kryos-stdlib-native`) | Compiled programs call into them; `grep -rE 'unsafe\s*\{' compiler/crates --include=*.rs` finds 467 `unsafe {` blocks on 2026-11-01 (executed; `docs/17-unsafe-audit.md` still says about 310, dated May 2026) | A memory-safety bug reachable from safe program text defeats compile-time checks at run time |
| The Kryos stdlib source (`compiler/stdlib/*.kry`) | It is built on raw-memory primitives, so the raw-memory gate exempts it (section 2, G6) | A malicious stdlib file has unrestricted raw memory access |
| The directory the toolchain resolves the stdlib from | Which directory counts as "the stdlib" decides what is exempt. The trusted search is, in order: `<exe_dir>/stdlib`, `<exe_dir>/../stdlib`, the `stdlib/` of the build tree (compile-time `CARGO_MANIFEST_DIR` ancestors), a `stdlib/` directory in any ancestor of the importing file, then `KRYOS_STDLIB_DIR` (`resolve.rs`, `find_trusted_stdlib_dir` and `find_stdlib_dir_impl`, source). While one of the first three exists, `KRYOS_STDLIB_DIR` changes where `use std::..` is read from but does not make that directory trusted (executed: with it pointing at another directory, the files in it were charged for raw memory; gate: `tests/security_gate.sh`). **When none of the first three exists (a binary run outside its distribution layout and outside its build tree, as CI does), a `stdlib/` directory above the importing file, or the `KRYOS_STDLIB_DIR` directory, is the trusted one** (source; not executed, because the first three steps could not be made to miss on the build used here) | Anyone who can write the toolchain's own stdlib directory, or, on such a layout, place a `stdlib/` directory above a project's files, can grant raw memory to any code |
| The person or host that invokes the compiler and chooses its flags | The mode (`--capabilities-mode`, `--strict-capabilities`, `[capabilities] mode` in the root `kryos.toml`) is chosen outside the program text | `permissive` turns most of the checking off (section 3, N2) |
| A host that embeds a compiled module or links `kryos-rt` | It supplies the process environment, the WASM imports, the filesystem | Capabilities do not describe or limit the host |
| The package registry index (`NORTHTEKDevs/kryos-registry`) and any explicit `git =` source, on first fetch | Checksums come from the same place as the code (section 3, N8) | A compromised index yields code with a matching checksum |

### Not trusted

- **The program text being checked**, including every `.kry` file it imports
  that is not the stdlib. This is the attacker-controlled input.
- **A dependency's source** (a user module, or a package fetched by `kryos pkg`).
  It is checked as program text. Executed for user modules; packages were not
  probed separately (not verified).

### What the attacker may do

Write any Kryos source that parses, including source that holds some
capabilities itself and tries to use them where `deny!` or a callee's declaration
says it must not. The attacker may not change the compiler, the flags, the
stdlib directory, the registry, or the host.

## 2. What capabilities DO guarantee

Each guarantee is for source accepted by `kryos check` with the stated mode.
**None of them is unconditional.** Each holds except for the six accepted
escapes in section 4.1 (they defeat `deny!` and a callee's narrowing) and the
eight zero-authority shapes in section 4.5 (B1 to B8), several of which defeat
G1, G2, G6, G7 and G9 outright; the exceptions are repeated under each
guarantee. "Default" is `inferred`: no flag, no project file. Error codes are
those printed by the binary above.

**G1. A gated builtin needs its capability, where it is called.**
A direct call to a gated builtin in a function that does not hold the capability
is rejected with `E0505` ("builtin `file_read` requires `fs:read` capability").

- Annotated function missing the capability: rejected in permissive, inferred
  and strict (executed: `@capabilities(net) fn main() { file_read("x.txt") }`).
- Unannotated `main`: rejected in inferred (default) and strict; accepted in
  permissive (executed: `fn main() { file_read("x.txt") }`, rc 1 / rc 1 / rc 0).
- **Exceptions.** B1 and B2 (section 4.5): defining any `impl` or `trait`
  method whose name equals a gated builtin makes every bare call to that
  builtin uncharged (executed in default and `--strict-capabilities`: a
  zero-authority `main` read the environment and a file, `check` rc 0). B5: in the default mode the
  boundary is `main` plus annotated functions, so an unannotated `pub fn` in a
  file with no `fn main`, and an unannotated `@test` fn run by `kryos test`,
  are accepted and not charged (rejected only with `--strict-capabilities`;
  executed).
- Which builtin needs which capability is a table in
  `kryos-capabilities/src/model.rs`. `tests/authority_surface_gate.sh` checks,
  for every classified builtin, that it is rejected with no capability and
  accepted with exactly the capability the diagnostic names (gate, 82
  builtins at last recorded run; not re-run for this document).
  `tests/inferred_soundness.sh` and `tests/strict_caps_examples.sh` pin the rest
  (gates, not re-run here). The file filter of the second one (every
  `examples/*.kry`, `examples/showcase/*.kry` and `examples/showcase/extra/*.kry`
  that is not `*_overreach.kry` and contains `fn main`) was run by hand on
  2026-11-01: `kryos check --strict-capabilities` accepted 102 of 102 files. The
  script itself was not run (it also kills `kryostokens.exe`).

**G2. Authority propagates up the call graph and never widens.**
A call to a function that needs a capability its caller does not hold is
rejected with `E0507` ("call to `inner` requires capabilities [fs:read] not
granted to caller"). Executed in default, strict and permissive for an annotated
caller (`@capabilities(fs:write) fn main() { inner() }` where `inner` is
`@capabilities(fs:read)`). In inferred mode an unannotated helper's authority is
the union of what it and its callees need, so `main` must declare it (executed:
helper calling `file_read`, `main` declaring only `fs:write`: `E0507` in
default, `E0505` on the helper in strict, accepted in permissive). **Exception:** B1 also
applies inside a callee: a callee that calls a builtin whose name is shadowed by
a method is not charged, so its caller needs no declaration for it.

**G3. `deny!(c) { .. }` rejects a block that uses `c`.**
A `deny!` block whose body uses a denied capability, directly or through calls
the checker resolves, is rejected with `E0110` ("capability `fs:read` is denied
in this block, but the block uses it"). Executed in default, strict and
permissive for the direct case. For closures reaching the block through
parameters, locals, containers, struct fields, actor state, `spawn`, generics,
`dyn` and the other shapes in section 2 of [10-capabilities.md](10-capabilities.md):
gate `tests/security_gate.sh` (224 `ok` lines in the run recorded in
section 5, plus one environment-caused FAIL there) and `tools/loop/escape_status.sh` (93 of
99 repros rejected). **Exceptions: the six shapes in 4.1, all of which defeat `deny!`.** B1 does not
defeat it (executed: a `deny!(process)` block calling `env_get` with a decoy
method of that name in scope is still rejected, `E0110`). `deny!` does not bound
a callee that is handed a closure (N16, B6).

**G4. `@pure` functions call no gated or impure builtin.**
Rejected with `E0110` ("`@pure` function cannot call non-pure function
`file_read`") in all three modes (executed). `@pure` restricts calls only: it does not
forbid mutating an argument container or module-level state (B7, executed: a
`@pure` function pushed onto its argument and incremented a global). No
dedicated gate was found for the call restriction.

**G5. Authority carried by a function value is charged at the call that invokes
it, including through containers.**
A call through a closure or fn-typed value is charged the authority the value
carries; when that cannot be shown to be a subset of what the caller holds it
requires `all` rather than nothing (fail closed). Evidence:
`tests/capability_matrix_gate.sh` enumerates 75 shapes (source x container x
transport); executed 2026-11-01 on the binary above: **75 of 75 attacks
rejected in default and strict**. The cost of failing closed is in 4.2.
**Exception:** the receiver of a function-typed or actor-handler parameter is not
charged; the sender is (N16, B6).

**G6. The raw-memory builtins need `ffi`, in every mode, at the call site in user
code. The `std::ffi` wrappers do not.**
`alloc`, `free_bytes`, `ptr_read_i64`, `ptr_write_i64`, `ptr_byte_at`,
`ptr_set_byte`, `str_to_ptr`, `arr_to_ptr` called from any non-stdlib function
are rejected with `E0505` ("requires `ffi` capability"), in permissive too
(executed: `str_to_ptr` + `ptr_byte_at` in an unannotated `main`, accepted with
`@capabilities(ffi)`). The requirement does not propagate out of the stdlib, so
a stdlib user needs no `ffi`. It also covers an unannotated helper and a user
module reached by `use`. Gate: `tests/security_gate.sh` sections 1, 1b, 2, 3
(`cap_escape_raw_memory*.kry`).

**Exceptions.** `std::ffi` (`compiler/stdlib/ffi.kry`: `cstr`, `read_i8` to
`read_i64`, `write_i8` to `write_i64`, `alloc`, `open`, `sym`, `call0` to `call6`,
and more) carries no `@capabilities`, so a zero-authority, unannotated `main`
that imports it reads and writes arbitrary process memory and calls arbitrary C
library functions (B3, executed in default and `--strict-capabilities`: `check` rc 0 in default and
strict, `kryos run` printed the bytes it read through `read_i8`; an earlier probe
called `system` through `call1` and its output appeared). `deny!(ffi)` does not
stop it. B1 also applies (executed: decoy methods named `str_to_ptr` and
`ptr_byte_at` let a zero-authority `main` recover a string by address, `check` rc 0
in default and strict).

**G7. `extern` blocks need `ffi`, except for `kryos_*` natives the capability table
does not map.**
`extern "C" { fn getpid() -> i32 }` is rejected with `E0506` (requires `ffi`)
in inferred and strict, and with `E0508` (not a `kryos_*` runtime symbol) in all
three (executed). No dedicated gate was found.

**Exception (B4).** An `extern` block naming a `kryos_*` runtime native that
`required_capability_for_native_symbol` (`kryos-capabilities/src/model.rs`) does
not map needs no capability: the table is a list of names, and its default arm
returns no requirement ("Genuine ambient plumbing"). Executed: `kryos_ffi_cstr`
plus `kryos_ffi_read_i8` in a zero-authority `main` (committed repro), and
`kryos_ffi_dlopen`, `kryos_ffi_dlsym`, `kryos_ffi_dlcall1` (a `system` call; not
committed, it is not portable), `check` rc 0 in default and strict; a
nonexistent `kryos_*` name is accepted by `check` and fails only at link.
`kryos_env_get` and `kryos_process_exec_simple` externs are rejected (`E0506`).

**G8. `unsafe { .. }` and `comptime { .. }` grant no authority.**
A `file_read` inside `unsafe { }` or `comptime { }` is charged to the enclosing
function like any other call (executed: both rejected in default, accepted in
permissive for an unannotated `main`). `comptime` does not evaluate at compile
time today (`docs/11-comptime.md`, source), so `kryos check` and `kryos build`
do not execute program text. `tests/security_gate.sh` pins `unsafe` blocks as
call-containing positions for the reassignment shapes (line 971 onward).

**G9. The same check gates run, build and the WASM target, except on a build-cache
hit and for the project manifest mode.**
One unannotated `file_read` program was rejected by `kryos check`, `kryos run`,
`kryos build` and `kryos build --backend wasm` (executed). The check runs in the
driver before backend selection (`pipeline.rs`, source). The REPL grants the
session only the capabilities passed with `--allow` (gate:
`tests/repl_capability_gate.sh`, not re-run here).

- **Cache (B8).** `kryos build --cache` and `KRYOS_CACHE=1` return a cached
  artifact before the checker runs (`pipeline.rs`, `build_cache.rs`). The key is
  the source text of the root file plus target, build mode, output type and
  compiler version; it omits the capability mode and the contents of imported
  modules. Executed: `kryos build` of an unannotated `env_get` program, `E0505`;
  then `build --capabilities-mode=permissive --cache`, rc 0; then `build --cache`
  in default mode **and** `build --strict-capabilities --cache`, both rc 0 with
  "cache: hit" (`-v`). `build --no-cache --strict-capabilities` is rejected. The
  cache directory is `KRYOS_CACHE_DIR`, `$XDG_CACHE_HOME/kryos` or
  `$HOME/.cache/kryos`, shared by every project of the user.
- **Manifest mode.** Only project-mode `kryos build` (no file argument) reads
  `[capabilities] mode` from `kryos.toml`. Executed with `mode = "permissive"`
  and an unannotated `env_get`: `kryos check` rc 1, `kryos run` rc 1, `kryos build`
  rc 0. `check` and `run` are the stricter ones there.

**G10. Stdlib modules carry their authority to the importer.**
`use std::fs::{exists}` in an unannotated `main` is rejected with `E0507`
("requires capabilities [fs:read]"); with `@capabilities(fs:read)` on `main` it
checks and runs (executed).

## 3. What capabilities do NOT guarantee

**N1. Nothing is enforced at run time.** See section 1. A compiled binary can be
copied, patched or built by another compiler; none of it is checked.

**N2. `--capabilities-mode=permissive` checks only annotated functions.** An
unannotated function is unconstrained (executed, G1). The mode is chosen by
whoever invokes the compiler, or by `[capabilities] mode` in the root
`kryos.toml` ("honoured when the flag is unset", `kryos build --help`). Only
project-mode `kryos build` reads the manifest (G9). A host
that compiles untrusted source must pass the mode itself and must not read it
from a manifest the source controls. Whether a dependency's own `kryos.toml` can
lower the mode: not verified.

**N3. Some authority is ambient and never charged.**
`println`, `print`, `eprintln`, `exit`, `abort`, `time_now`, `time_millis`,
`sleep`, `args()` and `read_line()` need no capability (source: `model.rs`;
executed: one program calling `args()`, `time_now()` and `sleep(1)`
and another calling `read_line()` both pass default check). Consequences: any data a program is allowed
to read can leave through stdout or stderr; program arguments and standard input
are readable by everything; timing and stalling are not bounded. `Capability::Time`
exists in the enum and is not enforced (`model.rs`, source). `file_exists` is
not ambient: it needs `fs:read` (executed).

Also accepted by `kryos check` with no capability (executed): `str_data_ptr`,
`mem_read_byte` and `mem_write_byte`, which read and write memory by address.
On the Rust toolchain they do not link (`kryos run` of a program using them:
`check` rc 0, then `LNK1120 unresolved externals`), so they are inert there. The
self-hosted compiler (`compiler/self-host`) uses them in its own runtime source
(`runtime.kry`), and contains no capability checker (source: a grep for
"capabilit" in `compiler/self-host/*.kry` finds only annotations on its own
programs); any program built with it is outside this document.

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
may be declared as `extern` (`docs/13-ffi.md`, source). C-library FFI through an `extern` block with a non-`kryos_*` name is rejected
(`E0508`, G7); through `std::ffi` (B3) and through unmapped `kryos_ffi_*` natives
(B4) it is not, and needs no capability. The `E0508` rejection is a limitation of
the compiler, not a security property: the check is "not a `kryos_*` name".

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
`spawn_process` need `process` (executed for `env_get`). `args()` does not (N3).

**N10. Randomness and time.** `random_bytes` needs `crypto` (source). `std::random`
is a deterministic, seedable PRNG and is ambient (source). Time is ambient (N3).

**N11. Resource exhaustion.** An infinite loop passes `kryos check` with no
declaration (executed: `while true { }`). `@budget(tokens = N, calls = M, usd = X)`
wraps a function in a runtime budget frame that **`std::llm` calls charge**
(`kryos-mir/src/lower.rs`, `inject_budget_frames`, source). It does not bound
CPU, memory, disk, sockets, `spawn`, or any non-`std::llm` call. `@budget` is not
a capability and nothing checks that a function calling the LLM stdlib carries
one.

**N12. Side channels** (timing, cache, error text, output size) are not
addressed. Not verified.

**N13. Backends.** The capability check runs once, before backend choice
(G9, source), so `kryos run` and `kryos build` accept and reject the same
programs (apart from the cache and the manifest mode, G9). `kryos run` is not an
in-process JIT: it compiles with the Cranelift backend in debug mode, links an
executable under the temp directory and starts it as a child process
(`commands/run.rs`, source; executed: a link error aborts `run`). That the
backends then *execute* the same authority is not established: documented value divergences between
backends exist and are pinned by `tests/backend_divergence_pins.sh`, and none was
analyzed here for authority impact. Not verified.

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
in principle". Treat it as a declaration, not a boundary; B1 to B4 mean a DLL
built from zero-authority source can still reach authority.

**N15. Zero known escapes is not a soundness proof.** The corpus is 99 repros
found by directed search; 75 more shapes are enumerated by a generator. Both
were written by the authors of the checker. Section 4.5 lists eight accepted
shapes that are in neither; they were found by reading the source and probing
while this document was reviewed, which is itself evidence that the corpus is
incomplete. Every previous "zero escapes" claim
in this repository was later wrong: the README claimed zero while twelve were
live (2026-08-10) and again before the 2026-10-15 wave added eighteen.

**N16. Authority travels with a closure (delegation).** A function-typed
parameter, and an actor-handler parameter, is charged to the *sender* at the
call that passes it; the receiver's declared set bounds nothing. A callee
declared `@capabilities()` runs a closure that needs `process` when a caller
holding `process` hands it over (B6, executed: `check` rc 0 in default and
strict, `kryos run` printed the marker). So `deny!` and a callee's
`@capabilities(..)` narrow direct and tracked calls only, never a closure the
callee is handed. The repository's own probe says the same
(`tests/security/assault4_actor_handler_map_param_probe_v2.kry`, header:
"DELEGATION, not an escape").

## 4. Known limitations

### 4.1 Capability escapes known and still accepted

Measured 2026-11-01 with `KRYOS_BIN=<kryos 1.2.0 built from dbfb989d>
bash tools/loop/escape_status.sh`: **`STILL ESCAPING: 6    now-rejected: 93    missing: 0`**.
Each row below is a program that `kryos check` accepts in default **and**
`--strict-capabilities` mode and whose `kryos run` printed the secret (executed).
`tools/loop/check-escapes-documented.sh` fails if this table and the measurement
disagree in either direction.

<!-- known-escapes:begin -->
| id | repro | shape | root |
|---|---|---|---|
| 64 | tests/security/attack_deny_double_indirection_wrapper_local.kry | a wrapper lambda around a local that holds a fn-typed parameter, called inside deny! | R3 |
| 72 | tests/security/assault_r1_real_program_plugin_platform_nopool.kry | a deny! block is judged before a later declaration stores the privileged closure | R3 |
| 77a | tests/security/attack_reassign_local_via_local_closure_return_lambda.kry | a fn-typed local reassigned from a local lambda that returns its parameter | shape layer |
| 77b | tests/security/attack_reassign_local_via_local_closure_return_nested_fn.kry | the same through a nested named fn | shape layer |
| 112 | tests/security/attack_slot_param_container_store.kry | a callee stores a closure into a container it was handed | R2 follow-up |
| 114 | tests/security/attack_slot_register_after_main.kry | a registration helper declared after the function that calls it | R2 follow-up |
<!-- known-escapes:end -->

What the six have in common, as observed in the six repros (not a proof about
unknown ones): in every repro the program holds the capability (`main` declares
`fs:read` and builds the privileged closure itself), and what fails is the
**narrowing**: the privileged closure runs where `deny!` says it must not. None
of the six makes an unannotated, zero-authority program read a file (section
4.5 lists shapes that do). A variant of 77a that hands the closure to a callee
declared `@capabilities()` instead of calling it inside `deny!` is also
accepted and runs, but that is delegation (N16, B6), not a seventh escape: a
callee's declaration never bounded a closure it was handed, so it has no row
here.

Per escape, what not to rely on, and the workaround. The only general
workaround is not to use `deny!` or a callee's declaration as the boundary
against code you did not write. The per-shape advice below only avoids the shape;
it does not make the check sound.

- **64** (LEDGER item 64, root R3). Do not rely on `deny!` when a lambda wraps a
  fn-typed parameter or a value derived from it and the lambda is invoked later,
  inside the block. The one-hop form (`go = || reader()`) is rejected. Invoke the
  parameter directly inside the block, or do not accept closures from callers in
  a function that narrows with `deny!`.
- **72** (item 72, root R3: verdict timing). `deny!` blocks inside an actor method
  are judged while the actor is checked, before `main`'s later stores. The same
  program with `main` declared above the actors is rejected (E0110, pinned as
  slot16). Declare the code that stores plugins before the code that runs them
  under `deny!`. Do not rely on it for a registry whose stores are declared later
  in the file than the block.
- **77a, 77b** (item 77, shape layer). Do not rely on `deny!` or a callee's
  declaration after `f = helper_returning_its_argument(reader)` where the helper
  is a local lambda or nested `fn`. A top-level function with the same shape is
  rejected. Use a top-level function.
- **112** (item 112). Do not rely on `deny!` when a helper stores a closure into
  an array, map or struct it received as an argument (`reg[0] = g`). Have the
  helper return the new container and assign it at the call site; that form
  (`reg = register(reg, name, f)`) is rejected when the helper is declared first
  (pinned: `tests/security/attack_slot_register_helper.kry`).
- **114** (item 114). Declare registration helpers above the functions that call
  them (the form above the call is rejected). Do not rely on file order alone.

### 4.2 Precision cost: legitimate programs that need `@capabilities(all)`

Failing closed has a price (LEDGER item 41, deliberate, not fixed). Reading a
closure out of a container yields an unknown row, which erases to `all`, so
passing even a pure closure through some shapes demands `@capabilities(all)` of
the caller. Measured 2026-11-01 with `tests/capability_matrix_gate.sh`:
**34 of 75** pure-closure controls compile, **41 are rejected** with `E0507`
("requires capabilities [all] not granted to caller"). The count is the same as
at item 41's first measurement (2026-08-14). The R2 wave added a second cost: a
struct field's row is shared by every value of that struct in the program, so a
registry that mixes a pure and a privileged plugin is judged by the union, and a
`deny!` of that capability that only calls the pure one is rejected. A function
with no declared capabilities cannot call a closure it takes out of a
container parameter (executed:
`@capabilities() fn run(fns: [fn() -> str]) { for f in fns { f() } }` is
rejected with `E0507` "call through a function value requires capabilities
[all]"). A plain `fn() -> str` or actor-handler parameter is the opposite case:
it is delegation and is accepted (N16). The cost is an over-rejection, never an escape; the workaround is a
more specific signature or `@capabilities(all)` on the dispatcher, with the
cascade that `tools/loop/ESCAPE-ROUTING.md` measured for annotating stdlib
dispatchers.

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

Both programs are accepted today in default and strict (executed for
`repo_auditor.kry`; pinned for both by `tests/security_gate.sh`). The row layer
(R2, R3) is the intended way to close the same items without that cost; R2 is
done, R3 is the open one (items 64 and 72).

### 4.4 Other limitations verified for this document

- **Escapes found after this table was written** are added to
  `tools/loop/escape_status.sh` and to the table above. They do not block a
  release unless they are undocumented or reopen a closed repro (see
  `tools/loop/GRAPH.md`, failure policy).
- **`kryos audit`'s capability inventory lists declared annotations only**; its
  "Capability violations" section does run the inferred-mode checker.
- **Unannotated mains in permissive mode are unchecked** (N2); `kryos new`
  scaffolds `inferred`.
- **The stdlib directory is read from disk at check time** (section 1); the
  raw-memory exemption follows the directory, not a hash of its contents.

### 4.5 Accepted zero-authority shapes outside the escape corpus

These were found by reading the source and probing while this document was
reviewed (2026-11-01). They are not rows of `tools/loop/escape_status.sh`, so the
measurement in 4.1 does not see them. Each has a committed repro under
`tests/security/bypass_*.kry`, and `tools/loop/check-escapes-documented.sh`
re-runs every row and fails if one stops behaving as documented (a fixed bypass
must be removed from this table in the same change) or if a `bypass_*.kry` file
is not listed. "verified by": `runs` means `kryos check` rc 0 in default **and**
`--strict-capabilities`, and `kryos run` printed `BYPASS-<id>-OK`;
`inferred-only` means rc 0 in default and rc 1 in strict, not run; `cache` means
the three-build sequence in B8. Binary: `kryos 1.2.0`, the `dbfb989d` measurement.

<!-- known-bypasses:begin -->
| id | repro | shape | verified by |
|---|---|---|---|
| B1 | tests/security/bypass_builtin_shadowed_by_method.kry | an impl method named like a gated builtin makes bare calls to that builtin uncharged | runs |
| B2 | tests/security/bypass_builtin_shadowed_by_trait_decl.kry | the same with only a trait method declaration, no impl | runs |
| B3 | tests/security/bypass_std_ffi_zero_cap.kry | std::ffi has no capability annotation: raw memory and C calls from a zero-authority main | runs |
| B4 | tests/security/bypass_extern_kryos_unmapped_native.kry | an extern block naming a kryos_* native the capability table does not map needs no ffi | runs |
| B5 | tests/security/bypass_inferred_library_entry_no_main.kry | in default mode an unannotated pub fn in a file with no main (or an unannotated @test fn) is not a boundary | inferred-only |
| B6 | tests/security/bypass_delegation_zero_cap_callee.kry | by design: a closure parameter is charged to the sender, so a callee declared with no capabilities runs it | runs |
| B7 | tests/security/bypass_pure_mutates_argument.kry | @pure restricts calls only; it still mutates arguments and globals | runs |
| B8 | tests/security/bypass_cache_skips_checker.kry | build --cache returns an artifact built in permissive mode to a default or strict build | cache |
<!-- known-bypasses:end -->

What not to rely on, and the workaround. B1 to B4 are compiler defects, not
design: closing them is a small checker or stdlib change (collect only free
functions; annotate `compiler/stdlib/ffi.kry` with `@capabilities(ffi)`; make the
native-symbol table fail closed), deliberately not made in this release. Until
then none of these is a boundary against source you did not write.

- **B1, B2** (`collect_defined_fns` in `kryos-capabilities/src/checker.rs` recurses
  into `impl` and `trait` bodies, though the comment above it justifies the
  shadowing rule only for a free function). Do not rely on E0505, E0507 or the
  raw-memory gate for code that may define a method named like a builtin
  (`env_get`, `file_read`, `file_write`, `str_to_ptr`, ...). Review for such
  names; `deny!` is not affected (G3).
- **B3, B4.** Treat `use std::ffi` and any `extern` block, even one that names
  only `kryos_*` symbols, as if it were `@capabilities(ffi)` when reviewing. The
  `ffi` declaration is not required for either.
- **B5.** Compile libraries and test files with `--strict-capabilities`, or give
  every entry point an explicit `@capabilities(..)`. `kryos test` runs unannotated
  `@test` functions with whatever they call (executed: a `tests/leak.kry` with an
  unannotated `@test` fn calling `env_get` passed `kryos check` and printed its
  marker under `kryos test`; `--strict-capabilities` rejected it).
- **B6.** Read any `fn`-typed or actor-handler parameter as carrying its
  sender's authority (N16). Do not use a callee's empty `@capabilities()` as a
  statement about what the callee may run.
- **B7.** `@pure` is a rule about calls. Do not read it as "no side effects".
- **B8.** Do not use `--cache` or `KRYOS_CACHE=1` for builds the checker must
  gate; use `--no-cache`, or a fresh `KRYOS_CACHE_DIR` per mode. The default
  mode and `--strict-capabilities` hit the same entry (executed).

## 5. How this document is kept true

`bash tools/loop/check-escapes-documented.sh` is the acceptance for the graph
nodes `stage2-rows` and `escape-root` (`tools/loop/GRAPH.md`). It first proves
the toolchain can build and run programs and can reject a gated call (a broken
toolchain would otherwise make every escape row look "fixed"), then runs
`tools/loop/escape_status.sh` and fails unless the set of accepted repros equals
the set in the table between the `known-escapes` markers, id for id and path for
path. A stale entry (fixed but still listed), an undocumented escape (accepted but
not listed), a duplicate id, a mismatched repro path, an accepted repro whose
leak did not reproduce, or fewer than 99 measured rows (`MIN_ROWS`: rows are only
ever added, so a lower count means one was deleted) each fail the check. It then
re-runs every row of the `known-bypasses` table (4.5) with the same binary and
fails when a row no longer behaves as documented, and when a
`tests/security/bypass_*.kry` file is not listed. A fixed escape or bypass must be
removed from its table in the same change. `tools/loop/check-threat-model.sh`
checks that this file has the three required headings and that the README links
it.

What the check cannot do: it cannot find an escape nobody has written down. The
corpus is a fixed list of names inside `escape_status.sh`; a program accepted by
`kryos check` that is in neither table is invisible to it (4.5 exists because
such programs were found). The promise is "every known escape is documented with
a committed repro, and no closed escape that `escape_status.sh` measures reopens
undetected": `escape_status.sh`, through the check, re-measures all 93 closed
rows on every run, and `tests/security_gate.sh` (part of the `escape-root`
accept) additionally pins most of them as must-reject (14 of the 93 closed rows
are named in no gate script, so the check is the only thing pinning them). It is
not "there are no escapes". Not done: a sweep that requires every
`tests/security/*.kry` accepted by `kryos check` to be classified as a control, a
corpus row or a bypass row; on 2026-11-01 `tests/security/` held 401
files, 127 of them accepted by `kryos check`; 6 are corpus escapes, 7 are the
`bypass_*` repros added with 4.5, and 114 are in neither (mostly must-accept
controls and delegation probes, not individually classified).

Measurement recorded for this revision. The binary first used for this document
(built from `dbfb989d`) is no longer available; the figures below marked "rerun"
were re-measured on a second `kryos 1.2.0` build whose six escaping ids are
exactly the six in 4.1 (its commit was not recorded, so the `dbfb989d` label in
the header belongs to the first binary). Rerun, one full
`check-escapes-documented.sh` with `KRYOS_BIN` set: canaries ok;
`escape_status.sh` 6 escaping / 93 rejected / 0 missing; 6 documented, 6 accepted,
0 undocumented, 0 stale; 8 documented bypasses, 8 verified; PASS.
`tests/capability_matrix_gate.sh` (rerun on the second binary) PASS: 0 escapes
across 75 enumerated shapes in both modes, pure-closure controls accepted 34 of 75.
Recorded on the first binary and not re-run: `tests/security_gate.sh` 224 `ok`
lines and 1 FAIL, run twice. The FAIL is the control "the real stdlib named by
KRYOS_STDLIB_DIR is still trusted". It fails because the binary used was copied
out of its build tree, so the directory it trusts as the stdlib (section 1) is not
this checkout's `compiler/stdlib`. The same check, run with the binary placed at
`compiler/bin_tmp/kryos.exe` so that `<exe_dir>/../stdlib` is this checkout's
stdlib, passed (rc 0). It was not re-run as part of the whole gate, so the gate as
a whole has not been recorded green for this revision; the graph node
`escape-root` should be verified with a binary built in place.

## 6. Reporting

To report a capability escape, a way around any guarantee in section 2, or a
statement in this file that is false, follow [SECURITY.md](../SECURITY.md)
(private report to the address listed there, with a minimal `.kry` reproduction
and the output of `kryos --version`). Capability-system escapes are in scope.
