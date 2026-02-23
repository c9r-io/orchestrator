# Orchestrator - kubectl-Style Extensions

**Module**: orchestrator  
**Scope**: `get <resource-type>` lists, resource `create`, stdin apply, label selector  
**Scenarios**: 4  
**Priority**: High

---

## Scenario 1: List-Style Get

### Preconditions

- Database initialized.
- At least one workspace/agent/workflow resource exists.

### Steps

1. List workspaces:
   ```bash
   ./scripts/orchestrator.sh get workspaces -o table
   ```

2. List agents:
   ```bash
   ./scripts/orchestrator.sh get agents -o json
   ```

3. List workflows:
   ```bash
   ./scripts/orchestrator.sh get workflows -o yaml
   ```

### Expected Result

- Commands succeed.
- Output includes resources of the requested type.
- `table/json/yaml` outputs are all supported.

---

## Scenario 2: Label Selector on Get List

### Preconditions

- Database initialized.
- At least one resource has labels (via `create --label` or `apply` manifest metadata).

### Steps

1. Query with single selector:
   ```bash
   ./scripts/orchestrator.sh get workspaces -l env=dev -o json
   ```

2. Query with multi-condition selector:
   ```bash
   ./scripts/orchestrator.sh get agents -l env=dev,tier=qa -o yaml
   ```

3. Validate single-resource get rejects selector:
   ```bash
   ./scripts/orchestrator.sh get workspace/default -l env=dev
   ```

### Expected Result

- List query returns only matching resources.
- Selector supports `key=value[,key2=value2]` (AND).
- Single-resource get with `-l` fails with clear error.

---

## Scenario 3: Create Commands (Dry Run + Persist)

### Preconditions

- Database initialized.

### Steps

1. Workspace create dry run:
   ```bash
   ./scripts/orchestrator.sh workspace create ws-demo \
     --root-path workspace/ws-demo \
     --qa-target docs/qa \
     --label env=dev \
     --dry-run -o yaml
   ```

2. Agent create persist:
   ```bash
   ./scripts/orchestrator.sh agent create qa-agent \
     --template-qa 'echo qa {rel_path}' \
     --capability qa \
     --label env=dev
   ```

3. Workflow create persist:
   ```bash
   ./scripts/orchestrator.sh workflow create qa-flow \
     --step qa --step fix \
     --loop-mode infinite \
     --max-cycles 3 \
     --label env=dev
   ```

4. Verify created resources are queryable:
   ```bash
   ./scripts/orchestrator.sh get agents -l env=dev
   ./scripts/orchestrator.sh get workflows -l env=dev
   ```

### Expected Result

- Dry run prints manifest only, does not mutate config.
- Persist mode creates/configures resources successfully.
- Label metadata is queryable via `get -l`.

---

## Scenario 4: Stdin Apply (`-f -`)

### Preconditions

- Database initialized.

### Steps

1. Apply manifest from stdin:
   ```bash
   cat <<'YAML' | ./scripts/orchestrator.sh apply -f -
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: stdin-agent
     labels:
       source: stdin
   spec:
     templates:
       qa: "echo '{\"confidence\":0.91,\"quality_score\":0.87,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"stdin-qa\",\"description\":\"qa from stdin\",\"severity\":\"info\"}]}]}'"
   YAML
   ```

2. Verify resource exists and label selector works:
   ```bash
   ./scripts/orchestrator.sh get agents -l source=stdin -o table
   ```

### Expected Result

- `apply -f -` reads from stdin and applies successfully.
- Applied resource can be queried by label selector.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | List-Style Get | ☐ | | | |
| 2 | Label Selector on Get List | ☐ | | | |
| 3 | Create Commands (Dry Run + Persist) | ☐ | | | |
| 4 | Stdin Apply (`-f -`) | ☐ | | | |
