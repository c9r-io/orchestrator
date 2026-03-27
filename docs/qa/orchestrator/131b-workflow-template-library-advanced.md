---
self_referential_safe: true
---

# QA-131b: Workflow Template Library — Advanced

Validates FR-077: advanced template features, echo-agent safety, showcase documentation, and doc site integration.

> **Split from:** [131-workflow-template-library.md](131-workflow-template-library.md) (scenarios 1-5)
> **See also:** [131c-workflow-template-library-regression.md](131c-workflow-template-library-regression.md) (scenario 11)

## Scenario 6: FR Watch — webhook Trigger with CEL filter

**Steps:**
```bash
grep 'source: webhook' docs/workflow/fr-watch.yaml
grep 'filter:' docs/workflow/fr-watch.yaml
```

**Expected:** Trigger has `source: webhook` and a CEL filter expression matching FR file paths.

## Scenario 7: All templates use echo agents (zero API cost)

**Steps:**
```bash
for f in docs/workflow/hello-world.yaml docs/workflow/qa-loop.yaml docs/workflow/plan-execute.yaml docs/workflow/scheduled-scan.yaml docs/workflow/fr-watch.yaml; do
  echo "--- $f ---"
  grep -c "echo '" "$f"
done
```

**Expected:** Every template file contains at least one echo command — no real API agent commands.

## Scenario 8: Showcase docs exist for all templates

**Steps:**
```bash
for name in hello-world qa-loop plan-execute scheduled-scan fr-watch; do
  test -f "docs/showcases/${name}.md" && echo "OK: $name" || echo "MISSING: $name"
done
```

**Expected:** All 5 showcase docs exist.

## Scenario 9: Doc site pages exist (EN + ZH)

**Steps:**
```bash
for lang in en zh; do
  for name in hello-world qa-loop plan-execute scheduled-scan fr-watch; do
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

## Checklist

- [ ] Scenario 6: FR Watch — webhook Trigger with CEL filter
- [ ] Scenario 7: All templates use echo agents (zero API cost)
- [ ] Scenario 8: Showcase docs exist for all templates
- [ ] Scenario 9: Doc site pages exist (EN + ZH)
- [ ] Scenario 10: VitePress site builds successfully
