# Orchestrator - Self-Bootstrap & AI Native SDLC Workflow

**Module**: orchestrator
**Scope**: Validate self-bootstrap workflow with AI native SDLC closed-loop: plan → qa_doc_gen → implement → self_test → self_restart → qa_testing → ticket_fix → align_tests → doc_governance, pipeline variable propagation, prehook-gated steps, and checkpoint/rollback safety
**Scenarios**: 5
**Priority**: High
**See also**: `docs/qa/self-bootstrap/01-survival-binary-checkpoint-self-test.md`, `docs/qa/self-bootstrap/02-survival-enforcement-watchdog.md` for the 4-layer survival mechanism (binary checkpoint, self-test gate, self-referential enforcement, watchdog)

---

## Background

Self-bootstrap workflows allow the orchestrator to orchestrate AI agents that
develop its own codebase. The simplified AI native SDLC closed-loop:

```
plan → qa_doc_gen → implement → self_test → self_restart → qa_testing → ticket_fix → align_tests → doc_governance → loop_guard
```

Key design decisions:
- **No separate build/test/lint steps** — these are covered by skill internals:
  - `implement` agent runs `cargo check` before finishing
  - `qa-testing` skill rebuilds CLI (`cargo build --release`) as prerequisite
  - `align-tests` skill runs `cargo test` + `cargo clippy` + `cargo build`, iterates until stable
- **ticket_fix is prehook-gated**: only runs when `active_ticket_count > 0`
- **Pipeline variables** flow between steps (`{goal}`, `{plan_output}`, `{source_tree}`, `{diff}`)
- **Safety config**: checkpoint via git tag, max consecutive failures

Fixture: `fixtures/manifests/bundles/self-bootstrap-test.yaml`

### Common Preconditions

```bash
rm -f fixtures/ticket/auto_*.md

QA_PROJECT="qa-bootstrap-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml
orchestrator project reset "${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply --project "${QA_PROJECT}" --force
```

---

## Scenario 1: Basic Bootstrap Workflow (plan → implement → build → test)

### Preconditions
- Common Preconditions applied
- Clean ticket directory (leftover ticket files without QA document references get picked up as active tickets for `__UNASSIGNED__` task items, causing false failures):
  ```bash
  find fixtures/ticket/ -name '*.md' ! -name 'README.md' -delete
  ```

### Steps
1. Create task with `bootstrap_basic` workflow
2. Start task and wait for completion

### Expected
- Task status: `completed`
- Events include `step_started`/`step_finished` for: `plan`, `implement`, `build`, `test`

---

## Scenario 2: Build Failure Triggers Fix via Prehook

### Preconditions
- Common Preconditions applied

### Steps
1. Create task with `bootstrap_with_fix` workflow (build exits non-zero)

### Expected
- Build step `success: false`, `build_errors > 0`
- Fix step prehook `build_errors > 0 || test_failures > 0` → true → fix runs

---

## Scenario 3: Successful Build Skips Fix Step

### Preconditions
- Common Preconditions applied

### Steps
1. Create task with `bootstrap_skip_fix` workflow (build + test succeed)

### Expected
- Fix step `step_skipped` with `reason: prehook_false`

---

## Scenario 4: Checkpoint Created at Cycle Start

### Preconditions
- Common Preconditions applied, workspace in git repo

### Steps
1. Create task with `bootstrap_checkpoint` workflow (`checkpoint_strategy: git_tag`)

### Expected
- `checkpoint_created` event with tag `checkpoint/{task_id}/1`
- Git tag exists in repository

---

## Scenario 5: Self-Bootstrap Manifest Applies Successfully

### Preconditions
- Clean runtime state

### Steps
1. Apply `fixtures/manifests/bundles/self-bootstrap-mock.yaml` (dry-run + real)
2. Verify resources

### Expected
- Workspace `self` with `self_referential: true`
- 4 agents registered with model-optimized capability split:
  - `architect` (opus): plan, qa_doc_gen — deep reasoning for planning and QA doc design
  - `coder` (sonnet): implement, ticket_fix, align_tests — code generation, fixing, test alignment
  - `tester` (sonnet): qa_testing — QA scenario execution requiring reliable tool-use
  - `reviewer` (haiku): doc_governance, review, loop_guard — lightweight pattern matching
- Workflow `self-bootstrap` with simplified SDLC steps: plan, qa_doc_gen, implement, self_test, self_restart, qa_testing, ticket_fix, align_tests, doc_governance, loop_guard
- Safety: `max_consecutive_failures: 3`, `checkpoint_strategy: git_tag`

---

## Checklist

| # | Scenario | Status | Date | Tester | Notes |
|---|----------|--------|------|--------|-------|
| 1 | Basic Bootstrap Workflow | | | | |
| 2 | Build Failure Triggers Fix | | | | |
| 3 | Successful Build Skips Fix | | | | |
| 4 | Checkpoint Created at Cycle Start | | | | |
| 5 | Self-Bootstrap Manifest Applies | | | | |
