---
self_referential_safe: true
---

# Orchestrator - Skill Single Source And Mirror Integrity

**Module**: Governance / CI
**Scope**: mirror completeness and shape across every declared root, the readability of every mirrored `SKILL.md`, the mirror policy's freshness, and the removal of the unproduced third copy
**Scenarios**: 5
**Priority**: High

## Background

`.claude/skills/` is the authoritative source for this repository's 29 skills; `.agents/skills/`
and `.cursor/skills/` mirror them as symlinks. FR-129 was filed because
`.agents/skills/fr-governance/SKILL.md` was a symlink to a **directory** — the FR governance skill
was unreadable through that mirror (`EISDIR`) for its entire life, and no check had ever opened
it.

The defect satisfied every structural property: the entry existed, the symlink resolved, the
target was present, `git ls-files -s` showed mode `120000`. That is why Scenario 2 below is the
load-bearing one — it is a *read*, and it is the only check that fails on a tree where everything
else is green.

**Self-referential safety.** These scenarios read `.claude/skills/`, which is this repository's own
skill source, and the gate under test is one that a governance session may itself be running. All
scenarios are read-only against the working tree: the automated gate copies the governed inputs
into `$TMPDIR` and injects defects only there. It starts no daemon, touches no runtime database,
invokes no provider binary, and modifies no file under the repository. Scenario 5 is the only one
that inspects deleted paths, and it does so through `git ls-files`.

Primary entry points:

```bash
./scripts/qa/test-skill-mirror-integrity.sh
./scripts/qa/test-skill-mirror-integrity.sh --fixture-test
```

See [DD-141](../../design_doc/orchestrator/141-skill-mirror-integrity.md).

### Recorded pre-migration baseline

Captured at `HEAD=dd993346c2f79272f58ccf93da7410e79ab91de0`, clean worktree, before any repair:

```
$ for root in .agents/skills .cursor/skills; do
    for d in "$root"/*; do [ -f "$d/SKILL.md" ] && [ -s "$d/SKILL.md" ] || echo "BROKEN: $d/SKILL.md"; done
  done
BROKEN: .agents/skills/fr-governance/SKILL.md

.agents/skills: 20 entries present, 10 skills missing
  dependabot-governance design-brief-gen design-governance guide-alignment
  integration-authoring playwright-cli qa-doc-governance security-test-doc-gen
  tools uiux-test-doc-gen
.cursor/skills: 16 entries present, 14 missing (13 skills + tools)
  align-tests dependabot-governance design-brief-gen design-governance fr-governance
  guide-alignment integration-authoring orchestrator-guide orchestrator-test-monitor
  playwright-cli qa-doc-governance security-test-doc-gen tools uiux-test-doc-gen
```

Scenario 2 replays this exact command and requires empty output. `tools/` is correctly absent from
both mirrors: it is a shared script bundle with no `SKILL.md`, declared `notSkills` in the policy.

---

## Scenario 1: Every Skill Is Mirrored Into Every Declared Root Or Exempted With A Reason

### Preconditions

- `jq` is installed.
- Clean worktree.

### Steps

1. `./scripts/qa/test-skill-mirror-integrity.sh`
2. `jq -r '.mirrorRoots[]' config/governance/skill-mirrors.json`
3. `jq -r '.exemptions | length' config/governance/skill-mirrors.json`
4. Compare the source skill list against each mirror directly:

```bash
src=$(ls -1 .claude/skills | grep -v '^tools$' | LC_ALL=C sort)
for root in .agents/skills .cursor/skills; do
  echo "$root: $(comm -23 <(echo "$src") <(ls -1 "$root" | LC_ALL=C sort) | tr '\n' ' ')"
done
```

### Expected result

- Step 1 exits `0` and reports `5 passed, 0 failed`, with `skills: 29`.
- Step 2 lists `.agents/skills` and `.cursor/skills`.
- Step 3 prints `0` — no skill is currently exempted; the mirrors are complete rather than
  selectively excused.
- Step 4 prints an empty list for both roots. Nothing is missing.

---

## Scenario 2: Every Mirrored SKILL.md Opens As A Non-Empty Regular File

This is the scenario that would have caught the original defect. Steps 2 and 3 deliberately avoid
the gate and read the filesystem directly, so a bug in the gate cannot make this pass.

### Preconditions

- Clean worktree.

### Steps

1. `./scripts/qa/test-skill-mirror-integrity.sh 2>&1 | grep check_skill_md_readable`
2. Replay the baseline command:

```bash
for root in .agents/skills .cursor/skills; do
  for d in "$root"/*; do
    [ -f "$d/SKILL.md" ] && [ -s "$d/SKILL.md" ] || echo "BROKEN: $d/SKILL.md"
  done
done
```

3. Verify the previously broken skill resolves through both mirrors and matches the source:

```bash
for root in .agents/skills .cursor/skills; do
  cmp "$root/fr-governance/SKILL.md" .claude/skills/fr-governance/SKILL.md && echo "$root ok"
done
```

4. `ls -l .agents/skills/fr-governance`

### Expected result

- Step 1 prints `PASS: check_skill_md_readable`.
- Step 2 prints nothing. Compare against the baseline above, which printed
  `BROKEN: .agents/skills/fr-governance/SKILL.md`.
- Step 3 prints `ok` for both roots — 9848 bytes, identical to the source.
- Step 4 shows `fr-governance -> ../../.claude/skills/fr-governance`, the same shape as every
  other entry. The `<name>/SKILL.md -> <directory>` form is gone.

---

## Scenario 3: Each Corruption Shape Is Independently Detected

### Preconditions

- `jq` is installed. The fixtures run entirely under `$TMPDIR`; the worktree is not touched.

### Steps

1. `./scripts/qa/test-skill-mirror-integrity.sh --fixture-test`
2. Confirm the worktree is unchanged afterwards: `git status --porcelain`

### Expected result

- Step 1 exits `0` and reports `17 passed, 0 failed`, including:
  - two positive controls — the unmodified copy passes all five checks, and the copy preserved
    symlinks rather than flattening them into directories;
  - **fixture 4a**, isolated to `check_skill_md_readable`: a `SKILL.md` that is a directory passes
    every structural check and fails only the read. This is the FR-129 defect reduced to its
    essence;
  - **fixture 4b**, the production defect verbatim (`<name>/SKILL.md -> <directory>`), caught by
    shape and read together;
  - fixture 1 (unmirrored, unexempted skill), fixture 2 (mirror replaced by a real directory
    holding a real copy — readable, and still wrong), fixture 3 (dangling symlink), fixture 5
    (exemption for a skill that no longer exists), fixture 6 (directory with no `SKILL.md` and no
    `notSkills` entry), fixture 7 (exemption with no substantive reason);
  - two meta-assertions: `ALL_CHECKS` names every check function the file defines, and every
    registered check is proven by at least one negative fixture.
- Each fixture line ends with `(isolated to [...])`. Fixtures 3 and 4b name two checks, because a
  dangling or directory-shaped mirror is both malformed and unreadable; every other fixture names
  exactly one.
- Step 2 prints nothing.

### Mutation evidence

Recorded during implementation, run against a copy placed in `scripts/qa/` and removed afterwards:

| Mutation | Result |
|---|---|
| `check_skill_md_readable` deleted from `ALL_CHECKS` | `FAIL: meta: ALL_CHECKS drifted from the defined check functions`, exit 1 |
| `check_skill_md_readable` neutered to `return 0` | fixtures 3, 4a, 4b fail, exit 1 |
| `check_mirror_shape` neutered to `return 0` | fixtures 2, 3, 4b fail, exit 1 |

---

## Scenario 4: The Gate Is Actually Enforced, Not Merely Present

### Preconditions

- `jq` and `rg` are installed.

### Steps

1. `./scripts/qa/test-qa-gate-surface.sh`
2. `jq '.scripts[] | select(.path == "scripts/qa/test-skill-mirror-integrity.sh")' config/governance/qa-gate-surface.json`
3. `rg -n 'test-skill-mirror-integrity' .github/workflows/ci.yml`

### Expected result

- Step 1 exits `0`, reports `5 passed, 0 failed`, and `14 of 47 gates are ci-required`. Its
  `check_wiring_truth` is what proves the claim in step 2 is not merely written down: a
  `ci-required` entry whose workflow job does not actually invoke the script fails this gate.
- Step 2 shows `enforcement: ci-required`, `workflow: .github/workflows/ci.yml`, `job: governance`,
  and `providerIsolation.mode: no-provider`.
- Step 3 shows two steps in the `governance` job: the verification run and the
  `--fixture-test` run.

---

## Scenario 5: The Unproduced Third Copy Is Gone And Packaging Still Works

`skills/orchestrator-guide/` was a git-tracked copy with no producer — `scripts/package-skills.sh`
reads `.claude/skills/` and writes `dist/`, never `skills/` — and it had already drifted ~32KB
from the source. It was deleted rather than pinned.

### Preconditions

- Clean worktree.

### Steps

1. `git ls-files skills/`
2. `rg -n 'skills/orchestrator-guide' --glob '!.claude/**' -S | grep -v '\.claude/skills'`
3. `bash scripts/package-skills.sh v0-fr129-check && tar tzf dist/orchestrator-skills-v0-fr129-check.tar.gz`
4. `rm -f dist/orchestrator-skills-v0-fr129-check.tar.gz`
5. `rg -n '\.gemini' -S`

### Expected result

- Step 1 prints nothing; the path is untracked and absent.
- Step 2 prints nothing. Every surviving reference points at `.claude/skills/orchestrator-guide`,
  including the FR-007 closure note in `docs/feature_request/README.md`, which previously named the
  deleted path and was repaired with the removal.
- Step 3 succeeds and lists exactly `.claude/skills/orchestrator-guide/SKILL.md` plus four
  `references/*.md` files. The release package is unaffected, because it never sourced from the
  deleted copy.
- Step 5 prints nothing. `SKILLS.md` previously documented a `.gemini/skills/` mirror that did not
  exist on disk; the line was removed.

### Reverse-applicable removal evidence

The deletion is a single commit touching only tracked file removals and the one falsified
reference. `git revert <commit>` restores `skills/orchestrator-guide/**` and
`skills/orchestrator-guide.skill` byte-for-byte from git history; no build step or generated
artifact stands between the commit and its inverse.

---

## Checklist

- [ ] `./scripts/qa/test-skill-mirror-integrity.sh` — 5 passed, 0 failed, 29 skills
- [ ] `./scripts/qa/test-skill-mirror-integrity.sh --fixture-test` — 17 passed, 0 failed
- [ ] `./scripts/qa/test-qa-gate-surface.sh` — 5 passed, 0 failed, 14 of 47 ci-required
- [ ] `./scripts/qa-doc-lint.sh` — PASS
- [ ] `cargo test --workspace` — all pass
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- [ ] `scripts/package-skills.sh` produces the release archive from `.claude/skills/`
- [ ] `git status --porcelain` empty before and after; `git rev-parse HEAD` unchanged across the run
