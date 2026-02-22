# Orchestrator - Capability Config View Fields

**Module**: orchestrator
**Scope**: Validate config view exposes capability-orchestration fields
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `07-capability-orchestration.md` to keep each QA document within 5 scenarios.

Entry point: `./scripts/orchestrator.sh config view`

---

## Scenario 1: Config View Shows Capability Fields

### Preconditions

- Orchestrator initialized: `./scripts/orchestrator.sh init --force`
- Config applied with capability fields: `./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/capability-test.yaml`

### Steps

1. Export current runtime config to a temporary YAML file:
   ```bash
   ./scripts/orchestrator.sh config export -f /tmp/exported-config.yaml
   ```

2. Inspect agent fields:
   ```bash
   ./scripts/orchestrator.sh config view -o json | jq '.agents'
   ```

3. Inspect workflow step fields:
   ```bash
   ./scripts/orchestrator.sh config view -o json | jq '.workflows | to_entries[0].value.steps'
   ```

### Expected

- Agents show `metadata.cost`, `capabilities`, and optional `selection.strategy`.
- Steps show `repeatable`, `is_guard`, `required_capability`, and `builtin` (when configured).
- Field names match runtime schema used by CLI.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Config View Shows Capability Fields | ☐ | | | |
