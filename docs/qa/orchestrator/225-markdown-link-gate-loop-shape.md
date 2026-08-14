---
lifecycle: active
self_referential_safe: true
---

# Orchestrator - The Markdown Link Gate Produces A Verdict On macOS

**Module**: QA tooling / governance gates
**Scope**: that `test-markdown-link-integrity.sh` completes and prints a verdict on
bash 3.2 instead of dying of a fatal signal; that its extraction semantics did not move
when the loop shape changed; and that a failed extractor is now named rather than read as
"this file has no links"
**Scenarios**: 4
**Priority**: High

## Background

The gate is `ci-required` and exited **134** (SIGABRT) on macOS in the primary working
directory and **138** (SIGBUS) in a clean worktree at the same commit, with no summary
line. CI on `ubuntu-latest` was green throughout, so the tree's link integrity was never
in doubt — what was missing was the gate's own local verdict.

The cause was the **loop shape**, not the corpus: a shell function running an outer
read-loop that forks one process substitution per iteration, on bash 3.2. See
[DD-187](../../design_doc/orchestrator/187-markdown-link-gate-process-substitution-crash.md).

## Safety

Read-only. The gate scans tracked markdown, builds its fixture corpus under `$TMPDIR`,
starts no daemon and touches no database.

---

## Scenario 1: The gate completes and prints a verdict

**Steps**

```bash
bash scripts/qa/test-markdown-link-integrity.sh; echo $?
```

Run it three times — the crash was deterministic, so a single green run is weaker evidence
than three.

**Expected result**

Exit 0 each time, and the **summary line is present**:

```
=== markdown link integrity: 2 passed, 0 failed ===
```

The summary line is the assertion, not the exit code. A run that aborts before printing it
is truncated, and §4.6 condition 5 treats that as "the run did not happen" rather than as a
failure verdict.

## Scenario 2: The extraction semantics did not move

**Steps**

```bash
bash scripts/qa/test-markdown-link-integrity.sh --fixture-test; echo $?
```

**Expected result**

`=== fixtures: 13 passed, 0 failed ===`, exit 0.

This is the scenario that constrains the repair. The awk program is preserved byte for
byte precisely so these 13 fixtures — six negative, the rest positive controls asserting a
link is *not* broken — keep testing the extraction they were written against. Any repair
that rewrites the extraction (for example the single-pass `FILENAME`/`FNR` rewrite the
ticket originally proposed) has to re-earn all of them.

## Scenario 3: A failed extractor is named, not silently read as "no links"

**Steps**

Force `extract_links` to fail during the checks only — gating it on an environment
variable set just before `run_all_checks`, so the pre-check statistics, which run at script
scope under `set -e`, are unaffected and the check is isolated.

**Expected result**

The gate names the file and fails:

```
    .CLAUDE.md: link extraction failed; this file was not checked
  FAIL: check_link_targets_resolve
=== markdown link integrity: 1 passed, 1 failed ===
```

`done < <(producer)` structurally cannot do this — the subshell's status has nowhere to go,
so a broken extractor reads exactly like a file with no links (§4.4 shape 5). Measured
side by side on a one-file corpus containing one genuinely broken link, with the extractor
forced to fail:

| Loop form | Verdict |
|---|---|
| `done < <(extract_links …)` | **rc=0** — passes a file whose link is broken |
| `extract_links … > "$links"` then `done < "$links"` | rc=1, names the file |

The comparison uses a small corpus deliberately: on the real 671-file tree the old form
crashes before it can be compared.

## Scenario 4: The scratch directory is removed, and one trap covers both roots

**Steps**

```bash
before=$(ls "${TMPDIR:-/tmp}" | grep -c '^md-link-integrity\.' || true)
bash scripts/qa/test-markdown-link-integrity.sh >/dev/null 2>&1
bash scripts/qa/test-markdown-link-integrity.sh --fixture-test >/dev/null 2>&1
after=$(ls "${TMPDIR:-/tmp}" | grep -c '^md-link-integrity\.' || true)
[ "$before" = "$after" ] && echo "no scratch leaked"
```

**Expected result**

`no scratch leaked` — both modes clean up.

The fixture corpus now lives *inside* the same scratch root as the links file so that one
EXIT trap covers both. A second `trap … EXIT` **replaces** the first rather than adding to
it, so two scratch roots with two traps would have leaked whichever was registered first.

## Checklist

- [ ] Scenario 1: three consecutive runs exit 0 with the summary line present
- [ ] Scenario 2: `--fixture-test` reports 13 passed, 0 failed
- [ ] Scenario 3: a failed extractor is named and fails the check
- [ ] Scenario 3: the old process-substitution form is the one that returns 0 on a broken link
- [ ] Scenario 4: neither mode leaves a scratch directory behind

## Known limits

- The bash 3.2 defect itself is unfixed and unfixable here; what is fixed is this gate's
  exposure to it. Any other gate forking a process substitution per iteration **inside a
  function** over a few hundred items has the same exposure. None does today, and nothing
  enforces that.
- The ~251-iteration threshold is an observation, not a constant — it depends on the
  shell's state, so no test asserts against it.
