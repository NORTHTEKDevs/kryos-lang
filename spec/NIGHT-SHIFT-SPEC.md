# Night-Shift Spec -- Kryos close-out to v1.4.0

## Goal

Close every live defect in the Kryos 1.3.0 inventory on the integration branch `closeout/integration`
(worktree ~/projects/experiments/kryos-integ = master + the four closeout tracks merged), so the tree is
ready to release as v1.4.0. Done means the external acceptance check below exits 0: every finish-line
item passes on a fresh release build, every control still passes, and the full gate/oracle diff against
master shows no regression.

## Acceptance Check

```bash
bash C:/Users/Krist/projects/experiments/kryos-oracle/finish/accept.sh
```

accept.sh builds this tree (kbuild.sh), runs finish/run.sh (fail-fast, cheapest items first), and only
when every item passes runs the gate diff and the run-oracle diff. It lives OUTSIDE the repo on purpose.
Per-item status: `bash .../finish/run.sh C:/Users/Krist/projects/experiments/kryos-integ` (no --fast)
prints `PASS|FAIL <id> <reason>` for every item. Read finish/README.md for what each item checks.

## Order of work (priority field in features.json)

1. swa (silent wrong answers): A2, bare-result-option, 6, A8, 122-typecheck, A1, M1, M3, M6, A20, A5 ...
   A prior UNVERIFIED revision of the bare Result/Option work exists as
   kryos-oracle/swa_uncommitted_revision_2026-11-21.patch. It was abandoned mid-round: reuse only what
   you re-verify against finish/run.sh.
2. misc: RUN-TMP-1 (kryos run must link to a unique per-invocation path), parser/import items, docs items,
   crashes/ICEs (A7, A3, A9, 135, A22, ...).
3. cap: escape_status.sh must reach STILL ESCAPING 0 with every ctl_* control accepted. Items 124/125
   were reopened by the 2026-11-24 revert; read the "Revert wave: residual verdict" LEDGER entry first --
   its verifier findings are the wall.
4. mem: 137/138/139 and the A10-A19 leaks. Read the LEDGER entry "Wave: shared-ownership REVERTED"
   before writing any code. The required design: ONE ownership rule for shared handles in the MIR -- a
   handle slot owns exactly one reference, every copy retains, every overwrite and scope exit releases.
   Per-consumer-shape patches are forbidden (they failed twice). Perf budget (owner-approved
   2026-11-25): binary_trees release AOT within 1.10x of the budget in finish/README.md and bounded
   peak memory. A cheaper arc release (non-atomic count for arcs that never cross a thread, skipping
   bounded-drain bookkeeping on the shallow case) is in scope if needed to meet budget.
5. doc items (B5, B6, B7, 41, B-latent-intrinsics, B9): document as known limitations in STABILITY.md
   with id and workaround, exactly as finish/README.md specifies.

## Intended behaviour changes

Stage 3b compares every repo program's runtime output against master. A fix that corrects a wrong
answer changes master's output on purpose: list each such program in tools/loop/closeout_targets.txt
as `<path>  # <item id>: <why master's output was wrong>`. Never list a program to silence a change you
cannot explain; an independent verifier re-checks every line before release.

## Safety Rules

- Work ONLY in ~/projects/experiments/kryos-integ on branch closeout/integration. Never checkout, commit
  to, or reset any other branch or worktree (parallel sessions drive kryos-lang). Never push.
- Never edit anything under ~/projects/experiments/kryos-oracle (the check and its programs).
- Never delete, weaken, or skip an existing test or gate. Add tests; each new pin must FAIL on master
  (kryos-co-nov binary) and pass on this tree -- show both outputs in the commit message body.
- One feature per iteration. Before marking a feature passing: its run.sh line is PASS on a fresh build
  AND `bash run.sh <tree> --fast` shows no item that passed before now failing.
- If a fix needs a change you cannot make sound in this iteration, revert your partial edit (git checkout
  of the files you touched) rather than committing a half-fix, and record the wall in tools/loop/LEDGER.md.
- Every fix gets a LEDGER.md entry: item id, root cause, files, both-ways evidence, residual shapes you
  know are NOT covered. Never write DONE/closed for a shape you did not execute.
- Cargo is at ~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin (not on PATH). Build with
  `bash C:/Users/Krist/projects/experiments/kryos-oracle/kbuild.sh <tree>` (machine-wide build slots).
- Windows Defender quarantines some Kryos test exes (ML false positive, Bearfoos): a missing exe after a
  build is that, not your bug; re-run.
- `kryos run` uses a fixed temp path until RUN-TMP-1 is fixed: never run two same-name programs at once.
