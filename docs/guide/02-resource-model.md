# 02 - Resource Model

The orchestrator manages nine core resource kinds, plus extensible Custom Resource Definitions (CRDs). All resources follow a Kubernetes-style manifest format.

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
  annotations:                          # optional
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
    description: "Primary code generation agent"
  selection:             # optional selection strategy override
    strategy: CapabilityAware    # default
  env:                   # optional environment variables
    - name: LOG_LEVEL
      value: "debug"
    - fromRef: shared-config     # import all keys from an EnvStore
    - name: MY_API_KEY
      refValue:                  # import a single key from a SecretStore
        name: api-keys
        key: OPENAI_API_KEY
  promptDelivery: arg    # how the prompt reaches the agent (default: arg)
```

| Field | Required | Description |
|-------|----------|-------------|
| `capabilities` | Yes | What this agent can do (matched against step `required_capability`) |
| `command` | Yes | Shell command template. Supports `{prompt}` placeholder (filled from StepTemplate) |
| `metadata.cost` | No | Used by agent selection strategy for cost-aware routing |
| `metadata.description` | No | Human-readable description of the agent |
| `selection` | No | Agent selection strategy override (see below) |
| `env` | No | Environment variables: direct values, `fromRef` (import all from store), or `refValue` (single key from store) |
| `promptDelivery` | No | How the rendered prompt reaches the agent: `stdin`, `file`, `env`, or `arg` (default: `arg`) |

### Agent Selection

When a step requires a capability (e.g., `required_capability: implement`), the orchestrator selects an agent that declares that capability. If multiple agents match, selection considers:

- Capability match (required)
- Selection strategy scoring (configurable per agent)
- Cost metadata (lower is preferred)
- Project-scoped agents (applied with `--project`) are used exclusively — no fallback to global agents

#### Selection Strategies

| Strategy | Description |
|----------|-------------|
| `CostBased` | Static cost-based sorting |
| `SuccessRateWeighted` | Weighted by historical success rate |
| `PerformanceFirst` | Latency-focused selection |
| `Adaptive` | Configurable weights across cost, success rate, performance, and load |
| `LoadBalanced` | Favors agents with lower current load |
| `CapabilityAware` | Adaptive scoring with health-aware capability tracking **(default)** |

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

## 5. Project

A Project provides an isolation domain for resources. All resource commands accept `--project` to scope operations.

```yaml
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: my-project
spec:
  description: "Frontend rewrite project"
```

## 6. RuntimePolicy

A RuntimePolicy configures runner behavior, resume strategy, and observability.

```yaml
apiVersion: orchestrator.dev/v2
kind: RuntimePolicy
metadata:
  name: default
spec:
  runner: { ... }
  resume: { ... }
  observability: { ... }
```

## 7. ExecutionProfile

An ExecutionProfile defines the sandbox/host execution boundary for agent steps. Defaults: `mode: host`, `fs_mode: inherit`, `network_mode: inherit`.

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  mode: sandbox                    # host | sandbox
  fs_mode: workspace_rw_scoped     # inherit | workspace_rw_scoped
  writable_paths: [src, docs]
  network_mode: deny               # inherit | deny | allowlist
```

## 8. EnvStore

An EnvStore holds reusable environment variable sets that agents can reference via `env.fromRef`.

```yaml
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: shared-config
spec:
  data:
    DATABASE_URL: "postgres://localhost/mydb"
    LOG_LEVEL: "debug"
```

## 9. SecretStore

A SecretStore has the same structure as EnvStore but is intended for sensitive values. The `kind` field distinguishes them at the resource level.

```yaml
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: api-keys
spec:
  data:
    OPENAI_API_KEY: "sk-..."
```

Agents reference stores via `env` entries (see Agent spec above).

## Resource Lifecycle

### Apply (Create / Update)

```bash
# From file
orchestrator apply -f manifest.yaml

# From stdin
cat manifest.yaml | orchestrator apply -f -

# Dry-run (validate without writing)
orchestrator apply -f manifest.yaml --dry-run
```

### Query

```bash
# List resources
orchestrator get workspaces
orchestrator get agents
orchestrator get workflows

# Detail view
orchestrator describe workspace/default

# Output formats
orchestrator get agents -o json
orchestrator get agents -o yaml

# Label selector
orchestrator get workspaces -l env=dev
```

### Export

```bash
# Export all config as YAML
orchestrator manifest export
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
orchestrator apply -f everything-in-one.yaml
```

## Next Steps

- [03 - Workflow Configuration](03-workflow-configuration.md) — step definitions, scopes, loops
- [04 - CEL Prehooks](04-cel-prehooks.md) — dynamic step gating
