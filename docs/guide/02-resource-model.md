# 02 - Resource Model

The orchestrator manages four core resource kinds, plus extensible Custom Resource Definitions (CRDs). All resources follow a Kubernetes-style manifest format.

## Manifest Structure

Every resource uses the same envelope:

```yaml
apiVersion: orchestrator.dev/v2
kind: <ResourceKind>
metadata:
  name: <unique-name>
  description: "optional description"   # optional
  labels:                               # optional
    key: value
spec:
  # kind-specific fields
```

Multiple resources can be defined in a single YAML file, separated by `---`.

## 1. Workspace

A Workspace defines the file system context for task execution.

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: my-project
spec:
  root_path: "."                    # project root directory
  qa_targets:                       # directories to scan for QA files (task items)
    - docs/qa
  ticket_dir: docs/ticket           # where failure tickets are written
  self_referential: false           # true = orchestrator modifies its own code (see chapter 06)
```

| Field | Required | Description |
|-------|----------|-------------|
| `root_path` | Yes | Project root; relative paths are resolved from here |
| `qa_targets` | Yes | Directories containing QA documents (`.md` files become task items) |
| `ticket_dir` | Yes | Directory for failure tickets |
| `self_referential` | No | Enables survival mechanisms when `true` (default: `false`) |

## 2. Agent

An Agent is an execution unit with declared capabilities and a shell command template.

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: coder
  description: "Code generation agent"
spec:
  capabilities:          # list of capabilities this agent provides
    - implement
    - ticket_fix
    - align_tests
  command: >-            # shell command template; {prompt} is injected at runtime
    claude --print -p '{prompt}'
  metadata:              # optional metadata for selection scoring
    cost: 100
```

| Field | Required | Description |
|-------|----------|-------------|
| `capabilities` | Yes | What this agent can do (matched against step `required_capability`) |
| `command` | Yes | Shell command template. Supports `{prompt}` placeholder (filled from StepTemplate) |
| `metadata.cost` | No | Used by agent selection strategy for cost-aware routing |

### Agent Selection

When a step requires a capability (e.g., `required_capability: implement`), the orchestrator selects an agent that declares that capability. If multiple agents match, selection considers:

- Capability match (required)
- Cost metadata (lower is preferred)
- Project-scoped agents (applied with `--project`) override global agents

## 3. StepTemplate

A StepTemplate decouples prompt content from agent definitions. The workflow step references a template by name; at runtime the template's `prompt` is injected into the agent's `{prompt}` placeholder.

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: plan
spec:
  description: "Architecture-guided implementation planning"
  prompt: >-
    You are working on the project at {source_tree}.
    Create a detailed implementation plan for: {goal}.
    Current diff: {diff}
```

| Field | Required | Description |
|-------|----------|-------------|
| `description` | No | Human-readable description |
| `prompt` | Yes | Prompt template with pipeline variable placeholders |

### Pipeline Variables

Templates can reference pipeline variables using `{variable_name}` syntax:

| Variable | Description |
|----------|-------------|
| `{goal}` | Task goal string |
| `{source_tree}` | Workspace root path |
| `{workspace_root}` | Absolute path to workspace |
| `{diff}` | Current git diff in the workspace |
| `{rel_path}` | Relative path of the current item (item-scoped steps) |
| `{qa_file_path}` | Path to QA file for current item |
| `{plan_output_path}` | Path to the plan step's output file |
| `{ticket_paths}` | Paths to active tickets for the current item |
| `{ticket_dir}` | Ticket directory path |
| `{task_id}` | Current task ID |
| `{task_item_id}` | Current task item ID |
| `{cycle}` | Current cycle number |
| `{workspace}` | Workspace ID |
| `{project}` | Project ID |
| `{workflow}` | Workflow ID |
| `{prev_stdout}` | Raw stdout from previous step |
| `{prev_stderr}` | Raw stderr from previous step |
| `{<step_id>_output}` | Output from step with given ID |
| `{prompt}` | Resolved prompt (used in Agent command templates) |

**Spill to disk**: Values exceeding 4096 bytes are automatically saved to a file, and the variable becomes `{<key>_path}` pointing to the file path instead.

## 4. Workflow

A Workflow defines a process flow: an ordered list of steps, a loop policy, and optional finalize rules.

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: qa_fix_retest
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
    - id: ticket_scan
      type: ticket_scan
      enabled: true
    - id: fix
      type: fix
      enabled: true
    - id: retest
      type: retest
      enabled: true
  loop:
    mode: once
```

Workflow configuration is detailed in [Chapter 03](03-workflow-configuration.md).

## Resource Lifecycle

### Apply (Create / Update)

```bash
# From file
./scripts/orchestrator.sh apply -f manifest.yaml

# From stdin
cat manifest.yaml | ./scripts/orchestrator.sh apply -f -

# Dry-run (validate without writing)
./scripts/orchestrator.sh apply -f manifest.yaml --dry-run
```

### Query

```bash
# List resources
./scripts/orchestrator.sh get workspaces
./scripts/orchestrator.sh get agents
./scripts/orchestrator.sh get workflows

# Detail view
./scripts/orchestrator.sh describe workspace default
./scripts/orchestrator.sh workspace info default

# Output formats
./scripts/orchestrator.sh get agents -o json
./scripts/orchestrator.sh get agents -o yaml

# Label selector
./scripts/orchestrator.sh get workspaces -l env=dev
```

### Export

```bash
# Export all config as YAML
./scripts/orchestrator.sh manifest export

# Edit interactively
./scripts/orchestrator.sh edit workspace default
```

## Multi-Document Manifests

A single YAML file can define all resources for a workflow. This is the recommended pattern:

```yaml
# everything-in-one.yaml
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
  name: mock_agent
spec:
  capabilities: [qa, fix, loop_guard]
  command: "echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: my_workflow
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
    - id: fix
      type: fix
      enabled: true
  loop:
    mode: once
```

Then apply it all at once:

```bash
./scripts/orchestrator.sh apply -f everything-in-one.yaml
```

## Next Steps

- [03 - Workflow Configuration](03-workflow-configuration.md) — step definitions, scopes, loops
- [04 - CEL Prehooks](04-cel-prehooks.md) — dynamic step gating
