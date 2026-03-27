---
self_referential_safe: true
---

# QA 130b: Integration Manifest Packages (Advanced)

Continuation of [130-integration-manifest-packages.md](130-integration-manifest-packages.md).

## FR Reference

FR-082

## Scenario 6: Secret rotation showcase exists

**Steps:**
1. `cat docs/showcases/secret-rotation-workflow.md | head -5`

**Expected:** File exists with Agent Collaboration header.

## Scenario 7: Each README has setup steps (Deferred)

> **Deferred**: Integration package READMEs are not yet published. This scenario will be activated when the `orchestrator-integrations` companion repo publishes integration packages with README files.

**Steps:**
1. Check each integration README for: Prerequisites, Setup steps, Apply commands

**Expected:** All READMEs are complete (once published).

## Checklist

- [ ] Scenario 6: Secret rotation showcase exists
- [ ] Scenario 7: Each README has setup steps
