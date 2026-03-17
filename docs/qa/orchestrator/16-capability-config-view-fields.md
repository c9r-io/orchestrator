---
self_referential_safe: false
---

# Orchestrator - Capability Manifest Export Fields

**Module**: orchestrator
**Scope**: Validate manifest export exposes capability-orchestration fields
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `07-capability-orchestration.md` to keep each QA document within 5 scenarios.

Entry point: `orchestrator manifest export`

---

## Scenario 1: Manifest Export Shows Capability Fields

### Preconditions

- Orchestrator initialized: `orchestrator init --force`
- **Capability-test fixture applied** (provides agents with `cost` configured):
  ```bash
  orchestrator apply -f fixtures/manifests/bundles/capability-test.yaml --project qa-strict
  ```
  This fixture defines `agent_qa_only` (cost: 30) and `agent_fix_only` (cost: 50).
  Without it, agents may lack a `cost` value and the field will be absent from export.

### Steps

1. Export current runtime config to a temporary YAML file:
   ```bash
   orchestrator manifest export -o yaml > /tmp/exported-config.yaml
   ```

2. Inspect agent fields:
   ```bash
   orchestrator manifest export -o json | jq '.agents'
   ```

3. Inspect workflow step fields:
   ```bash
   orchestrator manifest export -o json | jq '.workflows | to_entries[0].value.steps'
   ```

### Expected

- Agents show `metadata.cost`, `capabilities`, and optional `selection.strategy`.
  - `metadata.cost` is an **optional field** (`skip_serializing_if = "Option::is_none"`).
    It only appears for agents that have a cost value configured in the applied fixture.
    Inspect `agent_qa_only` or `agent_fix_only` (from the capability-test fixture) to verify.
- Steps show `repeatable`, `is_guard`, `required_capability`, and `builtin` (when configured).
- Field names match runtime schema used by CLI.

### Troubleshooting

- **False positive: `metadata.cost` missing** -- If cost is absent from the export, verify
  that the capability-test fixture was applied *before* exporting, and that you are inspecting
  an agent that has `cost` defined (e.g., `agent_qa_only`, not `plain_text_agent`).
  Cost is optional and omitted from serialization when not set.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Manifest Export Shows Capability Fields | ☐ | | | |
