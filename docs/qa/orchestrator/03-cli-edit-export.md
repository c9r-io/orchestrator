---
self_referential_safe: false
---

# Orchestrator - CLI Edit and Export

**Module**: orchestrator
**Scope**: Validate edit export/open commands for resource management
**Scenarios**: 4
**Priority**: Medium
**Status**: NOT IMPLEMENTED — all scenarios describe planned features that do not exist in the current CLI. The `edit` subcommand is not registered. Do not raise tickets against these scenarios until the feature is implemented.

---

## Background

This document tests the edit commands for exporting and opening resources in the editor.

Entry point: `orchestrator edit <command>` (subcommands: `export`, `open`; bare `edit <resource>` is NOT valid)

> **Note**: As of 2026-03-13, the `edit` subcommand does not exist. Running `orchestrator edit` returns `error: unrecognized subcommand 'edit'`. These scenarios are placeholders for a planned feature.

---

## Scenario 1: Edit Export Workspace

### Preconditions

- Workspace exists in configuration

### Steps

1. Export workspace resource:
   ```bash
   orchestrator edit export workspace/default
   ```

2. Read the temp file path printed to stdout and verify its contents:
   ```bash
   cat "$(orchestrator edit export workspace/default 2>/dev/null)"
   ```

3. Verify exported YAML contains exactly one document with the expected fields:
   - `apiVersion: orchestrator.dev/v2`
   - `kind: Workspace`
   - `metadata.name: default`
   - `spec.root_path`, `spec.qa_targets`, `spec.ticket_dir`

4. Confirm no duplicate resources (no `---` separator, only one `kind:` line):
   ```bash
   grep -c '^kind:' "$(orchestrator edit export workspace/default 2>/dev/null)"
   # Expected: 1
   ```

### Expected

- Export writes a temp file containing exactly one Workspace resource in manifest format
- No duplicate resource copies in the output
- Output can be used with `orchestrator apply`

### Troubleshooting

| Symptom | Likely Cause | Resolution |
|---------|-------------|------------|
| Multiple `kind:` lines in output | Confused with `manifest export` which dumps all resources | Use `edit export` (single resource) not `manifest export` (all resources) |
| Command not found | Missing `./scripts/` prefix | Use `orchestrator edit export workspace/default` |

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

- Config must be applied first: `orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml`
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
| 1 | Edit Export Workspace | N/A | 2026-03-13 | claude | `edit` subcommand not implemented; false positive tickets deleted |
| 2 | Edit Export Agent | N/A | 2026-03-13 | claude | `edit` subcommand not implemented |
| 3 | Edit Export Workflow | N/A | 2026-03-13 | claude | `edit` subcommand not implemented |
| 4 | Edit Open | N/A | 2026-03-13 | claude | `edit` subcommand not implemented |
