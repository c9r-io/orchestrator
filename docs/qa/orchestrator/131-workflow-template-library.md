---
self_referential_safe: true
---

# QA-131: Workflow Template Library

Validates FR-077: workflow template library with 5 beginner-to-advanced templates, showcase documentation, and doc site integration.

## Scenario 1: Template YAML valid structure

**Steps:**
```bash
for f in docs/workflow/hello-world.yaml docs/workflow/qa-loop.yaml docs/workflow/plan-execute.yaml docs/workflow/deployment-pipeline.yaml docs/workflow/scheduled-scan.yaml; do
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

## Scenario 5: Deployment Pipeline — ExecutionProfile resources

**Steps:**
```bash
grep 'kind: ExecutionProfile' docs/workflow/deployment-pipeline.yaml | wc -l
```

**Expected:** At least 1 ExecutionProfile resource with sandbox mode.

## Scenario 6: Scheduled Scan — Trigger resource

**Steps:**
```bash
grep 'kind: Trigger' docs/workflow/scheduled-scan.yaml | wc -l
```

**Expected:** At least 1 Trigger resource with cron schedule.

## Scenario 7: All templates use echo agents (zero API cost)

**Steps:**
```bash
for f in docs/workflow/hello-world.yaml docs/workflow/qa-loop.yaml docs/workflow/plan-execute.yaml docs/workflow/deployment-pipeline.yaml docs/workflow/scheduled-scan.yaml; do
  echo "--- $f ---"
  grep -c "echo '" "$f"
done
```

**Expected:** Every template file contains at least one echo command — no real API agent commands.

## Scenario 8: Showcase docs exist for all templates

**Steps:**
```bash
for name in hello-world qa-loop plan-execute deployment-pipeline scheduled-scan; do
  test -f "docs/showcases/${name}.md" && echo "OK: $name" || echo "MISSING: $name"
done
```

**Expected:** All 5 showcase docs exist.

## Scenario 9: Doc site pages exist (EN + ZH)

**Steps:**
```bash
for lang in en zh; do
  for name in hello-world qa-loop plan-execute deployment-pipeline scheduled-scan; do
    test -f "site/${lang}/showcases/${name}.md" && echo "OK: ${lang}/${name}" || echo "MISSING: ${lang}/${name}"
  done
done
```

**Expected:** All 10 pages exist (5 EN + 5 ZH).

## Scenario 10: VitePress site builds successfully

**Steps:**
```bash
cd site && npx vitepress build 2>&1 | tail -3
```

**Expected:** Output contains "build complete" with no errors.

## Scenario 11: Progressive complexity — resource count increases

**Steps:**
```bash
for f in hello-world qa-loop plan-execute deployment-pipeline scheduled-scan; do
  count=$(grep -c '^kind:' "docs/workflow/${f}.yaml")
  echo "$f: $count resources"
done
```

**Expected:** Resource counts increase from hello-world (3) through scheduled-scan (5+), demonstrating progressive complexity.
