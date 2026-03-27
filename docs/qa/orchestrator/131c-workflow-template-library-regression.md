---
self_referential_safe: true
---

# QA-131c: Workflow Template Library — Regression

Validates FR-077: regression check ensuring progressive complexity across the template library.

> **Split from:** [131-workflow-template-library.md](131-workflow-template-library.md) (scenarios 1-5)
> **See also:** [131b-workflow-template-library-advanced.md](131b-workflow-template-library-advanced.md) (scenarios 6-10)

## Scenario 11: Progressive complexity — resource count increases

**Steps:**
```bash
for f in hello-world qa-loop plan-execute scheduled-scan fr-watch; do
  count=$(grep -c '^kind:' "docs/workflow/${f}.yaml")
  echo "$f: $count resources"
done
```

**Expected:** Resource counts increase from hello-world (3) through fr-watch (5+), demonstrating progressive complexity.

## Checklist

- [ ] Scenario 11: Progressive complexity — resource count increases
