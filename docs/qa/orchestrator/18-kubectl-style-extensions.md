---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S2]
---

# Orchestrator - kubectl-Style Extensions

**Module**: orchestrator  
**Scope**: `get <resource-type>` lists, stdin apply, label selector
**Scenarios**: 3
**Priority**: High

---

## Scenario 1: List-Style Get

### Preconditions

- Database initialized.
- At least one workspace/agent/workflow resource exists.

### Steps

1. List workspaces:
   ```bash
   orchestrator get workspaces -o table
   ```

2. List agents:
   ```bash
   orchestrator get agents -o json
   ```

3. List workflows:
   ```bash
   orchestrator get workflows -o yaml
   ```

### Expected Result

- Commands succeed.
- Output includes resources of the requested type.
- `json` and `yaml` produce their respective encodings.
- `table` is accepted but currently falls back to JSON pretty-print for generic `get` commands (columnar table formatting is only implemented for `db` and `secret` subcommands which have fixed column schemas).

---

## Scenario 2: Label Selector on Get List

### Preconditions

- Database initialized.
- At least one resource has labels (via `apply` manifest metadata).
- **Note**: Built-in resource types (workspaces, agents, workflows) may not have labels by default. Label selector functionality is verified via Scenario 3's stdin apply (which creates a labeled agent) and via CRD resources that carry labels. Step 3 below validates the error path independently of label presence.

### Steps

1. Query with single selector:
   ```bash
   orchestrator get workspaces -l env=dev -o json
   ```

2. Query with multi-condition selector:
   ```bash
   orchestrator get agents -l env=dev,tier=qa -o yaml
   ```

3. Validate single-resource get rejects selector:
   ```bash
   orchestrator get workspace/default -l env=dev
   ```

### Expected Result

- List query returns only matching resources (empty result is acceptable if no resources carry the queried labels; label selector mechanism is validated end-to-end in Scenario 3).
- Selector supports `key=value[,key2=value2]` (AND).
- Single-resource get with `-l` fails with clear error.

---

## Scenario 3: Stdin Apply (`-f -`)

### Preconditions

- Database initialized.

### Steps

1. Apply manifest from stdin:
   ```bash
   cat <<'YAML' | orchestrator apply -f -
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
   orchestrator get agents -l source=stdin -o table
   ```

### Expected Result

- `apply -f -` reads from stdin and applies successfully.
- Applied resource can be queried by label selector.

### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Label selector returns empty but `get agents` shows the resource | Labels lost during `apply_to_project` bypassing resource_store | Fixed in `apply.rs` — `apply_to_project` now uses `apply_to_store` which preserves labels/annotations in the resource_store |
| `get steptemplates` or `get envstores` returns empty | Builtin CRD types not resolving through resource_store | Fixed in cli_handler/resource.rs — CRD fallback chains `resource_store.get()` |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | List-Style Get | ☐ | | | |
| 2 | Label Selector on Get List | ☐ | | | |
| 3 | Stdin Apply (`-f -`) | ☐ | | | |
