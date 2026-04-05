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

### Project-Only Deployments

In project-only deployments (no global workspaces), `orchestrator get` may fail.
Use sqlite queries to verify project-scoped resources:

```bash
sqlite3 ~/.orchestratord/agent_orchestrator.db \
  "SELECT json_extract(config_json, '$.projects.\"<project>\".workspaces') \
   FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1;"
```
