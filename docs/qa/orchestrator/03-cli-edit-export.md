# Orchestrator - CLI Edit and Export

**Module**: orchestrator
**Scope**: Validate edit export/open commands for resource management
**Scenarios**: 4
**Priority**: Medium

---

## Background

This document tests the edit commands for exporting and opening resources in the editor.

Entry point: `orchestrator edit <command>`

---

## Scenario 1: Edit Export Workspace

### Preconditions

- Workspace exists in configuration

### Steps

1. Export workspace resource:
   ```bash
   orchestrator edit export workspace/default
   ```

2. Verify exported YAML format:
   ```bash
   # Output should be valid YAML with apiVersion, kind, metadata, spec
   ```

### Expected

- Export shows workspace configuration in manifest format
- Output can be used with `orchestrator apply`

---

## Scenario 2: Edit Export Agent

### Preconditions

- Agent configured in configuration

### Steps

1. Export agent resource:
   ```bash
   orchestrator edit export agent/mock_echo
   ```

2. Verify agent templates are included:
   ```bash
   # Should show qa, fix, retest templates
   ```

### Expected

- Export shows agent configuration with all templates

---

## Scenario 3: Edit Export Workflow

### Preconditions

- Config must be bootstrapped first: `orchestrator config bootstrap --from fixtures/test-workflow-execution.yaml --force`
- Workflow configured in configuration

### Steps

1. Export workflow resource:
   ```bash
   orchestrator edit export workflow/qa_only
   ```

2. Verify workflow steps are included:
   ```bash
   # Should show steps, loop, finalize rules
   ```

### Expected

- Export shows full workflow configuration

---

## Scenario 4: Edit Open (if implemented)

### Preconditions

- $EDITOR environment variable set

### Steps

1. Try to open workspace in editor:
   ```bash
   EDITOR=cat orchestrator edit open workspace/default
   ```

### Expected

- Opens resource in editor (or shows content if EDITOR=cat)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Edit Export Workspace | ☐ | | | |
| 2 | Edit Export Agent | ☐ | | | |
| 3 | Edit Export Workflow | ☐ | | | |
| 4 | Edit Open | ☐ | | | |
