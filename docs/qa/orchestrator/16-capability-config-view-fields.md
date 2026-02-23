# Orchestrator - Capability Manifest Export Fields

**Module**: orchestrator
**Scope**: Validate manifest export exposes capability-orchestration fields
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `07-capability-orchestration.md` to keep each QA document within 5 scenarios.

Entry point: `./scripts/orchestrator.sh manifest export`

---

## Scenario 1: Manifest Export Shows Capability Fields

### Preconditions

- Orchestrator initialized: `./scripts/orchestrator.sh init --force`
- Config applied with capability fields: `./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/capability-test.yaml`

### Steps

1. Export current runtime config to a temporary YAML file:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/exported-config.yaml
   ```

2. Inspect agent fields:
   ```bash
   ./scripts/orchestrator.sh manifest export -o json | jq '.agents'
   ```

3. Inspect workflow step fields:
   ```bash
   ./scripts/orchestrator.sh manifest export -o json | jq '.workflows | to_entries[0].value.steps'
   ```

### Expected

- Agents show `metadata.cost`, `capabilities`, and optional `selection.strategy`.
- Steps show `repeatable`, `is_guard`, `required_capability`, and `builtin` (when configured).
- Field names match runtime schema used by CLI.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Manifest Export Shows Capability Fields | ☐ | | | |
