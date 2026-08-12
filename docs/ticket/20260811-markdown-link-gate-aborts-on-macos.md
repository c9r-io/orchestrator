# test-markdown-link-integrity.sh aborts with SIGABRT on macOS, so one ci-required gate cannot be certified locally

- **Observed during**: FR-163 pass-1 certification (2026-08-11), running the
  governance job's derived invocation list. Observed again independently during
  FR-165 requirement 2 certification (2026-08-12) — see "Second investigation"
- **Severity**: medium (not a product defect; it removes one ci-required gate
  from every local certification sweep, and a gate nobody can run locally is a
  gate whose local result is unknown rather than green)
- **Symptom**: `scripts/qa/test-markdown-link-integrity.sh` exits **134**
  (`Abort trap: 6`). Before `8f99560e` it printed its three header lines and
  nothing else; the summary line never appeared, so a sweep reading the log's tail
  rather than the exit status would have called it a pass
- **Status**: open (the abort). The truncated-log half is fixed — see
  "For ticket-fix" item 4

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

## Second investigation, 2026-08-12 at `af672d21` (671 files)

Found again from scratch during FR-165 certification, and filed as a separate
ticket before this one was noticed — that duplicate is deleted and its evidence
is folded in here. Two of its readings **disagree** with the section above and
neither is discarded.

### Where it aborts

Instrumented with an explicit counter to stderr, the abort is stable at
**iteration 251** of the 671-file outer loop, while processing
`docs/design_doc/orchestrator/agent-drain-enabled.md`.

That contradicts the `bash -x` reading above, which put the last input at the
*last* file of the loop. A counter written to stderr is the more reliable
instrument — a `bash -x` trace is buffered and its tail can outlive the abort —
so **iteration 251 is the reading to trust**, and "the abort lands at or after
the end of the per-file pass" should be treated as withdrawn rather than as a
second data point. 251 is close enough to 256 to suggest a fixed-size internal
table, but nothing below confirms which.

### Whether a clean worktree reproduces it

The section above reports the abort in a clean detached worktree at `70c85cba`.
On 2026-08-12 the opposite was measured at `af672d21`: **exit 0** both in a fresh
`git worktree` and in a tracked-files-only copy into a scratch git repository,
while the primary working directory aborts at the same commit.

Both observations are first-hand and they cannot both describe the same
behaviour. The differences between them are the revision and the scale — 660
files then, 671 now — which is what makes a count- or revision-sensitive trigger
the reconciliation worth testing. **The deleted ticket concluded from the clean
runs that the trigger was ignored build output in the primary directory. That
conclusion is not supported and is withdrawn**: the 100 GB `target/` in that
directory is the largest measured difference from a clean checkout, and no
mechanism connecting it to the abort was demonstrated — the gate never walks that
tree, and `[[ -e … ]]` is a stat.

### Hypotheses eliminated, and one that was not

Bisected by editing copies of the real gate in place:

| Hypothesis | Test | Result |
|---|---|---|
| the `grep -qxF "$target" <<< "$exempt"` here-string | that line replaced with `:` | still aborts |
| the per-file `exempt_targets_for` command substitution | replaced with `exempt=""` | still aborts |
| bash 3.2's `[[ =~ ]]` leak (a real, documented 3.2 defect) | `target_resolves`' regex rewritten as a `case` | still aborts |
| the volume being full (here-strings write temp files) | `df` | 277 GiB free |
| ~~the nested process substitution~~ | standalone loop of the same shape over the same 671 files | **does not clear it — see below** |

The nested-process-substitution row must not be read as an elimination. That
reproduction's inner command was `awk 'END{print NR"\tx"}'` — a trivial program —
whereas the gate invokes a ~1.5 KB multi-line awk program 671 times. The
reproduction therefore never exercised the real workload, and **the BSD awk
hypothesis named in the section above is untouched by any of this work**. If
anything the four bash-side eliminations point at it: every construct bisected
away is on the bash side, and the abort survived all of them.

### The cheapest discriminator has never been runnable here

"re-run the identical program under `gawk` if available" — `gawk` is **not
installed** on this host. `awk` is `/usr/bin/awk`, BSD `awk version 20200816`.
Item 2 below reads as though it were one command away and it is not; it needs
`brew install gawk` first.

### Third pass, 2026-08-13 — triaged in a ticket-fix sweep and deliberately not repaired

Re-checked the host preconditions rather than the abort, because the abort has
been characterised three times and the blockers have not:

- `gawk` is still **not installed**. Item 2 remains unrunnable without
  `brew install gawk`.
- `bash` is **3.2.57(1)-release** and it is the *only* bash on this machine —
  `/opt/homebrew/bin/bash` does not exist, so `which -a bash` returns one entry.
  The three bash-side hypotheses already bisected away were tested under 3.2 by
  necessity, not by choice, and none of them can be re-tested under bash 5 here.
  CI runs `ubuntu-latest` (bash 5) and has been green throughout, so the
  bash-version axis and the awk-implementation axis are still **confounded** on
  this host: no local run can separate them until one of the two is installed.

That is the whole reason this ticket is not being closed in the same pass as its
six siblings. Item 5's repair — collapsing the 671 per-file `awk` forks into one
`FILENAME`/`FNR` pass — is very likely correct and is a real simplification, but
it is a behaviour change to a `ci-required` gate whose 13 negative fixtures
encode the current extraction, and the machine that would measure the before and
after cannot currently run the discriminating experiment. Landing it here would
mean changing a release gate on a hypothesis this host cannot test.

**Unblocking it needs one of:** `brew install gawk` (separates BSD awk from
everything else in one run), or `brew install bash` (separates bash 3.2 from
everything else). Either turns item 2 from a plan into a measurement.

## Why this matters beyond the one gate

`qa-gate-surface.json` classifies this as `ci-required`, and the fr-governance
skill's §4.6.6 requires a certification sweep to *derive* its gate list from
that manifest. A gate that cannot execute on the certifying machine is a hole in
every such sweep on macOS — and it fails loudly here only because the sweep
captured the true exit code. Piped into `tail`, this would have read as a pass.

## For ticket-fix

1. Reproduce: `bash scripts/qa/test-markdown-link-integrity.sh; echo $?` on
   macOS → 134, with no summary line. Confirm on Linux (or in CI logs) that the
   same revision passes there, which pins it as environment-specific. CI on
   `ubuntu-latest` has been green throughout, most recently run `31569823881` at
   `af672d21`.
2. Bisect the cause along the three hypotheses above. Cheapest discriminator:
   re-run the identical program under `gawk` — which requires installing it
   first, see above. If that passes, it is the BSD awk build.
3. Classification is likely **bug in shared QA tooling**, not a product defect.
4. ~~Whatever the repair, the gate should **fail closed with a named diagnostic**
   rather than abort: an abort and a clean run differ only by a missing summary
   line, which is §4.4 shape 7 — a truncated run reads exactly like a complete
   one.~~ **Done at `8f99560e`.** The three statistics lines are computed before
   the checks and printed after them, with the verdict, so an aborted run's log is
   now just the title line. It no longer resembles a pass to anything reading the
   tail. The abort itself is unchanged and this ticket stays open for it.
5. **Suggested repair, not yet attempted**: replace the 671 per-file `awk` forks
   with a single pass using `FILENAME`/`FNR`. That removes the
   subprocess-accumulation hypothesis outright and is a genuine simplification.
   It is also a behaviour change to a `ci-required` gate whose 13 negative
   fixtures encode the current extraction, so it wants its own before/after
   measurement on both macOS and Linux rather than being folded into a
   record-keeping change.
