---
self_referential_safe: true
---

# Item-Scoped Git Worktree Isolation

## Scope

Verify that item-scoped self-evolution candidates execute in separate git worktrees, that the winner is merged back into the primary workspace, and that temporary worktrees are cleaned up.

## Scenario 1: Config Round-Trip Preserves `item_isolation`

1. Run:
   ```bash
   cargo test -p orchestrator-config workflow_config_item_isolation_round_trips_through_serde -- --nocapture && cargo test -p agent-orchestrator workflow_item_isolation_round_trip_through_spec_conversion -- --nocapture
   ```
2. Confirm both tests pass.

Expected:

- workflow serde preserves `strategy`, `branch_prefix`, and `cleanup`
- workflow spec/config conversion preserves `item_isolation`

## Scenario 2: Workspace Builds With Vendored `protoc`

1. Run:
   ```bash
   cargo check -p agent-orchestrator
   ```

Expected:

- `orchestrator-proto` build uses vendored `protoc`
- no `Could not find protoc` error appears

## Scenario 3: Full Workspace Regression

1. Run:
   ```bash
   cargo test --workspace --lib
   ```
2. Run:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   ```

Expected:

- all unit tests pass (integration tests excluded via `--lib` for safety)
- clippy reports no warnings

## Scenario 4: Self-Evolution Manifest Uses Worktree Isolation

1. Open `fixtures/workflow/self-evolution.yaml`.
2. Confirm workflow spec contains:
   - `item_isolation.strategy: git_worktree`
   - `cleanup: after_workflow`
3. Confirm item-scoped prompts forbid agent-managed `git checkout`, `git branch`, and `git worktree`.

Expected:

- workflow is configured for physical item isolation
- agents are not instructed to interfere with engine-managed git state

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ✅ ALL PASS | S1 PASS, S2 PASS, S3 PASS (435 tests, clippy clean), S4 PASS — 2026-03-29 |
