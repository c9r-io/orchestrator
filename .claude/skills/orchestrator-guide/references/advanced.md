# Advanced Features

## Table of Contents
- [Custom Resource Definitions (CRD)](#custom-resource-definitions)
- [Persistent Store (WP01)](#persistent-store-wp01)
- [Task Spawning (WP02)](#task-spawning-wp02)
- [Dynamic Items + Selection (WP03)](#dynamic-items--selection-wp03)
- [Invariant Constraints (WP04)](#invariant-constraints-wp04)
- [Self-Bootstrap](#self-bootstrap)
- [Safety Configuration](#safety-configuration)

## Custom Resource Definitions

### Define a CRD

```yaml
apiVersion: orchestrator.dev/v2
kind: CustomResourceDefinition
metadata:
  name: promptlibraries.extensions.orchestrator.dev
spec:
  kind: PromptLibrary
  plural: promptlibraries
  short_names: [pl]
  group: extensions.orchestrator.dev
  versions:
    - name: v1
      served: true
      schema:
        type: object
        required: [prompts]
        properties:
          prompts:
            type: array
            minItems: 1
            items:
              type: object
              required: [name, template]
              properties:
                name: { type: string }
                template: { type: string }
      cel_rules:
        - rule: "size(self.prompts) > 0"
          message: "at least one prompt required"
```

### Create instances

```yaml
apiVersion: extensions.orchestrator.dev/v1
kind: PromptLibrary
metadata:
  name: qa-prompts
spec:
  prompts:
    - name: code-review
      template: "Review code for {criteria}..."
```

Manage: `get pl`, `describe promptlibrary X`, `delete promptlibrary X`.

## Persistent Store (WP01)

```yaml
apiVersion: orchestrator.dev/v2
kind: WorkflowStore
metadata:
  name: context
spec:
  backend: local              # local (SQLite) | command
  retention:
    max_entries: 1000
    ttl_seconds: 86400
```

Step integration:

```yaml
# Write after step
behavior:
  post_actions:
    - type: store_put
      store: context
      key: result
      from_var: plan_output

# Read before step
store_inputs:
  - store: context
    key: result
    as_var: inherited_data
```

## Task Spawning (WP02)

```yaml
behavior:
  post_actions:
    - type: spawn_task
      goal: "verify-changes"
      workflow: verify_wf
    - type: spawn_tasks
      from_var: task_list       # JSON array
      workflow: child_wf
```

Safety:

```yaml
safety:
  max_spawned_tasks: 10
  max_spawn_depth: 3
  spawn_cooldown_seconds: 5
```

## Dynamic Items + Selection (WP03)

Generate items:

```yaml
behavior:
  post_actions:
    - type: generate_items
      from_var: candidates
```

Select items:

```yaml
- id: select_best
  builtin: item_select
  enabled: true
  item_select_config:
    strategy: weighted          # min | max | threshold | weighted
    metric_key: quality_score
    top_k: 3
    weights:
      confidence: 0.4
      quality_score: 0.6
```

## Invariant Constraints (WP04)

```yaml
safety:
  invariants:
    - id: main_branch_exists
      check:
        command: "git branch --list main | wc -l"
        expect: "1"
      on_violation: abort       # abort | warn | rollback
      protected_files:
        - ".github/workflows/*"
      checkpoint_filter:
        steps: [implement, self_test]
```

## Self-Bootstrap

### 2-Cycle Strategy

```
Cycle 1 (production):  plan → qa_doc_gen → implement → self_test → self_restart
Cycle 2 (validation):  implement → self_test → qa_testing → ticket_fix → align_tests → doc_governance → loop_guard
```

- `repeatable: false` on plan/qa_doc_gen/self_restart → only Cycle 1
- QA steps gated by `prehook.when: "is_last_cycle"` → only Cycle 2
- `self_restart` uses exec-based hot reload inside the daemon (preserves PID); CLI foreground mode has exit-code-75 fallback loop
- Implement step uses `execution_profile: sandbox_write` for filesystem isolation with API access

### Self-Referential Workspace

```yaml
kind: Workspace
spec:
  self_referential: true    # enforces auto_rollback + checkpoint + binary_snapshot
```

### 4-Layer Survival
1. **Binary Snapshot**: `.stable` backup at cycle start
2. **Self-Test Gate**: cargo check + cargo test --lib + manifest validate
3. **Self-Referential Enforcement**: `self_referential_safe` prehook variable filters unsafe QA docs
4. **Watchdog**: restores `.stable` on consecutive crashes

### Self-Restart Flow (exec-based)

1. `execute_self_restart_step()` builds new binary, verifies via `--help`, snapshots as `.stable`
2. Returns `RestartRequestedError` up the call stack
3. Daemon worker catches the error, sends binary path via watch channel
4. Daemon drains workers (30s timeout), then calls `exec()` to replace process in-place (same PID)
5. Fallback: if exec fails, CLI foreground loop catches exit code 75 and relaunches

## Safety Configuration

```yaml
safety:
  max_consecutive_failures: 3
  auto_rollback: true
  checkpoint_strategy: git_tag   # none | git_tag | git_stash
  binary_snapshot: true
  step_timeout_secs: 1800
  max_spawned_tasks: 10
  max_spawn_depth: 3
  spawn_cooldown_seconds: 5
  invariants: [...]
```

## Deployment Manifest Order

For production self-bootstrap workflows, apply manifests in this order:

```bash
# 1. Execution profiles (must exist before workflows reference them)
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap

# 2. Secrets (API keys for AI agents)
orchestrator apply -f docs/workflow/claude-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/minimax-secret.yaml --project self-bootstrap

# 3. Workflow (references profiles and agents)
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap
```

Workflow validation rejects references to nonexistent execution profiles, so profiles must be applied first.
