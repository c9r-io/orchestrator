# test-markdown-link-integrity.sh aborts with SIGABRT on macOS, so one ci-required gate cannot be certified locally

- **Observed during**: FR-163 pass-1 certification (2026-08-11), running the
  governance job's derived invocation list
- **Severity**: medium (not a product defect; it removes one ci-required gate
  from every local certification sweep, and a gate nobody can run locally is a
  gate whose local result is unknown rather than green)
- **Symptom**: `scripts/qa/test-markdown-link-integrity.sh` exits **134**
  (`Abort trap: 6`) after printing its three header lines and nothing else. The
  summary line never appears, so a sweep that reported the exit status at face
  value would be reporting a truncated run
- **Status**: open

## Mechanism (at `988e5f04`, re-verify)

Not caused by FR-163. Verified by re-running at `70c85cba` — the pre-FR-163
HEAD, in a clean detached worktree with the tree untouched — where it aborts
identically (`exit 134`, same three header lines). The counts differ only as the
tree differs (660 files / 525 links there, 661 / 529 here).

What is known:

- The abort is **reproducible**, not intermittent: four runs, four aborts.
- `bash -x` shows the last input reached is
  `docs/design_doc/orchestrator/qa-doctor-observability.md`, which is
  **unremarkable** — 65 lines, longest line 83 characters, no unusual link
  syntax. It is simply the last file the loop processes, so the abort lands at
  or after the end of the per-file pass rather than on that file's content.
- `awk version 20200816` (the BSD awk shipped with macOS). The per-file
  extraction is an `awk` program invoked once per tracked Markdown file, ~661
  times.
- The gate's own negative fixtures (`--fixture-test`) **pass**, so whatever
  aborts is in the full-tree path only.

Not yet established, and worth doing before repairing: whether this is BSD awk
specifically (compare against `gawk`), a resource ceiling reached after ~661
subprocess invocations, or something in a stage after the per-file loop. The
last file being ordinary argues against a content trigger.

## Why this matters beyond the one gate

`qa-gate-surface.json` classifies this as `ci-required`, and the fr-governance
skill's §4.6.6 requires a certification sweep to *derive* its gate list from
that manifest. A gate that cannot execute on the certifying machine is a hole in
every such sweep on macOS — and it fails loudly here only because the sweep
captured the true exit code. Piped into `tail`, this would have read as a pass.

## For ticket-fix

1. Reproduce: `bash scripts/qa/test-markdown-link-integrity.sh; echo $?` on
   macOS → 134, with no summary line. Confirm on Linux (or in CI logs) that the
   same revision passes there, which pins it as environment-specific.
2. Bisect the cause along the three hypotheses above. Cheapest discriminator:
   re-run the identical program under `gawk` if available; if that passes, it is
   the BSD awk build.
3. Classification is likely **bug in shared QA tooling**, not a product defect.
4. Whatever the repair, the gate should **fail closed with a named diagnostic**
   rather than abort: an abort and a clean run differ only by a missing summary
   line, which is §4.4 shape 7 — a truncated run reads exactly like a complete
   one.
