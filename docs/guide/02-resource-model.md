# 02 - Resource Model

The orchestrator manages eleven core resource kinds, plus extensible Custom Resource Definitions (CRDs). All resources follow a Kubernetes-style manifest format.

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

A Workspace defines the execution and file-system context for a task. `code_repo` is the backward-compatible default; `task` is for non-code processes such as Slack operations, research, document work, and support triage.

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: my-project
spec:
  kind: code_repo                   # default
  work_dir: "."                    # project root directory
  qa_targets:                       # directories to scan for QA files (task items)
    - docs/qa
  ticket_dir: docs/ticket           # where failure tickets are written
  self_referential: false           # true = orchestrator modifies its own code (see chapter 06)
```

| Field | Required | Description |
|-------|----------|-------------|
| `kind` | No | `code_repo` (default) or `task` |
| `work_dir` | Conditional | Required for `code_repo`; optional for `task`. `root_path` is accepted as a legacy input alias |
| `qa_targets` | Conditional | Required for `code_repo`; forbidden for `task` |
| `ticket_dir` | Conditional | Required for `code_repo`; forbidden for `task` |
| `self_referential` | No | Enables survival mechanisms when `true` (default: `false`) |

A non-code workspace may use a persistent shared directory:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: warehouse-ops
spec:
  kind: task
  work_dir: ~/warehouse-data
```

If `work_dir` is omitted, the daemon creates a private `0700` HOME/cwd for each task and removes it when the task reaches a terminal state. Task workspaces always have one implicit `__TASK__` item and do not scan QA files. Any host path used by a task workspace or its ExecutionProfile must be below the operator's `fileSharing.shareableRoots` ceiling. See [Non-code Workspaces and Global Skills](non-code-workspace.md).

## 2. Agent

An Agent is an execution unit with declared capabilities and an explicit provider driver. `spec.driver` is required: FR-173 removed the promotion that used to accept a command-only manifest, so Apply now refuses one and the diagnostic names the block to write.

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
  driver:
    provider: claude
    transport: cli
    options:
      model: sonnet
      maxTurns: 8
      permissionMode: governed
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
```

| Field | Required | Description |
|-------|----------|-------------|
| `capabilities` | Yes | What this agent can do (matched against step `required_capability`) |
| `command` | Conditional | Shell command template; required for an explicit `shell/cli` driver and omitted for Claude/Codex drivers. A `command` without a `driver` is refused, not promoted |
| `driver` | Yes | Typed provider/transport adapter (`shell`, `claude`, or `codex`; CLI transport is executable) |
| `metadata.cost` | No | Used by agent selection strategy for cost-aware routing |
| `metadata.description` | No | Human-readable description of the agent |
| `selection` | No | Agent selection strategy override (see below) |
| `env` | No | Environment variables: direct values, `fromRef` (import all from store), or `refValue` (single key from store) |
| `promptDelivery` | No | How the rendered prompt reaches an explicit shell driver: `stdin`, `file`, `env`, or `arg` (default: `arg`) |

### Agent Drivers

Explicit drivers keep provider flags out of manifests and emit normalized tool, permission, usage, and terminal events. Workflow steps declare their needs under `behavior.driverRequirements`; incompatible candidate Agents fail during apply. See the bilingual [Agent Driver Model](agent-driver-model.md) for complete fields, security boundaries, migration, and examples.

```yaml
spec:
  capabilities: [implement]
  driver:
    provider: claude
    transport: cli
    options:
      model: sonnet
      permissionMode: ask
      allowedTools: [mcp__orch]
```

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

List and read them like any other kind:

```bash
orchestrator get projects
orchestrator get project/my-project
```

Project is the one kind that is **not** project-scoped, so `--project` does not
narrow either query — the list is the same whatever scope you ask from. The
project whose name is empty is skipped: a blank id is a structural artefact, not
a project someone created.

## 6. RuntimePolicy

A RuntimePolicy configures runner behavior, resume strategy, and observability.

It is a **resolved singleton**, not a collection, and that changes how you read
it. A single read always answers — with the policy actually in effect for the
project, resolved as project → `_system` → built-in defaults — whatever name you
ask for:

```bash
orchestrator get runtimepolicy/default   # the effective policy
```

There is deliberately no `orchestrator get runtimepolicies`. Stored records do
exist — one per scope, since a project may override `_system` — but listing them
would not answer the question anyone actually asks. What governs a task is the
*resolved* policy, and no stored row holds it: it is composed from the chain at
read time, which is why a single read answers for any name and why a
RuntimePolicy cannot be deleted. A list of rows would show you the overrides
while leaving the effective policy unstated. For the same reason it does not
appear in the Console's resource catalog.

```yaml
apiVersion: orchestrator.dev/v2
kind: RuntimePolicy
metadata:
  name: default
spec:
  runner:
    shell: /bin/bash
    policy_mode: strict
    # … allowed_shells, env_allowlist, redaction_patterns
  resume: { ... }
  observability: { ... }
```

`runner.executor` no longer exists. It was a parse-only compatibility field whose only accepted value selected nothing, and FR-173 removed it at the v0.7 window; `RunnerSpec` now declares `deny_unknown_fields`, so a manifest still carrying it is refused by name rather than having the key quietly dropped. Provider execution belongs to each Agent's `spec.driver`; use `shell/cli`, `claude/cli`, or `codex/cli`.

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

> **Four things in this system are called a "Store", and they are unrelated.** `EnvStore` and `SecretStore` are built-in kinds holding key/value data an agent reads as environment (below). `WorkflowStore` is **not** a built-in kind — it is a CRD providing cross-task persistent memory, described in [05 - Advanced Features](05-advanced-features.md#persistent-store-wp01). `StoreBackendProvider` is the pluggable backend a `WorkflowStore` names, and is not a store at all. An EnvStore is not a smaller WorkflowStore: nothing an agent writes at runtime lands in either EnvStore or SecretStore.

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

A SecretStore has the same spec structure as EnvStore. The two are not interchangeable, and the `kind` is not a label — it is the switch three behaviours read:

| | EnvStore | SecretStore |
|---|---|---|
| Spec at rest | Stored as plaintext JSON | AEAD-encrypted, bound to the project and resource name |
| Export and overview | Values shown | Values replaced with a placeholder before leaving the daemon |
| Key operations | None | `orchestrator secret key status\|list\|rotate\|revoke\|history\|bootstrap` |

Choosing the wrong kind therefore has consequences you cannot see in the manifest: a secret written into an EnvStore is stored in the clear and is printed by export. Moving it later is not a rename — you must delete the EnvStore and apply a SecretStore, because the audit trail records `resource.env_store.apply` and `resource.secret_store.apply` as distinct actions and those action names are permanent. Since FR-167 the delete half is recorded the same way, as `resource.env_store.delete` and `resource.secret_store.delete`, so the move leaves a complete two-row trail rather than a create with no matching removal.

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

## 10. Trigger

A Trigger enables automatic task creation. For the cron case it follows the Kubernetes CronJob mental model.

**A Trigger carries four distinct jobs**, and it is worth knowing all four before you decide this kind is simple. `spec.cron` is one; the other three are all `spec.event`, distinguished by `event.source`:

| Job | Declared by | What it does |
|---|---|---|
| Schedule | `spec.cron` | Fires on a 5-field cron schedule in a named timezone |
| Task lifecycle | `spec.event.source: task_completed` / `task_failed` | Fires when a matching task ends |
| Webhook endpoint and credential holder | `spec.event.source: webhook` + `spec.event.webhook` | Owns installation identity, the signing secret, the outbound provider credential and the external-actor-to-role mapping |
| Filesystem watcher | `spec.event.source: filesystem` + `spec.event.filesystem` | Watches workspace-relative paths for create/modify/delete, with a debounce window |

The webhook job is the one that surprises people: a Trigger is where Slack credentials and actor roles live, which is why a [SourceTaskBinding](#12-sourcetaskbinding) references a Trigger through `triggerRef` rather than holding credentials itself. FR-166 evaluated splitting the credential-holding job into its own kind and decided against it: the identity a binding needs and the endpoint the webhook needs are the same installation, and separating them would put a mandatory join between a delivery and its own actor roles. The decision is permanent in one respect worth stating — `resource.trigger.apply` is already written into `control_action_audit`, and recorded audit action names are never renamed.

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: nightly-qa
spec:
  cron:
    schedule: "0 2 * * *"             # 5-field cron: min hour dom month dow
    timezone: "Asia/Shanghai"          # IANA timezone (optional, default UTC)
  action:
    workflow: full-qa                  # workflow to run
    workspace: main-workspace          # workspace for the task
  concurrencyPolicy: Forbid            # Allow | Forbid | Replace
  suspend: false
  historyLimit:
    successful: 5
    failed: 3
```

| Field | Required | Description |
|-------|----------|-------------|
| `cron` | One of cron/event | Cron schedule with optional timezone |
| `event` | One of cron/event | Event-driven trigger (source + filter) |
| `action.workflow` | Yes | Workflow to run when triggered |
| `action.workspace` | Yes | Workspace for the created task |
| `concurrencyPolicy` | No | `Allow` (default), `Forbid` (skip if active task), `Replace` (cancel active + create new) |
| `suspend` | No | Pause the trigger without deleting (default: `false`) |
| `historyLimit.successful` | No | Completed tasks to keep for this trigger. **No default** — omit `historyLimit` and nothing is ever pruned |
| `historyLimit.failed` | No | Failed tasks to keep for this trigger, counted separately from `successful` |

`historyLimit` prunes each trigger's own tasks by name and project, deleting the task with its
items, command runs, events and log files. A task that is still referenced by handoff, resume or
source-ingest records is left untouched and reported in the daemon log
(`history limit skipped a task still referenced elsewhere`, naming the table); those records are
not deleted by a retention limit. Every sweep also logs one `trigger history cleanup` line with
how many tasks it selected, deleted and skipped.

### Event Trigger

An event trigger fires when a matching task lifecycle event occurs:

```yaml
spec:
  event:
    source: task_completed             # task_completed | task_failed
    filter:
      workflow: build-pipeline         # only match tasks from this workflow
  action:
    workflow: deploy
    workspace: prod
```

### Webhook Trigger

`source: webhook` makes the Trigger an authenticated external endpoint. This is where installation identity, the signature secret, the outbound provider credential and the actor-to-role mapping live:

```yaml
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: T012345
      actorRoles: {U012345: operator}    # external actor -> role; requests cannot supply their own
      reactionRouting: bindings          # default: disabled
      secret: {fromRef: slack-signing}   # SecretStore holding the signing secret
      outboundCredential: {fromRef: slack-api, key: BOT_TOKEN}
  action:
    workflow: analyze
    workspace: main-workspace
```

`secret` and `outboundCredential` are separate same-project SecretStore references. Use `connectionRef` instead of both when a managed SourceConnection owns the credentials. See [SourceTaskBinding](#12-sourcetaskbinding) for how a reaction is routed to a template.

### Filesystem Trigger

`source: filesystem` watches paths relative to the Workspace `root_path`:

```yaml
spec:
  event:
    source: filesystem
    filesystem:
      paths: [docs/ticket]
      events: [create, modify]           # create | modify | delete; empty means all three
      debounce_ms: 500                   # default 500; snake_case here, unlike the webhook fields
  action:
    workflow: ticket-fix
    workspace: main-workspace
```

Debouncing coalesces a burst of writes into one firing. A tool that writes a file in several passes would otherwise create several tasks.

### Trigger Lifecycle

```bash
orchestrator trigger suspend <name>    # pause trigger
orchestrator trigger resume <name>     # resume trigger
orchestrator trigger fire <name>       # manually fire (create task immediately)
orchestrator get triggers              # list all triggers
orchestrator delete trigger/<name>     # remove trigger
```

## 11. SourceTaskTemplate

A SourceTaskTemplate is a project-scoped recipe for turning verified source evidence into a future task goal and action. It is separate from StepTemplate: SourceTaskTemplate describes task creation, while StepTemplate describes an agent prompt inside a workflow step.

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: docs-from-slack
spec:
  skill:
    name: docs
    invocation: "$docs"
    args: ["--concise"]
  action:
    workflow: slack-documentation
    workspace: main
    start: true
    initial_vars:
      origin: slack
  goalTemplate: >-
    {skill_invocation}: use {source_message_url} as the source request
  allowedVariables: [skill_invocation, source_message_url]
```

The renderer accepts only exact allowlisted variables, evaluates once, and requires Slack sample URLs to be HTTPS `slack.com` permalinks under `/archives/`. Preview uses the same daemon renderer as future live routing and does not create a task or source record:

```bash
orchestrator source template preview docs-from-slack \
  --project my-project --provider slack --installation primary \
  --message-url https://example.slack.com/archives/C123/p1234567890000100 \
  -o json
```

Normal deletion is blocked while a `SourceTaskBinding` references the template. Administrators can explicitly remove the template and references atomically with `--force --force-references`; that operation is audited.

## 12. SourceTaskBinding

SourceTaskBinding is a project-scoped exact policy that selects one SourceTaskTemplate from authenticated Slack reaction evidence. It references a same-project Slack webhook Trigger, which owns installation identity and the external-actor-to-role mapping.

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-code-analysis
spec:
  triggerRef: slack-main
  match:
    eventKind: reaction_added
    reaction: agent-analyze
    targetKind: message
    channels: [C01234567]
  templateRef: analyze-from-slack
  allowedActorRoles: [operator, admin]
  suspend: false
```

Choose exactly one channel policy: a non-empty `channels` list, or explicit `allChannels: true`. Roles are required and are resolved from Trigger `actorRoles`; source requests cannot supply a role. Enabled overlapping rules are rejected instead of ranked.

Trigger reaction routing is opt-in:

```yaml
event:
  source: webhook
  webhook:
    provider: slack
    installationId: T012345
    actorRoles: {U012345: operator}
    reactionRouting: bindings
    secret: {fromRef: slack-signing}
    outboundCredential: {fromRef: slack-api, key: BOT_TOKEN}
```

`reactionRouting` defaults to `disabled`. Validate a rollout without side effects:

```bash
orchestrator source binding simulate --project my-project \
  --provider slack --installation T012345 --reaction agent-analyze \
  --channel C01234567 --actor U012345 -o json
orchestrator source binding suspend slack-code-analysis --project my-project
orchestrator source binding resume slack-code-analysis --project my-project
```

Simulation never calls Slack, renders a template, or creates a task. When `reactionRouting: bindings` is active, a live signed reaction asynchronously resolves `channel + message_ts` through Slack `chat.getPermalink`, renders the selected frozen template, and creates one deterministic canonical task. Duplicate deliveries and daemon restart converge on the same message/reaction/binding route and task.

The signing `secret` and `outboundCredential` are separate same-project SecretStore references. The daemon resolves the outbound token only for the Slack API call; public source summaries, audit, timeline, and task inputs never contain it. Read-only users can inspect safe automation status/template/binding fields. Operators can explicitly retrieve the protected Slack link:

```bash
orchestrator source list --project my-project -o json
orchestrator source route <source-event-id> -o json
```

Set `reactionRouting: disabled` to stop new badge routes without deleting existing tasks or evidence. Normal deletion of a referenced Trigger or SourceTaskTemplate is blocked; Admin `--force --force-references` removes the references atomically and records audit evidence.

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
  work_dir: "."
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
