# CLI Command Reference

Run `orchestrator guide` to get the full categorized command reference with examples directly from the CLI.

```bash
orchestrator guide                         # full reference (markdown)
orchestrator guide task                    # filter by command name
orchestrator guide --category resource     # filter by category
orchestrator guide --format json           # machine-readable output
```

The guide covers all 64 commands across 11 categories:
Resource Management, Task Lifecycle, Workflow Execution, Agent Management,
Store Operations, Workflow Authoring, Trigger Management, Observability,
Security, System Administration, and Built-in Tools.

## Supplementary Notes

These notes cover details NOT included in `orchestrator guide`:

### Daemon Binary

The daemon (`orchestratord`) is a **separate binary**, not a CLI subcommand:

```bash
orchestratord --foreground --workers 2           # foreground (recommended)
nohup orchestratord --foreground --workers 2 &   # background via nohup
orchestratord --bind 0.0.0.0:9090 --workers 4   # TCP instead of UDS
```

Connection: CLI connects via UDS (`~/.orchestratord/orchestrator.sock`) by default, or `$ORCHESTRATOR_SOCKET` env.

### Apply Ordering

Production setups must apply resources in dependency order:

```bash
orchestrator apply -f execution-profiles.yaml --project my-project
orchestrator apply -f secrets.yaml --project my-project
orchestrator apply -f workflow.yaml --project my-project
```

Always use `--project` for production workflows to isolate resources.

### Environment Cleanup

To reset a project to a clean empty state (keeping only the `default` workspace), delete resources in reverse-dependency order: tasks → triggers → workflows → agents → workspaces → secrets/env stores.

```bash
# 1. Delete all tasks in the project
orchestrator task delete --all -p <project> -f

# 2. Inventory remaining assets
orchestrator get agents -p <project>
orchestrator get workflows -p <project>
orchestrator get workspaces -p <project>
orchestrator get triggers -p <project>

# 3. Delete each resource kind (repeat for each name)
orchestrator delete agent <name> -p <project> -f
orchestrator delete workflow <name> -p <project> -f
orchestrator delete workspace <name> -p <project> -f      # keep "default"
orchestrator delete trigger <name> -p <project> -f
orchestrator delete steptemplate <name> -p <project> -f
orchestrator delete secretstore <name> -p <project> -f
orchestrator delete envstore <name> -p <project> -f
```

**Notes:**
- The `default` workspace is the project's root context — keep it unless deleting the entire project.
- Rapid bulk deletes may trigger rate limiting (`rate_limited`); retry with a short pause between calls.
- `get` supports: agents, workflows, workspaces, triggers. StepTemplate/SecretStore/EnvStore are only addressable by name via `delete` or `describe`.
- To delete an entire project and all its data: `orchestrator delete project/<name> -f`.

### Project-Only Deployments

In project-only deployments (no global workspaces), `orchestrator get` may fail.
Use sqlite queries to verify project-scoped resources:

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db \
  "SELECT json_extract(config_json, '$.projects.\"<project>\".workspaces') \
   FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1;"
```
