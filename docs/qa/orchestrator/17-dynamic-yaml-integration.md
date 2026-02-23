# Orchestrator - Dynamic YAML Integration

**Module**: orchestrator
**Scope**: Validate dynamic orchestration fields through YAML export/import path
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `13-dynamic-orchestration.md` to keep each QA document within 5 scenarios.

Entry point: `./scripts/orchestrator.sh manifest <command>`

---

## Scenario 1: YAML Configuration Integration for Dynamic Fields

### Preconditions

- Orchestrator initialized: `./scripts/orchestrator.sh init --force`
- Config applied: `./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/output-formats.yaml` (or any valid config fixture)

### Steps

1. Export runtime config:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/exported-config.yaml
   ```

2. Verify adaptive workflow snippet:
   ```bash
   grep -A 5 "adaptive:" /tmp/exported-config.yaml || true
   ```

3. Verify dynamic steps snippet:
   ```bash
   grep -A 15 "dynamic_steps:" /tmp/exported-config.yaml | head -20 || true
   ```

4. Validate exported YAML:
   ```bash
   ./scripts/orchestrator.sh manifest validate -f /tmp/exported-config.yaml
   ```

### Expected

- Exported YAML can be validated successfully.
- Dynamic orchestration fields (if configured) are preserved in YAML representation.
- YAML remains an artifact for edit/export/apply, not the runtime source of truth.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML Configuration Integration for Dynamic Fields | ✅ | 2026-02-23 | opencode | PASS - exported YAML validates, dynamic_steps preserved |
