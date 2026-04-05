---
name: orchestrator-guide
description: >-
  Guide for working with the Agent Orchestrator — a CLI tool for AI-native SDLC
  automation. Use when writing or editing YAML manifests (Workspace, Agent,
  Workflow, StepTemplate, ExecutionProfile, SecretStore, EnvStore, Trigger, CRD),
  running orchestrator CLI commands, designing workflow step pipelines, writing
  CEL prehook/finalize expressions, configuring triggers (cron/event-driven task
  creation), or configuring self-bootstrap workflows.
  Triggers: any mention of orchestrator config, YAML manifests with
  "orchestrator.dev/v2", workflow steps, prehooks, finalize rules, task
  create/start/pause, orchestrator run, step filtering, direct assembly,
  agent capabilities, StepTemplate prompts, execution profiles, sandbox
  configuration, secret management, or trigger/cron scheduling.
---

# Agent Orchestrator Guide

## CLI Command Reference (Dynamic)

Before reading static documentation, query the orchestrator itself:

```bash
orchestrator guide                         # full categorized command reference
orchestrator guide task                    # filter by command name
orchestrator guide --category resource     # filter by category
orchestrator guide --format json           # machine-readable output
```

The `guide` subcommand outputs up-to-date command descriptions with usage examples, grouped by functional category. Use it as your primary CLI reference.

## Architecture (Client/Server)

The orchestrator uses a **client/server** model over gRPC:

- **`orchestratord`** — daemon binary (gRPC server + embedded worker pool). Listens on UDS (`~/.orchestratord/orchestrator.sock`) by default, or TCP with `--bind`.
- **`orchestrator`** — thin CLI client binary that forwards all commands to the daemon via gRPC.

Install via the one-line installer or download from GitHub Releases:

```bash
curl -fsSL https://raw.githubusercontent.com/c9r-io/orchestrator/main/install.sh | sh
```

Start the daemon first, then use the CLI:

```bash
# Start daemon (standalone binary, not a CLI subcommand)
orchestratord --foreground --workers 2           # foreground (recommended for monitoring)
nohup orchestratord --foreground --workers 2 &   # background via nohup
orchestratord --bind 0.0.0.0:9090 --workers 4   # TCP instead of UDS

# Monitor daemon
ps aux | grep orchestratord | grep -v grep       # check process

# Stop daemon
kill <pid>                                        # graceful SIGTERM
```

## Core Workflow

1. Start the daemon: `orchestratord --foreground --workers 2`
2. `orchestrator init` — initialize orchestrator runtime (creates `~/.orchestratord/` with DB, secrets, etc.)
3. `orchestrator apply -f manifest.yaml --project <name>` — load resources (daemon hot-reloads config via RwLock, no restart needed)
4. `orchestrator task create --name X --goal Y --workflow Z --project <name>` — create and run (auto-enqueues to worker)
5. `orchestrator task info <id>` / `task trace <id>` / `task logs <id>` — inspect results

### Lightweight Execution (`orchestrator run`)

```bash
# Synchronous: run specific steps, follow logs, exit with status code
orchestrator run --workflow sdlc --step fix --set ticket_paths=docs/ticket/T-0042.md

# Background (equivalent to task create)
orchestrator run --workflow sdlc --step fix --detach

# Direct assembly: execute a StepTemplate without a workflow
orchestrator run --template fix-ticket --agent-capability fix --set ticket_paths=docs/ticket/T-0042.md
```

`task create` also supports `--step` (repeatable) and `--set key=value` (repeatable) for step filtering and pipeline variable injection.

## Resource Kinds

All resources use `apiVersion: orchestrator.dev/v2` with `metadata.name` and `spec`.

| Kind | Scope | Purpose |
|------|-------|---------|
| Workspace | project | File system context: root_path, qa_targets, ticket_dir, self_referential |
| Agent | project | Execution unit: capabilities list + command template with `{prompt}` |
| StepTemplate | project | Prompt content with pipeline variables (`{goal}`, `{diff}`, `{source_tree}`, etc.) |
| Workflow | project | Step pipeline + loop policy + finalize rules + safety config |
| ExecutionProfile | project | Sandbox/isolation policy: fs_mode, network_mode, resource limits |
| SecretStore | project | Encrypted key-value pairs for sensitive data (API keys, tokens) |
| EnvStore | project | Plain key-value pairs for environment variables |
| WorkflowStore | project | Cross-task persistent key-value store (WP01) |
| Trigger | project | Cron-scheduled or event-driven automatic task creation |
| RuntimePolicy | singleton | Runner shell config, resume behavior, observability, redaction patterns |
| Project | cluster | Namespace for organizing resources |
| CustomResourceDefinition | cluster | Extensible resource types with JSON Schema + CEL validation |
| StoreBackendProvider | cluster | Custom workflow store backends |

## Minimal Manifest Example

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: my_agent
spec:
  capabilities: [qa]
  command: "echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: simple
spec:
  steps:
    - id: qa
      enabled: true
  loop:
    mode: once
```

## ExecutionProfile

Controls sandbox isolation for workflow steps. Referenced by name in step `execution_profile` field.

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  mode: sandbox                    # host | sandbox
  fs_mode: workspace_rw_scoped    # inherit | workspace_readonly | workspace_rw_scoped
  writable_paths:                  # paths writable within sandbox (relative to workspace root)
    - docs
    - core/src
    - crates
  network_mode: inherit            # inherit | deny | allowlist
  network_allowlist:               # required when network_mode=allowlist (Linux only)
    - api.example.com:443
  max_memory_mb: 512               # optional: RLIMIT_AS
  max_cpu_seconds: 60              # optional: RLIMIT_CPU
  max_processes: 4                 # optional: RLIMIT_NPROC
  max_open_files: 1024             # optional: RLIMIT_NOFILE
```

### Platform Behavior

| Feature | Linux | macOS |
|---------|-------|-------|
| `mode: sandbox` | namespace isolation | Seatbelt (`sandbox-exec`) |
| `network_mode: deny` | nftables DROP | Seatbelt deny |
| `network_mode: allowlist` | nftables per-target rules + auto DNS | **Not supported** (validation error) |
| Resource limits | setrlimit() | setrlimit() |

### Production vs QA Profiles

- **Production profiles**: Use `network_mode: inherit` — agents need API access for LLM calls.
- **QA/fixture profiles**: Use `network_mode: deny` — tests verify sandbox enforcement.

Referenced in workflow steps:

```yaml
steps:
  - id: implement
    execution_profile: sandbox_write   # references ExecutionProfile by name
```

## SecretStore & EnvStore

```yaml
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: claude-opus
spec:
  data:
    ANTHROPIC_API_KEY: "sk-ant-..."
---
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: build-env
spec:
  data:
    CARGO_TERM_COLOR: "always"
    RUST_BACKTRACE: "1"
```

Referenced in Agent spec via `env`:

```yaml
kind: Agent
spec:
  env:
    - name: MY_VAR
      value: "literal"             # direct literal
    - fromRef: claude-opus         # import all keys from SecretStore/EnvStore
    - name: API_KEY
      refValue:                    # import single key
        name: claude-opus
        key: ANTHROPIC_API_KEY
```

### Secret Key Management

Run `orchestrator guide "secret key"` for the full key lifecycle command reference.

## Step Definition Quick Reference

```yaml
- id: plan              # required, unique
  scope: task           # task (once/cycle) | item (per QA file)
  enabled: true         # required
  template: plan        # StepTemplate name (agent steps)
  builtin: self_test    # OR builtin handler name
  command: "cargo check" # OR direct shell command
  execution_profile: sandbox_write  # ExecutionProfile name (optional)
  max_parallel: 2       # per-step parallelism override (optional)
  timeout_secs: 600     # per-step timeout (optional)
  prehook:              # conditional gate (CEL)
    engine: cel
    when: "is_last_cycle"
  behavior:
    on_failure: { action: continue }
    post_actions:
      - type: store_put
        store: context
        key: result
        from_var: plan_output
  store_inputs: [{store: X, key: Y, as_var: Z}]
  store_outputs: [{store: X, key: Y, from_var: Z}]
```

Auto-inferred from `id`: builtin IDs → builtin mode; agent IDs → capability mode. See references for full lists.

## Agent Structured Output

Agents must produce JSON on stdout:

```json
{"confidence": 0.95, "quality_score": 0.9, "artifacts": [{"kind": "analysis", "findings": [{"title": "X", "description": "Y", "severity": "info"}]}]}
```

## Reference Files

Load these as needed for detailed specifications:

- **Resource model, pipeline variables, step fields**: Read [references/resource-and-steps.md](references/resource-and-steps.md)
- **CEL prehook/finalize variables and patterns**: Read [references/cel-expressions.md](references/cel-expressions.md)
- **CLI command reference**: Run `orchestrator guide` (or see [references/cli-reference.md](references/cli-reference.md) for supplementary notes on daemon binary and apply ordering)
- **Advanced features (CRD, Store, Spawn, Invariants, Self-Bootstrap)**: Read [references/advanced.md](references/advanced.md)
