# Run General Agent Tasks Without A Code Repository

Use a `task` Workspace when the work is a process—such as Slack triage, inventory lookup, document analysis, or drafting a reply—rather than a change to a code repository.

## Mental Model

A code-repository Workspace discovers QA files and may use tickets or Git checkpoints. A task Workspace has one piece of work: the task goal. It therefore creates one implicit item and finishes when the agent calls `mark_done` or its configured driver reports a successful terminal result.

| Capability | `code_repo` | `task` |
|---|---|---|
| `work_dir` | Required | Optional |
| `qa_targets`, `ticket_dir` | Required | Forbidden |
| Explicit target files | Supported | Forbidden |
| Git checkpoints / self-reference | Supported when configured | Forbidden |
| Execution | Host or sandbox by policy | Scoped sandbox required |
| Item completion | Workflow finalize / QA evidence | `mark_done` or driver terminal event |

## 1. Choose What The Daemon May Share

The daemon administrator creates `{data_dir}/file-sharing.yaml` before startup. With the default data directory, this is `~/.orchestratord/file-sharing.yaml`.

```yaml
fileSharing:
  globalSkills:
    - path: ~/.orchestrator/skills
  shareableRoots:
    - ~/.orchestrator/skills
    - ~/warehouse-data
```

`shareableRoots` is the maximum authority. A Workspace or ExecutionProfile may narrow it but cannot expand it. If this file is absent, host path sharing is denied. Restart `orchestratord` after changing it.

On Unix, each `globalSkills` directory must be owned by the daemon user and must not be group/world-writable. Keep it separate from every task `work_dir`, managed task home, and ExecutionProfile `writable_paths`; either direction of path overlap is rejected. Non-Unix platforms reject configured global Skills because the required UID/mode provenance cannot be verified.

Do not add your whole home directory. Prefer one root for Skills and one separate root for each operational data area.

## 2. Define A Task Workspace

Use a persistent directory when tasks intentionally share data:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: warehouse-ops
spec:
  kind: task
  work_dir: ~/warehouse-data
```

Omit `work_dir` for isolated scratch work:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: private-scratch
spec:
  kind: task
```

The daemon then creates a unique private HOME for each task and removes it at terminal completion. User-owned persistent directories are never deleted.

`root_path` remains accepted for older manifests, but new manifests should use `work_dir`.

## 3. Require A Scoped Sandbox

Every enabled step in a task Workspace must reference a sandbox profile:

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: task-sandbox
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  network_mode: deny
```

Add `readable_paths` or `writable_paths` only when necessary. Each path must remain below `shareableRoots`; environment-variable expansion in these paths is rejected for task Workspaces.

Inside the agent process:

- `HOME` is the resolved `work_dir` or managed task HOME.
- XDG config/cache/data/state/runtime and `TMPDIR` remain below it.
- Global Skill roots are read-only and listed in `ORCHESTRATOR_GLOBAL_SKILLS` as a colon-separated path list.
- `ORCHESTRATOR_READABLE_PATHS` contains the complete readable allowlist for compatible agent wrappers.

## 4. Define Agent, Prompt, And Workflow

This deterministic example drafts a reply and uses a driver terminal event to finish:

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: reply-prompt
spec:
  prompt: "{goal}"
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: reply-agent
spec:
  capabilities: [prepare_reply]
  command: your-agent-command
  driver:
    provider: shell
    transport: cli
    shell:
      requirePromptPlaceholder: false
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: reply-flow
spec:
  steps:
    - id: prepare
      required_capability: prepare_reply
      template: reply-prompt
      execution_profile: task-sandbox
      scope: task
  loop:
    mode: once
```

Claude and Codex drivers may instead call the orchestrator-owned `mark_done` MCP tool. Normal safety limits—cycle cap, timeout, resource limits, and cancellation—still apply.

## 5. Create And Inspect A Task

```bash
orchestrator apply --project operations -f operations.yaml
orchestrator task create \
  --project operations \
  --workspace warehouse-ops \
  --workflow reply-flow \
  --goal "Review the tagged Slack message and draft an inventory-backed reply"
```

Do not pass `--target-file` for a task Workspace.

In Process Console, open “Tasks” and select the task. The overview displays “Workspace type: Task”; the Expert workflow uses the generic “Task” label. Timeline, evidence, Attention, handoff, and Session controls work the same way as for code processes.

## Slack Badge Example

The complete mock bundle demonstrates badge routing into a task Workspace:

```bash
cargo build -p orchestratord -p orchestrator-cli
scripts/qa/test-non-code-workspace.sh
```

It runs an isolated daemon and loopback Slack service, then verifies signed reaction ingestion, permalink routing, global Skill access, inventory lookup, reply evidence, Attention projection, convergence, and HOME cleanup. It uses no live AI provider and consumes no API credits.

## Common Errors

| Error | Meaning | Fix |
|---|---|---|
| `TASK_WORKSPACE_SANDBOX_REQUIRED` | A step has no scoped sandbox | Add `execution_profile` with `mode: sandbox` and scoped `fs_mode` |
| `FILE_SHARING_PATH_OUTSIDE_CEILING` | A host path exceeds daemon authority | Add a narrow root to `file-sharing.yaml`, then restart the daemon |
| `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED` | A global Skill has the wrong owner, unsafe write bits, unsupported provenance, or overlaps a task write path | Change ownership to the daemon user, run `chmod go-w`, separate Skill/data roots, then restart the daemon |
| `TASK_WORKSPACE_QA_FIELDS_FORBIDDEN` | QA/ticket fields were copied from a code Workspace | Remove `qa_targets` and `ticket_dir` |
| `TASK_WORKSPACE_GIT_CHECKPOINT_FORBIDDEN` | Workflow uses a Git checkpoint | Set checkpoint strategy to `none` |
| `TASK_WORKSPACE_TARGET_FILES_FORBIDDEN` | Task creation supplied target files | Put the work in `goal` or initial variables instead |

## Security Checklist

- Keep `shareableRoots` minimal.
- Keep global Skills daemon-owned, remove group/world writes, and never overlap them with task-writable paths.
- Treat persistent `work_dir` as shared state; use omitted `work_dir` for isolation.
- Use `network_mode: deny` unless the agent truly requires outbound access.
- Pass credentials through SecretStore, never through shared files or task goals.
- Review [file-sharing ceiling tests](../security/authorization/02-file-sharing-ceiling.md) and [HOME isolation tests](../security/file-security/02-workspace-home-isolation.md) before production rollout.
