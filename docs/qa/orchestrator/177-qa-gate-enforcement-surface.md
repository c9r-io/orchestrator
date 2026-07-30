---
lifecycle: active
related_fr: FR-127
self_referential_safe: true
---

# Orchestrator - QA Gate Enforcement Surface

**Module**: Governance / CI
**Scope**: enforcement classification of every `scripts/qa` gate — and, since FR-147, of every script outside `scripts/qa` that a workflow job executes — wiring truth, provider isolation, and stale governance claims
**Scenarios**: 5
**Priority**: High

## Background

The repository authored 46 QA gate scripts but wired only three into `.github/workflows/`. A newly written gate therefore defaulted to "only the author knows to run it", which is why FR-126 needed four successive audit rounds: each round found drift that an authored-but-unexecuted gate would have caught.

FR-127 makes enforcement status itself a governed artifact. `config/governance/qa-gate-surface.json` classifies every gate, and `scripts/qa/test-qa-gate-surface.sh` verifies the classification against the repository. The gate runs in the `governance` job of `.github/workflows/ci.yml`.

All scenarios here are read-only or operate on temporary copies. None starts a daemon, mutates the runtime database, or invokes a real provider.

Primary entry points:

```bash
./scripts/qa/test-qa-gate-surface.sh                 # verify the repository
./scripts/qa/test-qa-gate-surface.sh --fixture-test  # prove each check rejects a defect
```

---

## Scenario 1: Every Gate Is Classified, In Both Directions

### Preconditions

- `jq` and `rg` are installed.

### Steps

1. `./scripts/qa/test-qa-gate-surface.sh`
2. `jq '.scripts | length' config/governance/qa-gate-surface.json`
3. `ls scripts/qa/*.sh scripts/qa/*.rb | wc -l`

### Expected result

- Step 1 exits 0 and prints `FR-127 gate surface: 5 passed, 0 failed`.
- Steps 2 and 3 report the same count (53), and step 1's summary line reports `20 of 53 gates are ci-required`. FR-127 closed at 12 of 45; FR-128 added `test-governance-ledger-tooling.sh`, FR-129 the skill mirror gates, FR-130 `core-boundary.rb` with `test-core-boundary.sh`, FR-131 the publishing and link gates, and FR-132 `doc-lifecycle.rb` with `test-doc-lifecycle.sh`. The assertion that matters is that the two counts agree, not the value — which is why this document records the value as history rather than as an expectation. Read the value off the gate's own summary line rather than from this paragraph: it stood at `16 of 49` through the whole of FR-131 because a prose count is exactly the thing that goes stale.
- A script on disk with no manifest entry, and a manifest entry with no script on disk, both fail. Scenario 2 proves this rather than asserting it.

## Scenario 2: Each Check Rejects Its Own Defect

This is the scenario that distinguishes a gate that is enforced from a gate that merely looks enforced. Each fixture is asserted to fail **the check it targets** while every other check still passes on the same tree, so a fixture cannot pass by tripping an unrelated check.

### Steps

1. `./scripts/qa/test-qa-gate-surface.sh --fixture-test`
2. `git status --porcelain`

### Expected result

- Step 1 exits 0 and prints `FR-127 gate surface fixtures: 8 passed, 0 failed`, comprising a positive control plus seven isolated negative fixtures:

  | Fixture | Injected defect | Isolated to |
  |---|---|---|
  | 1 | an unclassified `scripts/qa` script | `check_surface_complete` |
  | 2 | a manifest entry whose script was deleted | `check_surface_complete` |
  | 3 | a `manual-runbook` entry with an empty `reason` | `check_reason_and_owner` |
  | 4 | a `ci-required` entry pointing at a job that does not run it | `check_wiring_truth` |
  | 5 | the `export PATH` shadow removed from the production parity gate | `check_provider_isolation` |
  | 6 | `binary: fake-*` removed from a pinned bundle | `check_provider_isolation` |
  | 7 | a document claiming CI enforcement for a `manual-runbook` gate | `check_no_stale_claims` |

- Step 2 prints nothing. The fixtures operate on copies under a temporary directory and never touch the working tree.

## Scenario 3: No ci-required Gate Can Reach A Real Provider

`fixtures/manifests/bundles/agent-driver-production-parity.yaml` declares `provider: claude` with no `binary:` override. The only thing standing between that gate and the real `claude` CLI is one line in `test-agent-driver-production-parity.sh`. Before FR-127, deleting that line left the suite green while silently routing execution through a real provider. This scenario checks the declared isolation statically and then confirms it empirically.

### Steps

1. `rg -n 'export PATH="\$QA_ROOT/bin:\$PATH"' scripts/qa/test-agent-driver-production-parity.sh`
2. `jq -r '.scripts[] | select(.enforcement == "ci-required") | "\(.providerIsolation.mode)\t\(.path)"' config/governance/qa-gate-surface.json`
3. Confirm fixture 5 of Scenario 2 reports `isolated to check_provider_isolation`.
4. Create a directory containing executables named `claude` and `codex` that print a diagnostic and `exit 97`, and prepend it to `PATH`.
5. Run every `ci-required` gate: `test-qa-gate-surface.sh`, `test-coordination-governance.sh`, `test-codex-session-resume.sh`, `test-legacy-coordination-decommission.sh`, `test-coordination-strangler.sh`, `test-filesystem-trigger.sh`, `test-agent-driver-production-parity.sh`, `test-slack-live-certification.sh`, `certify-slack-managed-live.sh status`, `qa-doc-lint.sh`, `test-governance-ledger-tooling.sh`, `test-skill-mirror-integrity.sh`, `test-core-boundary.sh`, and `FR126_FAST=1 test-agent-driver-execution-migration.sh`. As of FR-132 that list is joined by `test-docs-publishing-integrity.sh`, `test-markdown-link-integrity.sh` and `test-doc-lifecycle.sh`, giving 17 invocations for 20 manifest entries: `coordination-governance.rb`, `core-boundary.rb`, `doc-lifecycle.rb` and `test-agent-driver-documentation-alignment.sh` are `ci-required` through their declared `invokedBy` wrappers rather than directly, and `qa-doc-lint.sh` is the wrapper for the last of those rather than a `scripts/qa` entry itself. Derive the list from the manifest rather than from this paragraph — a later FR adds gates, and a hand-maintained enumeration is the thing that goes stale.
6. Search every captured log for the stub diagnostic.

### Expected result

- Step 1 finds the shadow line; step 2 lists a declared isolation mode for every `ci-required` gate with no `null`; step 3 confirms removing the shadow fails the isolation check specifically.
- All gates exit 0 with the stubs shadowing the real CLIs.
- No log contains the stub diagnostic, proving no gate invoked either provider rather than merely tolerating their absence.
- A gate that fails here depends on a real provider and its `ci-required` classification is wrong.

The `governance` job installs the same stubs via `GITHUB_PATH`, so an accidental real-provider invocation in CI becomes a visible failure with exit code 97 instead of silent quota spend.

## Scenario 4: The Wired Gates Actually Fail The Build

Structural wiring proves a step exists; it does not prove the step can fail. This scenario induces a real violation.

### Steps

1. Create `docs/design_doc/orchestrator/zz-fr127-gate-proof.md` containing one of the retired phrases from `STALE_PATTERN` in `scripts/qa/test-agent-driver-documentation-alignment.sh` — for example the one asserting that the removed `streaming` executor drives the Claude CLI. Copy it from that pattern list rather than from here: this document is itself scanned, so reproducing the literal phrase would make the gate fail on this file.
2. `git add` the file. The retired-semantics scan enumerates `git ls-files '*.md'`, so an untracked file is invisible to it; a real commit or PR stages the file.
3. `./scripts/qa-doc-lint.sh`
4. Remove the file and unstage it, then re-run `./scripts/qa-doc-lint.sh`.

### Expected result

- Step 3 exits 1, names the offending file and line, and prints `FAIL: retired runner or command-only authoring guidance remains` followed by `[qa-doc-lint] FAILED`.
- Step 4 exits 0.
- Known scope limit: the scan matches a curated list of literal retired phrases, not the general shape of the claim. A novel wording of the same retired semantics passes. FR-127 wires the existing assertion into CI; broadening it is FR-126 design territory and out of scope here.

## Scenario 5: No Document Claims Enforcement It Does Not Have

### Steps

1. `./scripts/qa/test-qa-gate-surface.sh` and read the fifth check.
2. `rg -n 'release gate' docs/design_doc/orchestrator/guide-alignment.md docs/design_doc/orchestrator/138-agent-driver-execution-migration.md`

### Expected result

- Step 1 prints `PASS: no document claims CI or release-gate enforcement for a gate that has none`.
- Step 2 shows no surviving claim of an FR-126 release gate in `release.yml`. That gate never existed there; `guide-alignment.md` now names the `governance` job in `.github/workflows/ci.yml`.
- Any non-`ci-required` gate named on a documentation line alongside CI or release-gate wording fails the check. Fixture 7 of Scenario 2 proves it.

---

## Certification conditions

A run of this document counts as closure evidence only when all of the following hold:

1. `git status --porcelain` is empty at start and at end.
2. `git rev-parse HEAD` matches before and after.
3. Each script is invoked as `bash <script> > log 2>&1` with `$?` captured directly, never through a pipe.
4. Each log ends with the script's own summary line.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Every gate is classified, in both directions | ☑ PASS | 2026-07-25 | Claude | Surface gate `5/5`; manifest and disk both report 53 gates, 20 of them `ci-required` (45 and 12 at FR-127 closure; FR-128, FR-129, FR-130, FR-131 and FR-132 each added gates). |
| 2 | Each check rejects its own defect | ☑ PASS | 2026-07-25 | Claude | Fixtures `8/8`; every defect was confirmed to fail its target check while the other four still passed on the same tree. Working tree byte-identical afterwards. |
| 3 | No ci-required gate can reach a real provider | ☑ PASS | 2026-07-25 | Claude | The 10 `ci-required` gates at FR-127's closure exited 0 behind `exit 97` stubs shadowing `claude` and `codex`, and no log contained the stub diagnostic. Re-run for the gates added since — `test-governance-ledger-tooling.sh`, `test-skill-mirror-integrity.sh`, `test-core-boundary.sh`, `test-coordination-strangler.sh`, and at FR-132 also `test-docs-publishing-integrity.sh`, `test-markdown-link-integrity.sh` and `test-doc-lifecycle.sh` — all exit 0 behind the same stubs with zero diagnostic hits. A gate promoted to `ci-required` is not covered by an earlier run of this scenario; it has to be swept when it lands. |
| 4 | The wired gates actually fail the build | ☑ PASS | 2026-07-25 | Claude | A staged Markdown file carrying a retired phrase made `qa-doc-lint.sh` exit 1 naming the file and line; removing it returned exit 0. An unstaged file does not trip the scan, which enumerates `git ls-files`. |
| 5 | No document claims enforcement it does not have | ☑ PASS | 2026-07-25 | Claude | Three stale claims corrected. The check found the third (DD-124) itself on its first run, after two had been found by hand. |
