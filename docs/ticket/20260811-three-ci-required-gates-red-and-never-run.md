# Three ci-required gates are red and have never been run by CI

- **Observed during**: 2026-08-11 FR-164 governance certification sweep (53 derived invocations, 46 passed)
- **Severity**: high (these fail the `governance` job on the next push; the work that broke them is already committed)
- **Status**: open

## Symptom

Three `ci-required` gates fail at `04495a03`, and fail identically at `84569018`
— so they were **not** introduced by FR-164:

| Gate | Failure |
|---|---|
| `scripts/qa/test-persistence-dependency.sh` | ledger drift: `crates/orchestrator-persistence/src/attention_store.rs` sql `34 -> 37`, `crates/orchestrator-persistence/src/migration_steps.rs` sql `111 -> 112`, total `597 -> 601` |
| `scripts/qa/pipefail-short-circuit.rb` (and its fixture `test-pipefail-short-circuit.sh`, whose case 1 runs it against the working tree) | `scripts/qa/test-attention-routing-doc.sh:361` puts `head` in a pipeline under `set -o pipefail` — the FR-145 rule |
| `scripts/qa/test-persistence-extraction.sh` | case 6: "the baseline is not an ancestor of the first extraction commit" |

The first two are traceable to FR-162: `attention_store.rs` and
`migration_steps.rs` were last touched by `ea370d0d` / `8398ee1d` (FR-162 R3/R1)
without regenerating the persistence ledger, and `test-attention-routing-doc.sh`
is the doc-parity gate FR-162 itself created — it trips the pipefail rule that
FR-145 established.

## Why CI did not catch it

`origin/main` is `91419ac6`, which is also the last CI run (2026-08-10 20:04,
`success`). There are **14 unpushed commits** — all nine of FR-162's plus five
from FR-164. So this is not "main is red": these commits have never been
exercised by CI at all, and the `governance` job will fail on the first push.

That is the more useful finding. A closure certification that runs a derived
sweep locally is the only thing standing between a red gate and the push, and
FR-162's certification did not run these three.

## Reproduction

```bash
git rev-parse HEAD                       # pin; observed at 04495a03 and 84569018
./scripts/qa/test-persistence-dependency.sh; echo $?    # 1
./scripts/qa/pipefail-short-circuit.rb;      echo $?    # 1
./scripts/qa/test-persistence-extraction.sh; echo $?    # 1
```

Capture exit status directly, never through a pipe (`| head` reports the pager's
status — the FR-145/FR-146 defect operating on the reporter).

## For ticket-fix

1. Classification: Bug (three genuine gate failures), not a false positive. Each
   was reproduced at two pinned revisions.
2. `test-persistence-dependency.sh`: regenerate the derived half with
   `--emit-baseline`, **review the diff**, and confirm the new counts correspond
   to real SQL added by FR-162 rather than to a scanner change. A regenerated
   ledger that nobody read is a rubber stamp.
3. `pipefail-short-circuit.rb`: repair `test-attention-routing-doc.sh:361` per
   the gate's own advice (`sed -n '1,Np'` / `awk 'NR<=N'`, or capture and use
   `${out%%$'\n'*}` with no pipe at all). Then ask §4.4 shape 9's question of the
   repair: **which branch does that pipeline feed?** If the match feeds the
   failing branch, the defect was failing *open* — a violation reporting as
   clean — and whatever it was guarding needs re-checking, not just the pipeline.
4. `test-persistence-extraction.sh` case 6: determine whether the ancestry
   assertion is wrong or the baseline genuinely moved. Do not "fix" it by
   re-pointing the baseline until that question is answered — the assertion
   exists to catch exactly a silently re-pointed baseline.
5. Before closing, run the **derived** sweep, not a typed list:
   `jq -r '.scripts[] | select(.enforcement == "ci-required") | .path' config/governance/qa-gate-surface.json`
   reconciled against
   `ruby scripts/lib/workflow_model.rb run-commands .github/workflows/ci.yml governance`
   (paths are not invocations — some take arguments).

## Note on a fourth, non-failure

`scripts/qa/test-markdown-link-integrity.sh` exits **134 (SIGABRT)** on this
machine at both revisions, with a 168-byte log that stops after the header and
never prints its summary line. Under §4.6 condition 5 that is a *truncated run*,
not a failure verdict. `bash` here is macOS system 3.2.57; the `governance` job
runs on `ubuntu-latest` with bash 5. Treat as an environment artifact unless it
reproduces on Linux — but note that the repo maintains a bash 3.2 compatibility
gate, so a script that aborts under 3.2 may still be worth a look on its own.
