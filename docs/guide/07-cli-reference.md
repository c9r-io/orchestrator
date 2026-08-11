# 07 - CLI Reference

Quick-reference for all Agent Orchestrator CLI commands.

## Entry Points

| Binary | Description |
|--------|-------------|
| `orchestratord` | gRPC daemon — server + embedded workers |
| `orchestrator` | CLI client — lightweight gRPC calls over Unix socket |

The daemon holds all state (engine, DB, task queue). The CLI is a thin RPC client.

## Global Options

| Flag | Description |
|------|-------------|
| `-v, --verbose` | Enable verbose output |
| `-h, --help` | Print help |
| `-V, --version` | Print version |
| `--control-plane-config <path>` | Override control-plane client config (env: `ORCHESTRATOR_CONTROL_PLANE_CONFIG`) |

## Command Aliases

| Command | Alias |
|---------|-------|
| `agent` | `ag` |
| `agent list` | `agent ls` |
| `apply` | `ap` |
| `check` | `ck` |
| `db migrations list` | `db migrations ls` |
| `debug` | `dbg` |
| `delete` | `rm` |
| `describe` | `desc` |
| `event` | `ev` |
| `event list` | `event ls` |
| `get` | `g` |
| `guide` | `gd` |
| `secret key list` | `secret key ls` |
| `store list` | `store ls` |
| `task` | `t` |
| `task create` | `task new` |
| `task delete` | `task rm` |
| `task info` | `task get` |
| `task list` | `task ls` |
| `task logs` | `task log` |
| `trigger` | `tg` |

## Initialization & Configuration

### init

Create runtime directories and SQLite schema.

```bash
orchestrator init
```

### apply

Load resources from a YAML manifest into the database.

```bash
# From file
orchestrator apply -f manifest.yaml

# From stdin
cat manifest.yaml | orchestrator apply -f -

# Dry-run (validate only)
orchestrator apply -f manifest.yaml --dry-run

# Project-scoped apply
orchestrator apply -f manifest.yaml --project my-project
```

### check

Preflight validation: cross-reference agents, workflows, and templates.

```bash
orchestrator check
orchestrator check --workflow self-bootstrap
orchestrator check --project my-project
orchestrator check -o json
```

| Flag | Description |
|------|-------------|
| `--workflow <WORKFLOW>` | Check a specific workflow |
| `-o, --output` | Output format: table (default), json, yaml |
| `-p, --project` | Project filter |

## Resource Queries

### get

List resources (kubectl-style).

```bash
orchestrator get workspaces
orchestrator get agents
orchestrator get workflows

# Output format
orchestrator get agents -o json
orchestrator get agents -o yaml

# Project-scoped query
orchestrator get agents --project my-project

# Label selector
orchestrator get agents -l env=dev
```

| Flag | Description |
|------|-------------|
| `-o, --output` | Output format: table (default), json, yaml |
| `-l, --selector` | Label selector filter |
| `-p, --project` | Project filter |

### describe

Detailed view of a single resource.

```bash
orchestrator describe workspace/default
orchestrator describe agent/coder

# Project-scoped
orchestrator describe agent/my-agent --project my-project
```

### delete

Delete a resource by kind/name.

```bash
orchestrator delete workspace/my-ws --force
orchestrator delete agent/old-agent --force

# Dry-run
orchestrator delete agent/old-agent --dry-run

# Project-scoped
orchestrator delete agent/old --force --project my-project
```

| Flag | Description |
|------|-------------|
| `-f, --force` | Force delete without confirmation |
| `--dry-run` | Show what would be deleted |
| `-p, --project` | Project filter |

## Task Lifecycle

### task create

```bash
orchestrator task create \
  --name "my-task" \
  --goal "Implement feature X" \
  --workflow self-bootstrap \
  --project my-project \
  --workspace default \
  --target-file docs/qa/01-test.md    # can specify multiple times

# Step filtering: only run specific steps from the workflow
orchestrator task create \
  --workflow sdlc --project my-project \
  --step fix \
  --set ticket_paths=docs/ticket/T-0042.md

# Multiple steps (executed in workflow order)
orchestrator task create \
  --workflow sdlc --step plan --step implement
```

| Flag | Description |
|------|-------------|
| `-n, --name` | Task name |
| `-g, --goal` | Task goal/description |
| `-p, --project` | Project ID |
| `-w, --workspace` | Workspace ID |
| `-W, --workflow` | Workflow ID |
| `-t, --target-file` | Target files (repeatable) |
| `--no-start` | Create without auto-starting |
| `-S, --step` | Execute only specified step IDs (repeatable) |
| `--set` | Inject pipeline variable as `key=value` (repeatable) |

### run

Synchronous step execution — creates a task, follows logs, and exits with status code.

```bash
# Synchronous execution with step filter
orchestrator run \
  --workflow sdlc --step fix \
  --set ticket_paths=docs/ticket/T-0042.md

# Background mode (equivalent to task create)
orchestrator run --workflow sdlc --step fix --detach

# Direct assembly mode: execute a StepTemplate without a workflow
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --set ticket_paths=docs/ticket/T-0042.md
```

| Flag | Description |
|------|-------------|
| `-W, --workflow` | Workflow ID (required unless `--template` is specified) |
| `-S, --step` | Execute only specified step IDs (repeatable) |
| `--set` | Inject pipeline variable as `key=value` (repeatable) |
| `-p, --project` | Project ID |
| `-w, --workspace` | Workspace ID |
| `-t, --target-file` | Target files (repeatable) |
| `--detach` | Run in background (print task ID and return) |
| `--template` | StepTemplate name (direct assembly mode) |
| `--agent-capability` | Agent capability for direct assembly mode |
| `--profile` | ExecutionProfile override for direct assembly mode |

### task list / info

```bash
orchestrator task list
orchestrator task list -o json
orchestrator task list --project my-project    # filter by project
orchestrator task list --status running        # filter by status
orchestrator task list -v                      # verbose output

orchestrator task info <task_id>
orchestrator task info <task_id> -o yaml
```

| Flag (list) | Description |
|-------------|-------------|
| `-s, --status` | Filter by task status |
| `-p, --project` | Project filter |
| `-o, --output` | Output format: table (default), json, yaml |
| `-v, --verbose` | Verbose output |

### task items

List the items of a task and their individual status.

```bash
orchestrator task items <task_id>
orchestrator task items <task_id> --status running
orchestrator task items <task_id> -o json
```

| Flag | Description |
|------|-------------|
| `-s, --status` | Filter by item status |
| `-o, --output` | Output format: table (default), json, yaml |

### task recover

Recover orphaned running items (e.g. after a crash).

```bash
orchestrator task recover <task_id>
```

### task start / pause / resume

```bash
orchestrator task start <task_id>
orchestrator task start --latest             # start the most recent task

orchestrator task pause <task_id>
orchestrator task resume <task_id>
orchestrator task resume <task_id> --reset-blocked   # reset blocked items back to unresolved
```

| Flag (start) | Description |
|--------------|-------------|
| `-l, --latest` | Start the latest task |

| Flag (resume) | Description |
|---------------|-------------|
| `--reset-blocked` | Reset blocked items back to unresolved |

### task logs / watch / trace

```bash
# View execution logs
orchestrator task logs <task_id>
orchestrator task logs <task_id> --follow --timestamps
orchestrator task logs <task_id> --tail 50

# Live watch (auto-refreshing status panel)
orchestrator task watch <task_id>
orchestrator task watch <task_id> --interval 5

# Execution trace with anomaly detection
orchestrator task trace <task_id>
orchestrator task trace <task_id> --verbose -o json
```

| Flag (logs) | Description |
|-------------|-------------|
| `-f, --follow` | Follow logs in real-time |
| `-n, --tail` | Number of lines to show (default: 100) |
| `--timestamps` | Include timestamps |

| Flag (watch) | Description |
|--------------|-------------|
| `--interval` | Update interval in seconds (default: 2) |
| `--timeout <SECONDS>` | Exit after N seconds (0 = no timeout, default: 0) |

| Flag (trace) | Description |
|--------------|-------------|
| `--verbose` | Verbose trace output |
| `-o, --output` | Output format: table (default), json, yaml |

### task timeline

Show the semantic process timeline for a task — goal, execution, evidence, failure, and state transitions with stable pagination.

```bash
orchestrator task timeline <task_id>                       # first timeline page
orchestrator task timeline <task_id> --category failure --follow
orchestrator task timeline <task_id> -o json
```

| Flag | Description |
|------|-------------|
| `--cursor` | Resume from a pagination cursor |
| `-l, --limit` | Entries per page (default: 50) |
| `--category` | Filter by entry category |
| `-f, --follow` | Follow new timeline entries |
| `-o, --output` | Output format: table (default), json, yaml |

### task retry

Retry a failed task item.

```bash
orchestrator task retry <task_item_id> [--force]
```

### task delete

```bash
orchestrator task delete <task_id> --force
orchestrator task delete <id1> <id2> <id3> --force   # multiple task IDs
orchestrator task delete --all --force                # delete all tasks
orchestrator task delete --all --status completed     # delete all with status filter
orchestrator task delete --all --project my-project   # delete all in a project
```

| Flag | Description |
|------|-------------|
| `-f, --force` | Force delete without confirmation |
| `--all` | Delete all tasks |
| `--status <STATUS>` | Filter by status (used with `--all`) |
| `--project <PROJECT>` | Filter by project (used with `--all`) |

## Attention Queue

Cross-task human attention queue — only workflow conditions that need a human decision, ordered by severity and ownership. All queue mutations are authenticated, version-checked (`--expected-version`), and accept a retry-stable `--idempotency-key`.

```bash
orchestrator attention list                                # active inbox
orchestrator attention list --assignee me                  # items assigned to the current actor
orchestrator attention list --state resolved -o json       # audit resolved decisions
orchestrator attention get <id>                            # inspect one item
orchestrator attention claim <id> --expected-version 1
orchestrator attention snooze <id> --expected-version 2 --until 2026-07-13T09:00:00Z
orchestrator attention resolve <id> --expected-version 2 --reason reviewed
orchestrator attention action <id> resume_task --expected-version 1
orchestrator attention follow --after 42                   # stream inbox deltas (NDJSON)
```

| Subcommand | Description |
|------------|-------------|
| `list` | List attention items with optional filters (`--project`, `--state`, `--kind`, `--severity`, `--assignee`, `--task`, `--limit`). The full `--kind` vocabulary is the generated routing table in [Where Failures Go](03-workflow-configuration.md#where-failures-go) |
| `get` | Show the redacted condition, optimistic version, task context, and safe allowlisted actions |
| `claim` | Take ownership of an open item |
| `snooze` | Defer an open or claimed item until an RFC3339 deadline (`--until`) |
| `resolve` | Close an item with an audit reason (`--reason`) |
| `action` | Reserve and execute only an action advertised by the item, such as `retry_failed_item` or `resume_task` (`--input` supplies JSON action input) |
| `follow` | Follow monotonic queue changes from a durable change sequence (`--after`); streaming output is `-o json` (default, NDJSON) or `-o yaml` |

## Handoff & Resume

### handoff

Generate and inspect immutable task handoff snapshots for transferring context between agents or sessions.

```bash
orchestrator handoff generate <task_id>                    # snapshot at the latest event cursor
orchestrator handoff generate <task_id> --cursor 42 -o json  # snapshot at a selected event cursor
orchestrator handoff get <handoff_id>                      # retrieve one snapshot
```

### resume

Preview and execute safe logical resume operations.

```bash
orchestrator resume boundaries <task_id>                   # boundaries + side-effect classifications
orchestrator resume plan <task_id> --boundary <boundary_id> --mode <mode>
orchestrator resume execute <plan_id> --expected-state-version 3 \
  --reason "reviewed preview" --idempotency-key resume-1
```

| Subcommand | Description |
|------------|-------------|
| `resume boundaries` | List a task's logical boundaries and their side-effect classifications |
| `resume plan` | Persist an expiring consequence preview without changing task or workspace state (`--attention-item` links an attention item) |
| `resume execute` | Execute a previously reviewed plan with stale-state protection; requires `--expected-state-version`, `--reason`, and `--idempotency-key`; elevated plans need `--elevated-confirmation` |

## Manifest

```bash
# Validate a manifest file
orchestrator manifest validate -f manifest.yaml
orchestrator manifest validate -f manifest.yaml --project my-project

# Export all resources as manifest documents
orchestrator manifest export [-o yaml|json]
```

| Flag (validate) | Description |
|-----------------|-------------|
| `-f, --file` | Manifest file (required) |
| `-p, --project` | Project filter |

## Secret Key Management

```bash
orchestrator secret key status [-o json]
orchestrator secret key list [-o json]
orchestrator secret key rotate [--resume]
orchestrator secret key revoke <key_id> [--force]
orchestrator secret key bootstrap                 # emergency recovery when all keys are terminal
orchestrator secret key history [-n <limit>] [--key-id <id>] [-o json]
```

## Database Operations

```bash
orchestrator db status [-o json]
orchestrator db migrations list [-o json]
orchestrator db vacuum                            # reclaim disk space (VACUUM)
orchestrator db cleanup                           # clean up old log files from terminated tasks
orchestrator db cleanup --older-than 30           # logs older than N days (default 30)
```

## Project Cleanup

Use `orchestrator delete project/<id> --force` for project cleanup.

## Project Management

Project isolation is native — use `--project` on `apply`, `get`, `describe`, `delete`, `task create`, `task list`, and `store` commands.

```bash
# Apply resources to a project scope
orchestrator apply -f manifest.yaml --project my-project

# Explicitly prune resources omitted from the manifest
orchestrator apply -f manifest.yaml --project my-project --prune

# Query project-scoped resources
orchestrator get agents --project my-project

# Delete a project and all its data (tasks, items, runs, events, config)
orchestrator delete project/<project> --force
```

Default `apply` is merge-only: resources omitted from the manifest are preserved.
Use `--prune` only when you want omitted resources of the same applied kinds to be deleted
within the target project.

## Persistent Store

```bash
orchestrator store get <store_name> <key>
orchestrator store put <store_name> <key> <value>
orchestrator store put <store_name> <key> <value> --task-id <id>
orchestrator store delete <store_name> <key>
orchestrator store list <store_name>
orchestrator store list <store_name> --limit 50 --offset 10
orchestrator store prune <store_name>

# Project-scoped store
orchestrator store get <store_name> <key> --project my-project
orchestrator store put <store_name> <key> <value> --project my-project
```

| Flag (list) | Description |
|-------------|-------------|
| `-l, --limit` | Result limit (default: 100) |
| `--offset` | Result offset (default: 0) |
| `-o, --output` | Output format: table (default), json, yaml |
| `-p, --project` | Project filter |

| Flag (put) | Description |
|------------|-------------|
| `-t, --task-id` | Associated task ID |
| `-p, --project` | Project filter |

## Agent Lifecycle

Manage agent scheduling state (cordon, drain, uncordon).

```bash
# List agents with lifecycle state
orchestrator agent list
orchestrator agent list --project my-project -o json

# Cordon: mark agent as unschedulable (no new work dispatched)
orchestrator agent cordon <agent_name>
orchestrator agent cordon <agent_name> --project my-project

# Uncordon: mark a cordoned agent as schedulable again
orchestrator agent uncordon <agent_name>

# Drain: cordon + wait for in-flight work to complete
orchestrator agent drain <agent_name>
orchestrator agent drain <agent_name> --timeout 60
```

| Subcommand | Description |
|------------|-------------|
| `list` | List agents and their lifecycle state |
| `cordon` | Mark an agent as unschedulable |
| `uncordon` | Mark a cordoned agent as schedulable again |
| `drain` | Cordon + wait for in-flight work to complete |

| Flag | Description |
|------|-------------|
| `-p, --project` | Project filter |
| `-o, --output` (list only) | Output format: table (default), json, yaml |
| `--timeout` (drain only) | Timeout in seconds; force-drain after this duration |

### Agent Sessions

Observe and control interactive agent sessions. Writer control is a fenced lease: writer mutations require the current `--fencing-token`, and inputs carry a retry-stable `--idempotency-key`.

```bash
orchestrator agent session list --state detached -o json   # list observable sessions
orchestrator agent session get <session_id>                # lifecycle, process, and lease metadata
orchestrator agent session attach <session_id> --mode writer --client-id terminal-a
orchestrator agent session read <session_id> --offset 0 --chunks-json
orchestrator agent session heartbeat <session_id> --client-id terminal-a --fencing-token 1
orchestrator agent session send-input <session_id> --client-id terminal-a --fencing-token 1 \
  --text hello --idempotency-key input-1
orchestrator agent session detach <session_id> --mode writer --client-id terminal-a --fencing-token 1
orchestrator agent session close <session_id> --reason done --expected-version 2 --idempotency-key close-1
orchestrator agent session resolve --pid 1234 -o json      # diagnostic PID -> sessions (read-only)
```

| Subcommand | Description |
|------------|-------------|
| `list` | List daemon-authoritative sessions with `--task`, `--agent`, `--state` filters, without exposing transport paths or command text |
| `get` | Show public lifecycle, process, and writer lease metadata for one session |
| `attach` | Attach as a reader (`--mode reader`, default, read-only) or explicitly acquire the fenced writer lease (`--mode writer`, requires operator authority and an enabled session-control policy) |
| `read` | Follow or read transcript bytes from a client-owned `--offset`; `--chunks-json` emits structured chunks with `next_offset` for reconnect-safe streaming |
| `heartbeat` | Renew a writer lease; only the current unexpired client and fencing token can extend it |
| `send-input` | Send bounded input to a live session with the current writer fencing token |
| `detach` | Detach a reader or writer; writer detach requires the exact current fencing token |
| `close` | Close the backing session process — session-ID addressed, version-aware (`--expected-version`), audited (`--reason`), never authorized by PID alone |
| `resolve` | Resolve a diagnostic PID to sessions; read-only and never creates mutation authority |

## Daemon Lifecycle

```bash
orchestrator daemon status                    # show daemon PID and status
orchestrator daemon stop                      # send SIGTERM to daemon
orchestrator daemon maintenance --enable      # block new task creation
orchestrator daemon maintenance --disable     # allow task creation again
```

## Event Lifecycle

```bash
orchestrator event stats                      # show event table statistics
orchestrator event list --task <task_id>      # list events for a task
orchestrator event list --task <task_id> --type item --limit 100   # filter by event type prefix
orchestrator event cleanup                    # clean up old events
orchestrator event cleanup --older-than 30    # events older than N days (default 30)
orchestrator event cleanup --dry-run          # preview without deleting
orchestrator event cleanup --archive          # archive to JSONL before deleting
```

| Flag (list) | Description |
|-------------|-------------|
| `--task <TASK>` | Task identifier (required) |
| `--type <EVENT_TYPE>` | Filter by event type (prefix match) |
| `-l, --limit` | Maximum events to return (default: 50) |
| `-o, --output` | Output format: table (default), json, yaml |

## Audit

Query canonical control-plane action audit evidence — project-scoped mutation records that correlate transport authorization, domain mutation, and event evidence, without exposing request bodies or secrets.

```bash
orchestrator audit list --project demo --status failed     # list failed mutations
orchestrator audit list --project demo --target-type attention_item -o json
orchestrator audit get req-123 --project demo              # one record by request ID
```

| Flag (list) | Description |
|-------------|-------------|
| `-p, --project` | Project scope (required) |
| `--actor` | Filter by acting identity |
| `--target-type` / `--target-id` | Filter by mutation target |
| `--action` | Filter by action name |
| `--status` | Filter by outcome status |
| `--from` / `--to` | Time range bounds |
| `-l, --limit` | Maximum records (default: 100) |
| `-o, --output` | Output format: table (default), json, yaml |

## Trigger Lifecycle

```bash
orchestrator trigger suspend <name>           # suspend a trigger
orchestrator trigger resume <name>            # resume a suspended trigger
orchestrator trigger fire <name>              # manually fire a trigger once
orchestrator trigger fire <name> --payload '{"key":"value"}'   # fire with JSON payload
```

All trigger subcommands accept the `--project` flag for project-scoped operation.

## Source Integration

External source events (e.g. Slack) and their task bindings, durable automation routes, governed templates, and provider connections.

### Source events

```bash
orchestrator source list --state failed                    # list replay candidates
orchestrator source list --project demo --limit 20 -o json
orchestrator source get <source_event_id>                  # one normalized event
orchestrator source ingest --project demo --file event.json  # ingest a normalized fixture
orchestrator source replay <source_event_id>               # requeue one failed generic route
orchestrator source route <source_event_id>                # protected route + Slack deep link
```

| Subcommand | Description |
|------------|-------------|
| `list` | List recent provider-neutral source events with `--project`, `--task`, `--state`, `--limit` filters, without exposing raw provider payloads |
| `get` | Inspect one normalized event's routing state, provenance, and resolved process |
| `ingest` | Durably insert an authenticated normalized event fixture for adapter development and non-Slack integration testing (requires runtime source ingestion; `--payload-hash` optionally pins the payload) |
| `replay` | Admin-only recovery for generic source events; events linked to a badge automation route must use `source automation replay` instead |
| `route` | Inspect the protected automation route resolved for a source event, including its Slack deep link |

### Source bindings

Correlate trusted provider conversation coordinates with orchestrator tasks, and control governed source-to-task bindings.

```bash
orchestrator source bindings <task_id>                     # bindings correlated with one task
orchestrator source bind --project demo --task <task_id> --provider fixture \
  --installation install-1 --conversation C1 --thread T1 --source-event <event_id>
orchestrator source binding simulate --project demo --installation T1 \
  --reaction agent-analyze --channel C1 --actor U1
orchestrator source binding suspend badge-default --project demo
orchestrator source binding resume badge-default --project demo
```

| Subcommand | Description |
|------------|-------------|
| `bindings` | List primary, related, and notification_target bindings for one task |
| `bind` | Create a trusted binding (`--binding-type primary|related|notification_target`, provenance via `--source-event`) |
| `binding simulate` | Simulate deterministic matching against caller-supplied evidence — no side effects, no provider API calls |
| `binding suspend` | Stop a binding from matching new events, effective immediately |
| `binding resume` | Re-enable a suspended binding after conflict validation against the active bindings |

### Source automation

Inspect and control durable badge automation routes. Operational output omits Slack message coordinates, bodies, credentials, and permalinks. `replay` and `ignore` are audited operator controls requiring `--reason`, `--expected-version`, and `--idempotency-key`.

```bash
orchestrator source automation list --project demo --state needs_attention -o json
orchestrator source automation list --page-size 20 --page-token <token>
orchestrator source automation get <route_id> --attempt-limit 20
orchestrator source automation status --project demo -o json
orchestrator source automation watch --project demo --after 42
orchestrator source automation simulate --project demo --installation T1 \
  --reaction agent-analyze --channel C1 --actor U1 \
  --message-url https://acme.slack.com/archives/C1/p123 --target-id C1:1.23
orchestrator source automation replay <route_id> --expected-version 7 \
  --reason "credential rotated" --idempotency-key replay-1
orchestrator source automation ignore <route_id> --expected-version 8 \
  --reason "obsolete request" --idempotency-key ignore-1
```

| Subcommand | Description |
|------------|-------------|
| `list` | List safe route projections with bounded keyset pagination (`--page-size`, `--page-token`; filters: `--project`, `--state`, `--provider`, `--binding`, `--task`) |
| `get` | Show one route's safe projection and bounded attempt history (`--attempt-limit`) |
| `status` | Report backlog, oldest age, active leases, retrying routes, Attention count, and low-cardinality failure families |
| `watch` | Follow reconnectable route transitions from a durable change sequence (`--after`); streaming output is `-o json` (default, NDJSON) or `-o yaml` |
| `simulate` | Run the live matcher and renderer against caller-supplied safe evidence — never reads credentials, calls Slack, reserves a route, creates Attention, or creates a task |
| `replay` | Replay an actionable route from its durable checkpoint; keeps the pinned generation unless `--adopt-current-config` is explicitly requested |
| `ignore` | Deliberately close a route without task creation and resolve its matching Attention item |

### Source templates

```bash
orchestrator source template preview badge-default --provider slack \
  --installation T1 --message-url https://acme.slack.com/archives/C1/p123
```

`source template preview` renders a side-effect-free sample using the daemon's active configuration — it never calls the provider or creates a task. Optional evidence overrides: `--event-id`, `--reaction`, `--target-id`.

### Source connections

Manage provider connections and OAuth installation intents. Mutations are audited (`--reason`, `--idempotency-key`) and version-checked (`--expected-version`) where they touch an existing connection; OAuth-opening commands accept `--no-open` to print the URL instead of launching a browser.

```bash
orchestrator source connection catalog                     # managed/manual provisioning capabilities
orchestrator source connection list -p demo
orchestrator source connection list -p demo --include-disconnected -o json
orchestrator source connection get <connection_id> -p demo
orchestrator source connection watch -p demo --after 42    # stream connection changes (NDJSON)

# Official Slack App OAuth
orchestrator source connection connect -p demo --reason "onboard workspace" --idempotency-key connect-1
orchestrator source connection status <intent_id> -p demo  # poll or resume an OAuth intent
orchestrator source connection cancel <intent_id> -p demo --reason "abandoned flow" --idempotency-key cancel-1
orchestrator source connection reauthorize <connection_id> -p demo --expected-version 2 \
  --reason "scope update" --idempotency-key reauth-1

# Dedicated (workspace-owned) Slack App
orchestrator source connection provision-dedicated -p demo --config-token-stdin \
  --reason "private app" --idempotency-key prov-1
orchestrator source connection dedicated-status <provisioning_id> -p demo
orchestrator source connection dedicated-resume <provisioning_id> -p demo \
  --reason "approve preview" --idempotency-key resume-1
orchestrator source connection dedicated-abandon <provisioning_id> -p demo \
  --reason "wrong workspace" --idempotency-key abandon-1
orchestrator source connection dedicated-upgrade <connection_id> -p demo --expected-version 3 \
  --config-token-stdin --approve --reason "apply manifest fix" --idempotency-key upgrade-1
orchestrator source connection migrate-to-shared <connection_id> -p demo --expected-version 3 \
  --reason "move to official app" --idempotency-key migrate-1
orchestrator source connection dedicated-delete <connection_id> -p demo --expected-version 5 \
  --app-id-confirmation A0123 --reason "decommission" --idempotency-key delete-1

# Connection lifecycle
orchestrator source connection disconnect <connection_id> -p demo --expected-version 2 \
  --reason "offboard workspace" --idempotency-key disc-1
orchestrator source connection transfer <connection_id> -p demo --expected-version 2 \
  --target-daemon-id <daemon_id> --reason "move to prod daemon" --idempotency-key transfer-1
```

| Subcommand | Description |
|------------|-------------|
| `catalog` | Report which managed and manual provisioning modes the daemon supports per provider |
| `list` | List safe connection projections without exposing credentials; disconnected connections hidden unless `--include-disconnected` |
| `get` | Inspect one connection's safe projection, lifecycle state, and version |
| `watch` | Follow monotonic connection changes (`--after`); streaming output is `-o json` (default, NDJSON) or `-o yaml` |
| `connect` | Start the official Slack App OAuth flow (creates an installation intent and opens the authorization URL) |
| `status` | Poll or resume one pending OAuth intent |
| `cancel` | Cancel an unfinished OAuth intent |
| `reauthorize` | Start OAuth again for an existing connection (e.g. after a scope change or credential revocation) |
| `provision-dedicated` | Validate and provision a workspace-owned private Slack App from a configuration token read on stdin (`--config-token-stdin`); preview first, then `--approve` |
| `dedicated-status` | Inspect a dedicated App provisioning checkpoint |
| `dedicated-resume` | Resume credential handoff or approve a reviewed dedicated App preview |
| `dedicated-abandon` | Abandon a non-terminal provisioning checkpoint |
| `dedicated-upgrade` | Review and apply the fixed manifest to an existing dedicated App (preview first, re-run with `--approve`) |
| `migrate-to-shared` | Start a reviewed dedicated-to-official App migration |
| `dedicated-delete` | Permanently delete a disconnected dedicated App; requires the App ID as `--app-id-confirmation` |
| `disconnect` | Disconnect a connection and destroy its managed credentials |
| `transfer` | Move exclusive connection ownership to another daemon (`--target-daemon-id`) |

## Debug & System

```bash
orchestrator debug                   # inspect internal state
orchestrator debug --component config  # show active config
orchestrator version                 # build version + git hash
orchestrator version -o json         # JSON version output
orchestrator check                   # preflight validation
orchestrator check -o json           # structured check output
orchestrator guide                   # guided CLI reference with examples
orchestrator guide task              # filter by command name
orchestrator guide -c task -f json   # filter by category, JSON output
```

### debug sandbox-probe

Run a local sandbox probe without contacting the daemon — exercises resource and
network limits for sandbox validation.

```bash
orchestrator debug sandbox-probe write-file --path /tmp/probe.txt
orchestrator debug sandbox-probe open-files --count 256
orchestrator debug sandbox-probe cpu-burn
orchestrator debug sandbox-probe alloc-memory --total-mb 256 --chunk-mb 8
orchestrator debug sandbox-probe spawn-children --count 64 --sleep-secs 60
orchestrator debug sandbox-probe dns-resolve --host example.com --port 443
orchestrator debug sandbox-probe tcp-connect --host 127.0.0.1 --port 8080 --timeout-secs 3
```

## QA Observability

```bash
orchestrator qa doctor               # observability health metrics from task_execution_metrics
orchestrator qa doctor -o json       # structured output
```

## Process Metrics

Process Console operational metrics.

```bash
orchestrator metrics process -p demo                       # snapshot over the default 24h window
orchestrator metrics process -p demo --window 7d --bucket 1d -o json
orchestrator metrics prune --retention-days 30             # delete optional metrics past retention
orchestrator metrics rebuild -p demo                       # rebuild materialized rollups
```

| Subcommand | Description |
|------------|-------------|
| `process` | Query one project-scoped Process Console snapshot over a time `--window` (default: 24h) with a configurable `--bucket` size (default: 1h) |
| `prune` | Delete optional metrics older than the `--retention-days` threshold |
| `rebuild` | Rebuild retained materialized rollups for one project |

## Built-in Tools

Helper utilities for CRD plugin scripts (invoked from trigger/finalize plugins): `tool webhook-verify-hmac`, `tool payload-extract`, and `tool secret-rotate`.

```bash
# Verify an HMAC signature (exit 0 = valid, exit 1 = invalid)
orchestrator tool webhook-verify-hmac --secret <secret> --body <body> --signature <sig> [--algo sha256]

# Extract a value from JSON using a dot-separated path (reads stdin)
echo '{"event":{"type":"push"}}' | orchestrator tool payload-extract --path event.type
orchestrator tool payload-extract --path event.type < payload.json

# Rotate a key in a SecretStore (requires a running daemon)
orchestrator tool secret-rotate <store> <key> --value <new_value> [--project <id>]
```

## Output Formats

Every non-streaming command takes a unified `-o, --output {table,json,yaml}` flag:

- Collection commands (`list`-style) default to `table`.
- Single-object reads and mutations default to `yaml`.

Streaming commands (`attention follow`, `source automation watch`, `source connection watch`) take `-o {json,yaml}` and default to `json`, emitted as NDJSON (one JSON object per line).

Two deliberate exceptions keep their own switches: `agent session read --chunks-json` (structured chunk framing with reconnect offsets) and `guide --format {markdown,json}`.

`--json` survives only as a hidden deprecated alias for `-o json` on `version` and `task trace`, for one release cycle — use `-o json` instead.

## Daemon (C/S Mode)

### orchestratord

The daemon binary that runs the gRPC server and embedded background workers.

```bash
# Start in foreground (recommended for development)
./target/release/orchestratord --foreground

# With multiple workers
./target/release/orchestratord --foreground --workers 3

# TCP bind (for remote access)
./target/release/orchestratord --foreground --bind 0.0.0.0:50051
```

| Flag | Description |
|------|-------------|
| `--foreground`, `-f` | Run in foreground (don't daemonize) |
| `--bind <addr>` | TCP bind address (default: Unix socket) |
| `--workers <N>` | Number of background workers (default: 1) |
| `--insecure-bind <addr>` | Insecure TCP bind for development (feature-gated: `dev-insecure`) |
| `--control-plane-dir <DIR>` | Control plane certificate directory |
| `--uds-max-role <ROLE>` | Maximum role for UDS callers when no `uds-policy.yaml` exists: `read-only`, `operator`, `admin` (default: operator, env: `ORCHESTRATOR_UDS_MAX_ROLE`) |
| `--event-retention-days <DAYS>` | Days to retain events (default: 30, 0 = disabled) |
| `--event-cleanup-interval-secs <SECS>` | Cleanup sweep interval in seconds (default: 3600) |
| `--event-archive-enabled` | Archive events to JSONL before cleanup |
| `--event-archive-dir <DIR>` | Override event archive directory |
| `--log-retention-days <DAYS>` | Days to retain log files before automatic cleanup (default: 30, 0 = disabled) |
| `--task-retention-days <DAYS>` | Days to retain terminated tasks before automatic cleanup (default: 0 = disabled) |
| `--stall-timeout-mins <MINS>` | Minutes before a running item is considered stalled (default: 30, 0 = disabled) |
| `--webhook-bind <ADDR>` | Bind address for HTTP webhook server (default: `127.0.0.1:19090`, `none` to disable). Non-loopback addresses require a secret. |
| `--webhook-secret <SECRET>` | Shared secret for webhook HMAC-SHA256 verification (env: `ORCHESTRATOR_WEBHOOK_SECRET`) |
| `--webhook-allow-unsigned` | Allow webhook on non-loopback without signature verification (env: `ORCHESTRATOR_WEBHOOK_ALLOW_UNSIGNED`) |

### control-plane issue-client

Issue client TLS materials for connecting to the daemon's control plane:

```bash
orchestratord control-plane issue-client \
  --bind <addr> --subject <name> [--role <role>]
```

`--role` accepts `read-only`, `operator` (default), or `admin`. Optional
`--home` and `--control-plane-dir` override the certificate locations.

Files created:
- PID: `~/.orchestratord/daemon.pid`
- Socket: `~/.orchestratord/orchestrator.sock`

### webhook-secret

Print the webhook HMAC secret derived from the control-plane CA certificate.

```bash
orchestratord webhook-secret
orchestratord webhook-secret --control-plane-dir <dir>
```

### daemon management

```bash
./target/release/orchestratord --foreground --workers 2   # foreground (recommended)
nohup ./target/release/orchestratord --foreground &       # background via nohup
orchestrator daemon stop                                  # graceful SIGTERM
```

### C/S CLI command surface

All commands connect to the daemon via Unix socket:

```bash
# Resource management (--project for project scope)
orchestrator apply -f manifest.yaml [--project <id>] [--dry-run]
orchestrator get <resource> [-o json|yaml] [--project <id>]
orchestrator describe <kind/name> [--project <id>]
orchestrator delete <kind/name> --force [--project <id>]

# Task lifecycle
orchestrator task create --name X --goal Y [--project <id>] [--workflow Z] [--step S] [--set k=v]
orchestrator run --workflow Z [--step S] [--set k=v]          # synchronous execution
orchestrator run --template T --agent-capability C [--set k=v] # direct assembly mode
orchestrator task list [-o json] [--project <id>] [--status <s>]
orchestrator task items <id> [--status <s>] [-o json]
orchestrator task info <id> [-o json]
orchestrator task start <id>
orchestrator task pause <id>
orchestrator task resume <id>
orchestrator task logs <id> [--tail N] [--follow]
orchestrator task watch <id>
orchestrator task trace <id> [--verbose]
orchestrator task retry <item_id> [--force]
orchestrator task recover <id>
orchestrator task timeline <id> [--category <c>] [--follow] [-o json]
orchestrator task delete <id> --force

# Attention queue
orchestrator attention list [--state <s>] [--assignee me] [-o json]
orchestrator attention get <id>
orchestrator attention claim|snooze|resolve|action <id> --expected-version <v>
orchestrator attention follow [--after <seq>]

# Handoff & resume
orchestrator handoff generate <task_id> [--cursor <n>]
orchestrator handoff get <handoff_id>
orchestrator resume boundaries <task_id>
orchestrator resume plan <task_id> --boundary <b> --mode <m>
orchestrator resume execute <plan_id> --expected-state-version <v> --reason <r> --idempotency-key <k>

# Agent lifecycle
orchestrator agent list [--project <id>] [-o json|yaml]
orchestrator agent cordon <agent_name> [--project <id>]
orchestrator agent uncordon <agent_name> [--project <id>]
orchestrator agent drain <agent_name> [--project <id>] [--timeout <secs>]

# Agent sessions
orchestrator agent session list|get|attach|read|heartbeat|send-input|detach|close|resolve

# Trigger lifecycle
orchestrator trigger suspend|resume|fire <name> [--project <id>] [--payload <json>]

# Source integration
orchestrator source list|get|ingest|replay|route|bind|bindings
orchestrator source binding simulate|suspend|resume
orchestrator source automation list|get|status|watch|simulate|replay|ignore
orchestrator source template preview <name> --provider <p> --installation <i> --message-url <url>
orchestrator source connection list|get|watch|catalog|connect|status|cancel|reauthorize
orchestrator source connection provision-dedicated|dedicated-status|dedicated-resume|dedicated-abandon
orchestrator source connection dedicated-upgrade|migrate-to-shared|dedicated-delete|disconnect|transfer

# Project cleanup
orchestrator delete project/<id> --force

# Store (--project for project scope)
orchestrator store put <store> <key> <value> [--project <id>]
orchestrator store get <store> <key> [--project <id>]
orchestrator store list <store> [-o json] [--project <id>]
orchestrator store delete <store> <key> [--project <id>]
orchestrator store prune <store> [--project <id>]

# Manifest
orchestrator manifest validate -f <file>
orchestrator manifest export [-o yaml|json]

# Secret key management
orchestrator secret key status|list|rotate|revoke|bootstrap|history

# Database
orchestrator db status [-o json]
orchestrator db migrations list [-o json]
orchestrator db vacuum
orchestrator db cleanup [--older-than <days>]

# Events
orchestrator event stats
orchestrator event list --task <id> [-o json]
orchestrator event cleanup [--older-than <days>] [--dry-run] [--archive]

# Audit evidence
orchestrator audit list --project <id> [--actor <a>] [--status <s>]
orchestrator audit get <request_id> --project <id>

# Process metrics
orchestrator metrics process --project <id> [--window <w>] [--bucket <b>]
orchestrator metrics prune [--retention-days <n>]
orchestrator metrics rebuild --project <id>

# Daemon lifecycle
orchestrator daemon status|stop
orchestrator daemon maintenance --enable|--disable

# QA & tools
orchestrator qa doctor [-o json]
orchestrator tool webhook-verify-hmac|payload-extract|secret-rotate

# System
orchestrator version
orchestrator debug [--component config]
orchestrator check [-o json] [--workflow <w>]
orchestrator init [<root>]
```

## Resource Metadata

All resources support `metadata.labels` (key-value pairs for categorization and label-selector queries) and `metadata.annotations` (arbitrary key-value metadata). Both are optional.

```yaml
metadata:
  name: my-resource
  labels:
    env: dev
    team: platform
  annotations:
    note: "created for sprint 12"
```

## Structured Agent Output

Agents must produce JSON on stdout conforming to this schema:

```json
{
  "confidence": 0.95,
  "quality_score": 0.9,
  "artifacts": [
    {
      "kind": "analysis",
      "findings": [
        {
          "title": "finding-name",
          "description": "details",
          "severity": "info"
        }
      ]
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `confidence` | `float` | Agent's confidence in the result (0.0–1.0) |
| `quality_score` | `float` | Quality assessment (0.0–1.0) |
| `artifacts` | `array` | Structured output artifacts |
| `artifacts[].kind` | `string` | `analysis`, `code_change`, etc. |
| `artifacts[].findings` | `array` | List of findings with title/description/severity |
| `artifacts[].files` | `array` | List of modified files (for code_change) |

This output is parsed into `AgentOutput` and used for prehook variable injection (`qa_confidence`, `qa_quality_score`) and finalize rule evaluation.
