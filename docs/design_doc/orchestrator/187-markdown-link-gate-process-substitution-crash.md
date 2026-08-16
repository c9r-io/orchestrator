---
lifecycle: active
---

# DD-187: A Gate That Died Of Its Own Loop Shape, Not Of Its Input

**Status**: Released
**QA**: [225](../../qa/orchestrator/225-markdown-link-gate-loop-shape.md)
**Closes**: `docs/ticket/20260811-markdown-link-gate-aborts-on-macos.md` (four investigation passes, 2026-08-11 → 08-13)

## The problem

`scripts/qa/test-markdown-link-integrity.sh` is `ci-required` and, on macOS, exited with a
**fatal signal** instead of a verdict — 134 (SIGABRT) in the primary working directory, 138
(SIGBUS) in a clean `git worktree` at the same commit. CI on `ubuntu-latest` was green
throughout. A gate nobody can run locally has an unknown local result, not a green one, so
every local certification sweep in this repository was one gate short for three days.

Four passes narrowed it and left exactly one hypothesis standing — **BSD awk**, the only
component that sees the corpus — with a proposed repair of replacing the ~667 per-file
`awk` forks with a single `FILENAME`/`FNR` pass.

**That hypothesis was wrong.** Measured at `0ec05ef5`, 671 tracked markdown files,
bash 3.2.57.

## What it actually was

| Experiment | Result |
|---|---|
| the gate's exact awk program over the same 671 files, every bash construct removed, 3 runs | **0 failures** — awk eliminated |
| instrument the gate and count iterations | dies at iteration **251**, 3/3 |
| reverse the file order | dies at iteration **251** — *a different file* |
| the file it originally died on | 2,226 bytes, 46 lines, **zero** extracted links |
| remove jq / remove `target_resolves` / remove the `grep` here-string | still crashes (each) |
| replace the inner process substitution with a file | **survives** |
| the same loop in a brace group instead of a function | **survives** 671 iterations |
| shrink `extract_links` to `awk -f <file>`, everything else unchanged | **survives** |
| inner substitution calls `awk` directly, big `extract_links` merely *defined* | **still crashes** |

So the trigger is the **loop shape**, not the input: a shell *function* running an outer
read-loop that forks one process substitution per iteration, while the shell carries a
function whose body is a large inline literal. bash 3.2 copies that state on every fork and
falls over around iteration 251. The last row is the decisive one — the crash does not need
the big function to be *called*, only to *exist* — which is why every content hypothesis
was doomed and why the awk rewrite would have "worked" for the wrong reason.

This retrospectively explains the three anomalies the ticket could not:

- **Byte-identical markdown crashed on one tree and passed on another.** The shell state
  differed, not the markdown. The ticket's pass 4 built three trees to eliminate the
  corpus; reversing the file order does it in one run.
- **Two different fatal signals on identical input.** Memory corruption, not a resource
  ceiling — a fixed-size table would fail the same way each time.
- **It vanished under `bash -x`.** Tracing changes allocation and timing.

## The repair

`check_link_targets_resolve` reads its inner loop from a scratch file instead of
`< <(extract_links …)`. Two lines. The awk program is untouched **byte for byte**, so all
13 negative fixtures still test the extraction they were written against.

The ticket's own item 5 — one awk pass over all files via `FILENAME`/`FNR` — is **not**
taken. It would also have worked, by removing the per-iteration fork as a side effect, but
it is motivated by a falsified hypothesis and it rewrites the extraction that those 13
fixtures encode. A behaviour change to a `ci-required` gate needs a better reason than a
mechanism that turned out not to be the mechanism.

### The repair pays for itself twice

`done < <(producer)` **structurally cannot** report a failed producer: the subshell's exit
status has nowhere to go, so a broken extractor is indistinguishable from a file with no
links. That is §4.4 shape 5, and it is the entire rationale of this repository's own
`scripts/lib/gate_jq.sh`. Materialising makes the status observable, and the difference is
measured rather than argued — one file containing one genuinely broken link, with the
extractor forced to fail:

```
OLD form verdict: rc=0                                    # silently clean
    a.md: link extraction failed; this file was not checked
NEW form verdict: rc=1
```

The old form passes a file whose link is broken. On the real corpus that comparison cannot
even be run, because the old form crashes first — which is why the demonstration uses a
corpus below the ~251 threshold.

One EXIT trap now covers both the scratch file and the fixture corpus. Previously the
fixture path installed a second `trap … EXIT`, which **replaces** rather than adds; with
two scratch roots that would have leaked whichever was registered first.

## Known limits

- **The bash 3.2 defect itself is not fixed and cannot be here.** macOS ships 3.2.57 for
  licensing reasons and this repository is required to run under it
  (`scripts/qa/bash32-compat.rb`). What is fixed is this gate's exposure to it. Any other
  gate that forks a process substitution per iteration *inside a function* over a few
  hundred items has the same exposure; none does today, and nothing enforces that.
- **The threshold is approximate.** 251 was stable across every run measured here, but it
  is a function of the shell's state, not a constant — a different set of defined functions
  would move it. It is recorded as an observation, not a limit to test against.
- **Not reproduced in a minimal standalone script.** A loop with the same shape but a small
  function body survives 671 iterations, so the large-literal ingredient is necessary and
  the reduction stops at the real gate. A portable reproducer for an upstream report would
  need that ingredient isolated, which this did not pursue.
