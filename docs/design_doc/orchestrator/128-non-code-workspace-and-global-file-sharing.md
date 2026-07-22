# Orchestrator - Non-Code Workspace And Global File Sharing

**Module**: orchestrator  
**Status**: Approved  
**Related Plan**: FR-117 and FR-117-A, task-oriented Workspace semantics and daemon-owned file-sharing ceiling
**Related QA**: `docs/qa/orchestrator/165-non-code-workspace-and-global-file-sharing.md`, `docs/qa/orchestrator/167-global-skill-directory-provenance.md`
**Created**: 2026-07-22  
**Last Updated**: 2026-07-23

## Background

The original Workspace contract assumed a code repository: a root directory, QA-document scan targets, ticket output, and file-backed task items. Slack operations, document analysis, inventory assistance, and other general agent processes need a governed execution context without pretending that a QA file or Git repository exists.

### Threat Model

Removing the repository boundary changes who the adversary is. In a `code_repo` workspace the Git tree and workspace root implicitly bound the agent; in a `task` workspace there is no project boundary, so **the sandbox plus the daemon-owned file-sharing ceiling are the only remaining constraints**. The design therefore treats the agent process itself as the primary untrusted principal:

- **Untrusted agent process** — the LLM may be prompt-injected. In the warehouse pilot the triggering Slack message is attacker-influenced content; a compromised turn may attempt to read `~/.ssh`, exfiltrate the daemon user's ambient HOME, or write outside the workspace. SI-1 (ceiling subset) and SI-2 (HOME isolation) exist specifically to contain this principal.
- **Untrusted manifest author** — a project manifest must not be able to expand host authority beyond what the operator approved. The two-level subset model makes the operator (via `file-sharing.yaml`), not the manifest, the authority owner.
- **Untrusted external actor** — the Slack sender/reactor. Bounded by existing FR-113/114 routing (permalink-only, role-gated, message text cannot select Skill/workflow); this FR inherits, does not re-open, that boundary.

Out of model: a compromised daemon or operator, and kernel/sandbox-backend escapes (delegated to the OS sandbox).

## Goals

- Preserve existing `code_repo` behavior while adding `kind: task`.
- Give every non-code task exactly one implicit item and a valid cwd/HOME.
- Permit operator-approved host paths only beneath a daemon-owned ceiling.
- Mount global Skill directories read-only and prevent ambient host HOME reads.
- Converge task items from `mark_done` or a successful driver terminal event.
- Present task semantics in Process Console without exposing the `__TASK__` storage sentinel.

## Non-goals

- Removing Git, QA, or ticket behavior from `code_repo` workspaces.
- Granting arbitrary host access when `file-sharing.yaml` is absent.
- Automatically translating every provider's private Skill discovery layout.
- Changing Slack ingestion, badge matching, or OAuth ownership.

## Scope

- In scope: Workspace schema, resource validation, task creation, sandbox profiles, HOME/XDG isolation, lifecycle cleanup, convergence, Console presentation, fixture and QA automation.
- Out of scope: container runtimes, multi-user home management, message-body ingestion, and non-sandbox execution for task workspaces.

## User Experience

A user declares a general task workspace without QA fields:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: warehouse-ops
spec:
  kind: task
  work_dir: ~/warehouse-data # optional
```

If `work_dir` is omitted, the daemon creates `{data_dir}/task-homes/{task_id}` with mode `0700`, uses it for cwd and HOME, and removes it after terminal completion. The Process Console shows “Workspace type: Task”; the implicit item is labeled “Task”.

## Interfaces And Data

### Workspace

- `spec.kind`: `code_repo` (default) or `task`.
- `spec.work_dir`: canonical field; legacy `root_path` remains a deserialize alias.
- `qa_targets` and `ticket_dir`: still required for `code_repo`, forbidden for `task`.
- No database migration is required. Existing task columns retain the resolved runtime paths and the implicit item uses the internal `__TASK__` sentinel.

### Daemon File Sharing

`{data_dir}/file-sharing.yaml` is operator-owned and loaded at daemon startup:

```yaml
fileSharing:
  globalSkills:
    - path: ~/.orchestrator/skills
  shareableRoots:
    - ~/.orchestrator/skills
    - ~/warehouse-data
```

A missing file means an empty ceiling. Paths are expanded, canonicalized, checked for lexical traversal and symlink escape, and must be contained by one configured root.

### Stable Rejection Codes

- `CODE_REPO_WORK_DIR_REQUIRED`
- `TASK_WORKSPACE_SELF_REFERENTIAL_FORBIDDEN`
- `TASK_WORKSPACE_QA_FIELDS_FORBIDDEN`
- `TASK_WORKSPACE_GIT_CHECKPOINT_FORBIDDEN`
- `TASK_WORKSPACE_DYNAMIC_ITEMS_FORBIDDEN`
- `TASK_WORKSPACE_SANDBOX_REQUIRED`
- `TASK_WORKSPACE_TARGET_FILES_FORBIDDEN`
- `FILE_SHARING_PATH_OUTSIDE_CEILING`
- `FILE_SHARING_DYNAMIC_PATH_FORBIDDEN`

Static Workspace validation happens during resource apply. Cross-resource compatibility is enforced at task-create preflight because a Workspace and Workflow may be selected dynamically by CLI or source routing.

## Key Design

1. `WorkspaceKind` is additive and defaults to `code_repo`, avoiding a second resource graph.
2. A task workspace always materializes one implicit item; QA discovery and finalize rules are bypassed.
3. Every enabled step must select a scoped sandbox profile. Git checkpoints, item generation, item isolation, explicit targets, and dynamic path expansion fail closed.
4. Runtime `HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_RUNTIME_DIR`, and `TMPDIR` are forced beneath the resolved workspace root.
5. macOS Seatbelt uses an explicit read set and explicit write denial for global Skill paths. Linux masks the ambient host home with tmpfs and rebinds only approved subpaths.
6. Successful `driver_finished` and `mark_done` signals converge the implicit item even when steps execute at task scope and user-facing artifact collection is disabled.
7. Managed task homes are guarded during creation, recreated safely on resume, removed at terminal task exit, and removed together with managed artifacts when the task is deleted.

## Alternatives And Tradeoffs

- A separate `NonCodeWorkspace` kind would make semantics obvious but duplicate project/resource plumbing and UI administration. A discriminator keeps resource selection uniform.
- Treating every task as item-less would require broad scheduler and persistence changes. One implicit item preserves established lifecycle and event joins.
- Allowing per-profile paths without a daemon ceiling is flexible but lets project manifests expand host authority. The two-level subset model makes the operator the authority owner.

## Risks And Mitigations

- Symlink or prefix escape: canonicalize before subset comparison and test sibling-prefix and symlink cases.
- Host HOME leakage: strict read mode plus forced environment variables and a real sandbox pilot.
- Temporary data leakage: `0700`, per-task identity, RAII cleanup on failed creation, terminal cleanup, and delete cleanup.
- **Global Skill supply-chain surface**: every `globalSkills` directory is mounted read-only into *every* task sandbox, so its contents execute with the authority of each agent. Configuration load now requires the directory owner to match the daemon effective UID, rejects group/world write bits, and rejects ancestor-or-descendant overlap with an explicit task `work_dir`, the managed `task-homes` root, or any project ExecutionProfile `writable_paths`. The same check runs at daemon bootstrap, resource apply, and phase setup; failures use `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED` with an actionable fix. Non-Unix platforms reject configured global Skills because Unix provenance cannot be verified. Read-only sandbox mounting remains a second independent layer.
- **Convergence trust shift**: a task item converges on the agent self-reporting via `mark_done` or a successful driver terminal event. When the agent is the untrusted principal, self-certified completion is a weaker gate than a QA exit code. This is an accepted trade-off for non-code work (there is no external verifier to run); it is bounded by `max_cycles`, degenerate-loop detection, and budget caps, and the operator remains the decision authority via the Attention review item rather than trusting the suggestion directly.
- Hidden non-code semantics in the UI: explicit `workspace_kind`/`item_kind` presentation fields without changing the public gRPC schema.
- Low-confidence suggestions disappearing: successful low-confidence steps now both resolve stale step attention and open a review item before task-level resolution.

## Observability

- Canonical events remain `step_started`, `step_finished`, `driver_finished`, `task_completed`/`task_failed`, and Attention change records.
- `execution_profile_applied` records profile name, execution mode, and backend.
- Task rows persist resolved `workspace_root` and `artifacts_dir`; command runs persist cwd and sandbox evidence.
- File-sharing errors use stable codes and canonical paths but do not emit host file contents.

## Operations / Release

- Restart the daemon after changing `file-sharing.yaml`.
- Before restart, make every global Skill directory daemon-owned, remove group/world write bits, and keep it disjoint from all task write boundaries.
- Roll out task workspaces only after every enabled step references a supported scoped sandbox profile.
- Keep `shareableRoots` narrow; do not add the whole user home.
- Rollback is code-only and additive. Existing manifests continue to deserialize through `root_path`; task manifests must be disabled before running an older binary that does not understand them.

## Test Plan

- Unit: schema aliasing, task validation, ceiling/symlink checks, global Skill UID/mode/write-boundary provenance, implicit item materialization, gate failures, private HOME, sandbox profile generation, Attention policy.
- Integration: isolated daemon, signed Slack delivery, fake permalink provider, source route, sandbox agent, global Skill and inventory evidence, convergence, Attention, cleanup.
- UI: Vitest and Playwright verify task presentation and absence of the internal sentinel.

## QA Docs

- `docs/qa/orchestrator/165-non-code-workspace-and-global-file-sharing.md`
- `docs/qa/orchestrator/167-global-skill-directory-provenance.md`
- `docs/security/authorization/02-file-sharing-ceiling.md`
- `docs/security/file-security/02-workspace-home-isolation.md`

## Acceptance Criteria

- Existing code-repository manifests remain compatible and serialize canonically as `work_dir`.
- Task workspaces omit QA fields, use one implicit item, and require a scoped sandbox.
- File-sharing paths remain inside the daemon ceiling and global Skills are read-only.
- HOME/XDG values never expose the ambient host home.
- Driver/`mark_done` convergence, Slack pilot, Attention projection, cleanup, Console UI, full tests, and lint all pass.
