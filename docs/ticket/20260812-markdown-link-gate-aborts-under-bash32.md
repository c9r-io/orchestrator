# test-markdown-link-integrity.sh aborts under bash 3.2 in a built working directory

- **Filed**: 2026-08-12, while certifying FR-165 requirement 2
- **Severity**: low for CI, medium for local certification — the gate produces no
  verdict at all, and an aborted run is not evidence in either direction
- **Gate**: `scripts/qa/test-markdown-link-integrity.sh` (ci-required, job
  `governance`)

## Symptom

In the primary working directory at `4600b683`:

```
$ bash scripts/qa/test-markdown-link-integrity.sh; echo $?
=== FR-131: markdown link integrity ===

files:      666 tracked markdown files
links:      542 inline link targets outside code spans and fenced blocks
exemptions: 0
134
```

`134` is `128 + SIGABRT`; the parent shell reports `Abort trap: 6`. Output stops
before `check_link_targets_resolve` prints anything, so neither check reports.
Deterministic across three consecutive runs.

## What it is not

Measured, in this order, because the first hypothesis was wrong twice:

| Hypothesis | Test | Result |
|---|---|---|
| the tree's content | same commit in a fresh `git worktree` | **exit 0** |
| the tree's content | tracked-files-only copy into a scratch git repo | **exit 0** |
| my markdown edits | same worktree with `git checkout e131c069 -- '*.md'` | exit 0 either way |
| a changed file count | `git ls-files '*.md'` at both revisions | 666 both |
| awk crashing on some file | ran the gate's own awk program over all 666 files, checking for signals | no signal on any file |
| a file-descriptor limit | `ulimit -n 4096` | still 134 |
| the `gate_jq_rows` loop | minimal reproducer, 666 iterations | completes |
| empty ignored directories | scratch copy plus empty `target/`, `gui/node_modules/` | exit 0 |

So: not the tracked tree, not the markdown, not the file count, not awk, not a
descriptor limit. The remaining difference between the primary directory and a
clean copy of the same commit is the ignored build output the directory carries —
`target/`, `gui/node_modules/` — with real content rather than empty. That
directory's `target/` grew during the session in which this first appeared
(`cargo test --workspace`, `cargo clippy --workspace --all-targets`), which fits
the timing: the same gate passed earlier in the same session.

## Why CI is probably unaffected, and why "probably" is the honest word

Only bash 3.2 is installed on the affected host, which is macOS's system bash. The
`governance` job runs on `ubuntu-latest` with bash 5.x, where this shape of malloc
abort is not known. But the repository maintains `scripts/qa/bash32-compat.rb`
precisely because bash 3.2 is a supported shell here, and the abort has not been
reduced to a specific construct, so the mechanism is not actually understood — only
its correlates are. Claiming CI is safe would be inferring a cause from a
correlation.

## Why it matters beyond one host

The failure mode is the one §4.4 shape 5 describes, arriving from the other
direction. A gate that aborts before its summary line prints has produced no
verdict, and:

- `bash script > log 2>&1; echo $?` reports 134, which a sweep records as a
  failure — fine, that is visible;
- but a sweep that reads the log's tail for a verdict finds the statistics banner
  (`files: 666`, `links: 542`, `exemptions: 0`) and no `FAIL` line, which reads
  like a healthy run.

The banner is printed before either check runs. That is worth fixing independently
of the abort: the statistics should print with the verdict, or the script should
emit an explicit incomplete marker, so a truncated run cannot be mistaken for a
clean one by anything that reads the log rather than the status.

## Suggested work

1. Reduce the abort to a construct. The candidates not yet eliminated are the
   nested process substitution in `check_link_targets_resolve`
   (`< <(extract_links ...)` inside `< <(git ls-files ...)`, once per file) and the
   `grep -qxF "$target" <<< "$exempt"` here-string. Both are bash 3.2 sore points.
2. Move the statistics banner to print with the verdict, or add an incomplete
   marker, so an aborted run is unambiguous in the log and not only in `$?`.
3. If the construct is confirmed bash-3.2-specific, either restructure it or state
   in `bash32-compat.rb`'s scope that this gate is exempt and why — as a named
   entry, never a subtree (§4.4 shape 8).

## Not blocking

FR-165 requirement 2 changed no markdown link and added no link exemption. The
gate passes at the certified commit in a clean checkout, which is what CI will
run. Recorded here rather than reported as green, because the run that aborted
asserted nothing.
