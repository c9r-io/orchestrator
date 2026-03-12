# Item-Scoped Git Worktree Isolation

## Summary

FR-029 closes the isolation gap in `self-evolution` by executing each item-scoped candidate in its own git worktree instead of sharing the primary workspace.

The implementation keeps the existing segment scheduler and item selection model, but changes the execution context for item-scoped phases:

- each dynamic item gets a dedicated temporary branch
- each branch is materialized as a separate git worktree
- item-scoped steps render `{source_tree}` to that worktree path
- `item_select` promotes winner metadata and the engine fast-forwards the winner branch back into the primary workspace
- temporary worktrees and branches are removed after workflow completion

## Design

### Config surface

`WorkflowConfig`, `TaskExecutionPlan`, and manifest-facing `WorkflowSpec` now accept:

```yaml
item_isolation:
  strategy: git_worktree
  branch_prefix: evo-item
  cleanup: after_workflow
```

This keeps the isolation policy workflow-scoped and avoids per-step duplication for the current use case.

### Runtime model

The scheduler keeps task-scoped execution on the primary workspace root. For item-scoped execution it now:

1. ensures the primary workspace git tree is clean before preparing isolated items
2. creates `logs/<task_id>/item-worktrees/<item_id>/` as the physical worktree path
3. creates a temporary branch named from `branch_prefix/task_id/item_id`
4. attaches pipeline vars:
   - `item_worktree_path`
   - `item_branch`
5. resolves step workspace root from `item_worktree_path` for item-scoped phases

This means agent prompts, builtin command steps, diff capture, and dynamic item execution all observe the isolated worktree.

### Winner application

After `item_select`, winner pipeline vars are promoted as before. The engine now additionally checks `item_branch` and applies:

```text
git merge --ff-only <winner-branch>
```

to the primary workspace before task-scoped finalize steps continue.

`--ff-only` keeps the apply path deterministic. If the merge cannot fast-forward, the workflow fails instead of silently synthesizing a merge commit.

### Cleanup

When the task loop exits, the engine removes:

- all temporary worktrees under `logs/<task_id>/item-worktrees`
- all temporary item branches for the task

Cleanup is skipped only when the policy is `cleanup: never`.

## Tradeoffs

- `git_worktree` is heavier than branch-only isolation, but it gives real filesystem isolation and prevents cross-item file leakage.
- cleanup is deferred to task completion, which keeps winner and loser states available during selection and task-scoped finalize steps.
- the implementation intentionally requires a clean primary git workspace before creating worktrees; this avoids ambiguous base state.

## Verification

- `cargo check -p agent-orchestrator`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
