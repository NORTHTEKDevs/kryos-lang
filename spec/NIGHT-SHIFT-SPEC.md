# Kryos Night-Shift Spec

## Goal (human terms)
Make Kryos **functional and adoptable**. Two thrusts:
1. **Prove the "governed agent is a portable embeddable unit" architecture**: ONE governed agent
   (calls a mock LLM, returns a `Tracked<str>` answer, annotated `@budget(usd=)` + `@capabilities(net:http)`)
   that runs unchanged in three hosts — a native Kryos program, a WASM host, and a non-Kryos host via the
   C ABI — each keeping the compile-time guarantees.
2. **Close real correctness bugs**, starting with in-place mutation of aggregates inside collections.

## Acceptance check (the only definition of DONE)
```
bash spec/night-acceptance.sh
```
Exit 0 == done. It runs: compiler build + full cargo suite (must stay green) + `demo/<mutation|native|wasm|cabi>/check.sh`.
Each milestone's `check.sh` is created by the work and must exit 0. Contracts are in `spec/features.json`.

## How to make progress each iteration
- Read `spec/features.json`, pick the FIRST feature whose `status` != "done", and advance it.
- Build small, verify with the milestone `check.sh`, then run the full acceptance check.
- A feature is done only when its `check.sh` exits 0 on a fresh run. Update only the `status` field in features.json.

## Hard rules (safety — do not violate)
- **Build-plane only.** NEVER run `gh repo edit ... --visibility`, NEVER flip any repo public, NEVER touch
  GitHub billing, NEVER `git push`. Local commits only (the harness commits for you).
- **Never weaken the gate.** Do not edit `spec/night-acceptance.sh` to make it pass, do not delete or skip
  tests, do not `--ignore` failures. Fix the code, not the check.
- **Mock LLM only.** No live API calls, no API keys, no network spend. The mock is a deterministic stub
  returning a canned answer + a fixed token/cost number.
- **Never regress the cargo suite** (currently 70 suites / 1064 tests, 0 failures). If a change reddens it, fix or revert.
- Do not edit `spec/features.json` except the `status` field. Do not edit this spec.

## Engineering notes (load-bearing — read before coding)
- Repo: `~/projects/active/kryos-lang`. Compiler in `compiler/` (Rust, 21 crates, Cranelift + LLVM backends).
  Build: `cd compiler && cargo build --release -j 4` (ALWAYS `--release`; debug uses ~48GB RAM).
  Binary: `compiler/target/release/kryos.exe`. Set `KRYOS_STDLIB_DIR=compiler/stdlib` when running it.
- Two backends: `kryos run <f>` = Cranelift JIT (fast, dev); `kryos build --release <f>` = LLVM AOT (production).
  Bugs to date were AOT-only aggregate mishandling (boxing/ABI). Prefer verifying BOTH backends for every demo.
- **Machine hygiene**: `kryostokens.exe` leaks and wedges builds. The acceptance check kills strays first;
  if a build hangs, `taskkill //F //IM kryostokens.exe //IM kryos.exe //IM clang.exe //IM link.exe` then retry.
- Kryos syntax gotchas live in the repo `CLAUDE.md` — read it. No semicolons; `elif` not `else if`; closures
  `|x|`; tuple-variant enums only; annotate `Result<T,E>`/`Option<T>` on signatures; literal braces `{{`/`}}`.
- The mutation fix root cause: `compiler/crates/kryos-mir/src/lower.rs` `lower_nested_field_assign` — the
  IndexAccess base hits the `_` fallback (mutates a throwaway copy, no write-back). Correct fix = in-place
  mutation through the element's box pointer (works for stack + heap). A naive `kryos_array_set` write-back
  crashes stack-promoted literal arrays — do NOT reintroduce that.
- Aggregate boxing convention learned this session: enum payloads / closure captures / array elements box
  with `kryos_calloc` (matching the `kryos_free` drop); closure aggregate ARGS forward as `ptr byval(%Agg)`.
  Reuse these patterns.

## End every response with exactly one sentinel
`<promise>NEXT</promise>` (more to do) / `<promise>DONE</promise>` (all gates pass) / `<promise>BLOCKED</promise>:<reason>`.
DONE is only real if `bash spec/night-acceptance.sh` exits 0.
