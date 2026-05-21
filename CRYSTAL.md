# kryos-lang — Project Crystal

## Stack
- Rust
- Kryos
- Cranelift
- LLVM
- GitHub Actions

## Key Files
- README.md - Overview of the Kryos language.
- CHANGELOG.md - Release notes for v0.2.1-v0.3.3.
- CONTRIBUTING.md - Guidelines for contributing to the project.
- install.ps1 - Installation script for Kryos.
- docs/WHY_KRYOS.md - Reasoning behind using Kryos.
- compiler/Cargo.toml - Build configuration for the Kryos compiler.
- compiler/crates/kryos-types/src/check.rs - Type checking logic.
- compiler/examples/hello.kry - Example program in Kryos.
- CONTINUE.md (deleted) - Old file.
- docs/stdlib/agent.md - Documentation for the agent module.
- docs/stdlib/chan.md - Documentation for the channel module.
- docs/stdlib/cost.md - Documentation for cost-related functions.
- docs/stdlib/fmt.md - Formatting utilities documentation.
- docs/stdlib/fs.md - File system operations documentation.
- docs/stdlib/http.md - HTTP functionality documentation.
- docs/stdlib/io.md - Input/output operations documentation.
- docs/stdlib/iter.md - Iterator related functionalities documentation.
- docs/stdlib/option.md - Option type documentation.
- docs/stdlib/os.md - Operating system related functions.
- docs/stdlib/path.md - Path manipulation documentation.
- docs/stdlib/probable.md - Probable values documentation.
- docs/stdlib/result.md - Result type documentation.
- docs/stdlib/stream.md - Stream handling documentation.
- docs/stdlib/sync.md - Synchronization utilities documentation.
- docs/stdlib/tensor.md - Tensor operations documentation.
- docs/stdlib/test.md - Testing framework documentation.
- docs/stdlib/tracked.md - Tracked types documentation.

## Patterns
- Self type in traits and Type::method() associated function syntax.
- StaticMethodCall parsed when ColonColon follows a bare identifier in expression position.
- Cranelift backend for dev (~500ms), LLVM for release (Rust perf parity).
- 3-stage bootstrap SHA-256 identity proof instead of 2-stage.

## Decisions
- ARC + move semantics chosen over borrow checker lifetimes for approachability.
- Self type resolves at impl site not declaration site.
- StaticMethodCall parsed when ColonColon follows a bare identifier in expression position.
- Cranelift backend for dev (~500ms), LLVM for release (Rust perf parity).
- 3-stage bootstrap SHA-256 identity proof instead of 2-stage.
- Deleted orphaned stub docs (auth, claude, config, db, email, server, stripe) rather than leaving them as aspirational.
- http.md replaces server.md -- merged client+server+router into one cohesive module doc to match the single http.kry source file.
- tensor.md documents handles as raw i64 (not a struct) because the FFI contract is the real API surface.
- agent.md exposes all alignment constants and lifecycle states as named integer constants rather than an enum, matching the source.
- chan.md uses shared keyword on is_closed/count fields to document atomic semantics.
- All docs use use std::module import syntax throughout -- not import std::module.

## Gotchas
- Debug builds use ~48GB RAM due to monomorphization -- always build with --release -j 4
- `cargo build --bin kryos` does NOT rebuild `target/release/kryos_rt.lib` (staticlib output skipped for --bin builds). Use `cargo build --release` (no --bin) when modifying runtime sources.
- `test_bootstrap.sh` historically swallowed per-module stderr via `out=$(...)`. As of 2026-05-20 it surfaces DOUBLE-FREE / panic / corrupt-array diagnostics on failure.
- Stage-1's runtime uses share-on-clone + no-op free (steps 37-41). Refcount infrastructure in place but `*_free` is no-op for self-compile reliability. Restoring real dealloc requires codegen retain-emission audit (RValue::Field, Operand::Local, function args).

## Self-host bootstrap (2026-05-20)
- Stage-0: `cargo build --release` produces `target/release/kryos.exe` (Rust).
- Stage-1: `./target/release/kryos.exe build self-host/main.kry -o target/bootstrap/kryos-stage1 --skip-ownership` produces a Kryos-compiled compiler.
- Verify: `bash self-host/test_bootstrap.sh` from `compiler/` → 16/16 modules.
- All 16 self-host modules compile deterministically with stage-1.
- 8/8 example programs compile + link + run end-to-end.
- 295+ workspace lib tests pass.
