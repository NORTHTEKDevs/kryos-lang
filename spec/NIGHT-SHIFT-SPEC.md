# Night-Shift Spec -- kryos-lang

## Goal

Harden kryos-lang until it can be handed to the public with confidence. "Done"
means no known silent-wrong-answer or memory-corruption bugs, the capability
model actually attenuates, and the self-host compiler runs clean. The immediate
blocker is the last known corruption: parsing a nested binary expression
corrupts a LATER tokenize on the Cranelift backend.

## Acceptance Check

```bash
bash tests/acceptance.sh
```

Runs in ~230s. Three conditions, all required: the tier-1 gate ladder, the
security gate (capability attenuation), and the self-host nested-binop repro
printing `after parse: 31 tokens`. Manually verified to exit 1 today, failing
on exactly the third condition while the first two pass.

## Feature List

See `spec/features.json`. Ranked by seriousness for the intended use case
(capability-attenuated agent infrastructure), NOT by which gate is red:
trust-model hole > silent wrong answer > blocks CI > leak > papercut.

## Working method -- follow this, it is what has worked

Read `tools/loop/LEDGER.md` FIRST. It carries the ranked queue, what has
already been RULED OUT for each bug, and the measurement traps. Do not
re-derive a disproven theory; several are recorded specifically to stop that.

1. **Reproduce before theorising.** `tools/loop/kryos-loop.sh repro <file>`.
   Three attributions were wrong in one session from reading code instead of
   measuring it.
2. **Bisect mechanically** -- commit-level (`git cherry-pick` into a worktree)
   or program reduction. Never hand-edit a hypothesis in and treat the result
   as evidence; one such edit silently removed more than intended and produced
   a confident wrong answer.
3. **Prove both directions.** A test that cannot fail is not a test. Verify it
   fails without the fix.
4. **Run `tools/loop/kryos-loop.sh gates 2` before committing.** The acceptance
   check only runs tier 1 for speed; tier 2 must be green too.
5. **Update the LEDGER in the same commit**, including anything newly ruled out.

## Known measurement traps (each cost real time)

- `cargo build -p kryos-cli` does NOT regenerate the staticlib archives an
  AOT program links. Runtime edits are invisible until a full
  `cargo build --release`. Run `kryos-loop.sh preflight` first, every time.
- Bootstrap fails spuriously with rc=127 on rotating modules under load. Only a
  failure reproduced on a SOLO run is real. It is excluded from the acceptance
  check for this reason.
- A single peak-RSS reading is noise. Repeat it three times before believing a
  leak number -- an "82% reduction" this session was a sampling artifact.
- `KRYOS_FREE_DIAG=1` completing while the program normally crashes means the
  crash IS memory corruption, not the reported error.
- `kryos_string_clone` is a refcount bump returning the SAME pointer, not a
  deep copy.

## Safety rules

- NEVER delete or weaken a test to make the check pass. Making a gate unable to
  fail is the worst possible outcome here.
- NEVER modify `tests/acceptance.sh`, this spec, or `spec/features.json`
  except the `status` field.
- NEVER revert the security gate or the raw-memory capability gating.
- NEVER force-push. Commit and push to `master` normally.
- If a change does not fix its target, REVERT it rather than leaving it in.
- Prefer a truthful BLOCKED over a green achieved by lowering the bar.

## Test command

```bash
bash tests/acceptance.sh
```
