---
self_referential_safe: false
self_referential_safe_scenarios: [S1]
---

# Orchestrator - CLI Output Formats

**Module**: orchestrator
**Scope**: Validate JSON/YAML output formats for all list and info commands
**Scenarios**: 5
**Priority**: Medium

---

## Background

This document tests that all CLI commands support proper JSON and YAML output formats for scripting and integration.

> **Note on log lines**: Structured log lines (e.g., `INFO agent_orchestrator: structured logging initialized`) are written to **stderr**, not stdout. When piping CLI output to `jq` or `yq`, only stdout is passed through the pipe, so log lines do **not** interfere with JSON/YAML parsing. If you see log lines interleaved in terminal output, that is normal stderr display — it does not affect `| jq` correctness.

Project setup (run once):

```bash
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml --project "${QA_PROJECT}"
```

### Common Preconditions (Scenarios 2, 3, 5)

- Config must be applied: `orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml`
- Recreate the isolated project scaffold: `orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true && rm -rf "workspace/${QA_PROJECT}" && orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml --project "${QA_PROJECT}"`

---

## Scenario 1: Task List JSON/YAML Output

### Preconditions

- Tasks exist in database

### Steps

1. Get JSON output:
   ```bash
   orchestrator task list -o json
   ```

2. Get YAML output:
   ```bash
   orchestrator task list -o yaml
   ```

3. Verify JSON is valid:
   ```bash
   orchestrator task list -o json | jq '.'
   ```

### Expected

- JSON output is valid and parseable
- YAML output is valid
- Both contain all task fields

---

## Scenario 2: Task Info JSON/YAML Output

### Preconditions

- See **Common Preconditions** above
- Task exists

### Steps

1. Get task info in JSON:
   ```bash
   orchestrator task info {task_id} -o json
   ```

2. Get task info in YAML:
   ```bash
   orchestrator task info {task_id} -o yaml
   ```

### Expected

- Output contains task details, items, status
- Format is valid JSON/YAML

---

## Scenario 3: Workspace List JSON/YAML

### Preconditions

- See **Common Preconditions** above
- Workspaces exist

### Steps

1. Get workspace list in JSON:
   ```bash
   orchestrator workspace list -o json
   ```

2. Get workspace list in YAML:
   ```bash
   orchestrator workspace list -o yaml
   ```

### Expected

- Output shows all workspaces
- Format is valid

---

## Scenario 4: Manifest Export JSON/YAML

### Preconditions

- Configuration exists

### Steps

1. Get config in JSON:
   ```bash
   orchestrator manifest export -o json
   ```

2. Get config in YAML:
   ```bash
   orchestrator manifest export -o yaml
   ```

3. Verify config can be parsed (manifest export returns a CRD-style array):
   ```bash
   orchestrator manifest export -o json | jq '[.[] | select(.kind == "Workspace")]'
   ```

### Expected

- Full configuration is output as a JSON array of CRD resources (`apiVersion`, `kind`, `metadata`, `spec`)
- JSON/YAML is valid and parseable
- Workspace resources can be filtered with `jq '[.[] | select(.kind == "Workspace")]'`

---

## Scenario 5: Workflow/Agent List JSON/YAML

### Preconditions

- See **Common Preconditions** above
- Configuration exists

### Steps

1. List workflows in JSON:
   ```bash
   orchestrator get workflows -o json
   ```

2. List agents in JSON:
   ```bash
   orchestrator get agents -o json
   ```

### Expected

- Output shows workflow/agent details
- Format is valid

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task List JSON/YAML | ☐ | | | |
| 2 | Task Info JSON/YAML | ☐ | | | |
| 3 | Workspace List JSON/YAML | ☐ | | | |
| 4 | Manifest Export JSON/YAML | ☐ | | | |
| 5 | Workflow/Agent List JSON/YAML | ☐ | | | |
