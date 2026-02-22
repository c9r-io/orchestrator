# Orchestrator - CLI Output Formats

**Module**: orchestrator
**Scope**: Validate JSON/YAML output formats for all list and info commands
**Scenarios**: 5
**Priority**: Medium

---

## Background

This document tests that all CLI commands support proper JSON and YAML output formats for scripting and integration.

Project setup (run once):

```bash
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/output-formats.yaml
```

### Common Preconditions (Scenarios 2, 3, 5)

- Config must be applied: `orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml`
- Use isolated project reset: `./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force`

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

## Scenario 4: Config View JSON/YAML

### Preconditions

- Configuration exists

### Steps

1. Get config in JSON:
   ```bash
   orchestrator config view -o json
   ```

2. Get config in YAML:
   ```bash
   orchestrator config view -o yaml
   ```

3. Verify config can be parsed:
   ```bash
   orchestrator config view -o json | jq '.workspaces'
   ```

### Expected

- Full configuration is output
- JSON/YAML is valid and parseable

---

## Scenario 5: Workflow/Agent List JSON/YAML

### Preconditions

- See **Common Preconditions** above
- Configuration exists

### Steps

1. List workflows in JSON:
   ```bash
   orchestrator config list-workflows -o json
   ```

2. List agents in JSON:
   ```bash
   orchestrator config list-agents -o json
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
| 4 | Config View JSON/YAML | ☐ | | | |
| 5 | Workflow/Agent List JSON/YAML | ☐ | | | |
