---
lifecycle: active
related_fr: FR-152
---

# Orchestrator - The First-Run Path Is Executable, Modern, And Self-Explaining

**Module**: Documentation / Fixtures / Distribution
**Scope**: the README Quick Start and `docs/guide/01-quickstart.md` (EN/ZH)
against `fixtures/manifests/bundles/quickstart.yaml`; the Agent fixture
corpus driver ratchet (`core/src/fixture_driverless_tests.rs`); the
error-code glossary and its derivation gate
(`scripts/qa/test-error-code-glossary.sh`); the skills-install confinement
checks in `scripts/qa/test-release-publish-surface.sh`
**Scenarios**: 5
**Priority**: High

## Background

FR-152 (2026-08-01 audit): a new user's first run hit a nonexistent
`manifest.yaml` in the README, a quickstart teaching the deprecated
driverless Agent form, a fixture corpus modeling that form 137 documents
wide, bracketed machine error codes with no queryable entry point, and an
installer that wrote `.claude/skills/` into whatever directory `curl | sh`
ran from. Design record: `docs/design_doc/orchestrator/163-first-run-path-modernization.md`.

**Safety**: scenarios 2–5 are read-only against the repository (Rust tests
and gate scripts with private temp trees). Scenario 1 is the exception and
says so: it starts an isolated daemon with a redirected `HOME` and
`ORCHESTRATORD_DATA_DIR` in throwaway temp directories — never the
developer's own runtime database.

## Scenario 1: the README Quick Start runs end to end in a clean environment with zero legacy warnings (manual runbook)

Steps: in a repository checkout with release binaries built, execute the
README Quick Start block verbatim with the process environment redirected to
throwaway directories (`HOME`, `ORCHESTRATORD_DATA_DIR`), daemon in
foreground with two workers, ending at `orchestrator task logs <task_id>`
for the created task. Capture the full transcript and grep it for
`legacy_` and `Warning:`.

Expected result: every command exits 0; `apply` reports the three quickstart
resources; the task reaches a terminal state with the echo agent's output in
`task logs`; the transcript contains zero `[legacy_*]` markers and zero
apply warnings. Recorded run (2026-08-01, at `c61a5009`): all commands green,
`grep -c 'legacy_'` = 0 — the transcript excerpt is archived in this
document's appendix.

## Scenario 2: the quickstart bundle is permanently valid and warning-free

Steps:

```bash
cargo test -p agent-orchestrator --lib fixture_corpus 2>&1 | tail -3
```

Expected result: `test result: ok` including
`every_tracked_bundle_is_accepted_or_declared` (the bundle validates) and
`quickstart_bundle_applies_without_warnings` (dispatching all three
documents through the apply path's `collect_warnings` yields an empty list —
the assertion observes what apply would print, not how the YAML is spelled).

## Scenario 3: a new driverless Agent fixture cannot land

Steps:

```bash
cargo test -p agent-orchestrator --lib fixture_driverless 2>&1 | tail -3
```

Expected result: 6 tests pass. `every_agent_fixture_is_typed_or_exempt`
scans the git-derived yaml corpus (empty scan fails closed);
`a_commented_out_driver_block_is_a_violation` takes a typed Agent document
derived from the real corpus, comments out — does not delete — its driver
block, asserts the premise that the driver is gone, and requires a violation
naming that document; the remaining tests reject empty exemption reasons,
typed documents still carrying the exempt comment, and exclusions matching
zero files.

## Scenario 4: the error-code glossary equals the source-derived set, and the CLI points at it

Steps:

```bash
bash scripts/qa/test-error-code-glossary.sh --fixture-test
cargo test -p orchestrator-cli commands::resource 2>&1 | tail -3
cargo run -q -p orchestrator-cli -- guide error-codes | head -8
```

Expected result: the gate reports `7 passed, 0 failed` — the derived set (16
codes, three anchored extraction rules, `fs_watcher` excluded with a
staleness-checked reason) equals the EN glossary in both directions, ZH
equals EN, and three fixtures (commented-out entry, ghost entry, diverged
mirror) each fail with a diagnostic naming the code. The CLI unit tests for
`contains_bracketed_code` pass, and `guide error-codes` renders the
glossary pointer entry.

## Scenario 5: install.sh writes only its announced skills target

Steps:

```bash
bash scripts/qa/test-release-publish-surface.sh
bash scripts/qa/test-release-publish-surface.sh --fixture-test
```

Expected result: both report `6 passed, 0 failed`. The confinement check
runs the real install.sh against a stubbed local release in a scratch CWD
and asserts the CWD entry listing is identical before and after, the skill
lands in `$HOME/.claude/skills/orchestrator-guide`, and the target is
announced in the output; the override check asserts
`INSTALL_ORCHESTRATOR_SKILLS_DIR` redirects the install and `none` skips
it. Fixture 5 mutates the skills default back to the working directory and
must fail with a diagnostic naming the CWD pollution.

## Checklist

- [ ] README Quick Start, guide Step 5, and the corpus test all reference
      the same `fixtures/manifests/bundles/quickstart.yaml`, as real
      markdown links where prose allows
- [ ] the recorded clean-environment run shows zero `[legacy_*]` output
      through `task logs`
- [ ] the driverless ratchet's negative fixture mutates by commenting out
      and asserts its own premise before its verdict
- [ ] the glossary gate's derived set is produced by anchored rules with
      reasoned, staleness-checked exclusions — never a hand-typed list
- [ ] install.sh confinement is asserted on the filesystem (CWD listing
      diff), not on the script's text

## Appendix: recorded quickstart transcript (scenario 1)

Recorded 2026-08-01 at `c61a5009`, release binaries, scratch `HOME` and
`ORCHESTRATORD_DATA_DIR`; transcript grep: `legacy_` 0 occurrences,
`Warning:` 0 occurrences. Excerpt (item log lines repeat per QA target and
are truncated):

```text
$ orchestratord --foreground --workers 2 &
$ orchestrator init
Orchestrator initialized at <data_dir> (sqlite: <data_dir>/agent_orchestrator.db)
rc=0
$ orchestrator apply -f fixtures/manifests/bundles/quickstart.yaml
workspace/default created (project: default)
agent/echo_agent created (project: default)
workflow/simple_qa created (project: default)
configuration version: 1
rc=0
$ orchestrator task create --goal "My first QA run" --workflow simple_qa
# annotation: the README flow deliberately uses the default --project scope
Task enqueued: 3c8a29ac-f894-4e7f-9be1-bce045e323bb
rc=0
$ orchestrator task list
ID        NAME          STATUS   FINISHED FAILED
3c8a29ac  QA Sprint 20  running  0        0
rc=0
### final status: completed
$ orchestrator task logs 3c8a29ac-f894-4e7f-9be1-bce045e323bb
[cb042168-...][qa]
{"artifacts":[{"findings":[{"description":"no issues found","severity":"info",
"title":"all-good"}],"kind":"analysis"}],"confidence":0.95,"quality_score":0.9}
rc=0
```
