---
self_referential_safe: true
---

# QA-131: Workflow Template Library

Validates FR-077: workflow template library with 5 beginner-to-advanced templates, showcase documentation, and doc site integration.

> **Split notice:** This doc covers scenarios 1-5 (structure and content validation). See also:
> - [131b-workflow-template-library-advanced.md](131b-workflow-template-library-advanced.md) — scenarios 6-10 (advanced features and documentation)
> - [131c-workflow-template-library-regression.md](131c-workflow-template-library-regression.md) — scenario 11 (regression)

## Scenario 1: Template YAML valid structure

**Steps:**
```bash
for f in docs/workflow/hello-world.yaml docs/workflow/qa-loop.yaml docs/workflow/plan-execute.yaml docs/workflow/scheduled-scan.yaml docs/workflow/fr-watch.yaml; do
  echo "--- $f ---"
  grep -c 'apiVersion: orchestrator.dev/v2' "$f"
done
```

**Expected:** Each file exists and contains at least one `apiVersion: orchestrator.dev/v2` resource definition.

## Scenario 2: Hello World template — minimal resource set

**Steps:**
```bash
grep 'kind:' docs/workflow/hello-world.yaml | sort -u
```

**Expected:** Output contains exactly: `Agent`, `Workspace`, `Workflow` — the minimal three-resource set.

## Scenario 3: QA Loop template — multi-step capability matching

**Steps:**
```bash
grep -E 'required_capability:|capabilities:' docs/workflow/qa-loop.yaml
```

**Expected:** Workflow steps reference `qa`, `fix`, `retest` capabilities; agents declare matching capabilities.

## Scenario 4: Plan-Execute template — StepTemplate resources

**Steps:**
```bash
grep 'kind: StepTemplate' docs/workflow/plan-execute.yaml | wc -l
```

**Expected:** At least 3 StepTemplate resources (plan, implement, verify).

## Scenario 5: Scheduled Scan — agent audit + static check two-phase

**Steps:**
```bash
grep 'kind: StepTemplate' docs/workflow/scheduled-scan.yaml | wc -l
grep 'kind: Trigger' docs/workflow/scheduled-scan.yaml | wc -l
```

**Expected:** At least 2 StepTemplate resources (agent_audit, static_check) and 1 Trigger with cron schedule.

## Checklist

- [x] Scenario 1: Template YAML valid structure
- [x] Scenario 2: Hello World template — minimal resource set
- [x] Scenario 3: QA Loop template — multi-step capability matching
- [x] Scenario 4: Plan-Execute template — StepTemplate resources
- [x] Scenario 5: Scheduled Scan — agent audit + static check two-phase
