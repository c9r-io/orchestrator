# Business Logic Security - Workflow Abuse And Replay Tests (Generic)

**Module**: Business Logic Security  
**Scope**: Workflow step skipping, one-time token replay, idempotency and rollback  
**Scenarios**: 4  
**Risk**: Critical  
**OWASP ASVS 5.0**: V2 Validation and Business Logic

---

## Background

Common attacks against business workflows:
- Skipping prerequisite steps and directly calling later APIs
- Reusing one-time tokens (reset/invite/redeem)
- Missing idempotency causing double charges or duplicate creation

Project-specific overlay: FR-097 resume execution must reject a missing prerequisite plan, expired/stale `expected_state_version`, changed boundary, reused idempotency key with different input, and non-idempotent replay without both project policy and elevated operator confirmation. Planning must not mutate tasks, Attention state, scheduler queues, or the workspace. See `docs/qa/orchestrator/144-handoff-and-safe-resume.md`.

FR-109/FR-110 source-routing overlay: exact reaction, target, channel, and Trigger-derived role must produce exactly one enabled `SourceTaskBinding`. Zero matches are explainable no-ops; multiple matches fail closed. Omitted channel/role restrictions, cross-project references, and overlapping rules are rejected before active config publication. Trigger `reactionRouting` defaults to `disabled`; once enabled, a durable automation key over project/installation/message/reaction/binding plus a deterministic task ID must converge delivery replay, concurrent reservation, and restart recovery on one canonical task. The selected binding/template revisions are frozen before provider work, while the outbound credential is resolved freshly from SecretStore. See `docs/qa/orchestrator/157-source-task-binding-badge-matching.md` and `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md`.

FR-112 overlay: unsaved GUI preview/simulation must overlay exactly one expected manifest onto an isolated active-config clone and call the production matcher/renderer without persistence or network access. Frontend fields cannot locally select a Skill or binding outside daemon validation. Replay defaults to the pinned generation and only adopts current configuration after explicit reviewed confirmation. See `docs/qa/orchestrator/160-process-console-source-automation-ui.md` Scenarios 1 and 3.

FR-114 overlay: callers cannot skip OAuth consent by forging an installation/project, reuse state/poll credentials, complete an intent without the exact Slack callback, or enable reaction routing through connect alone. Reauthorize, disconnect, and transfer require current connection version plus canonical Admin action context. A managed Trigger always begins with `reactionRouting: disabled`; badge task mutation remains governed by the existing preview/simulation/enable workflow. See `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md` Scenarios 1 and 4.

---

## Scenario 1: Step Skipping (Bypass Prerequisite Validation)

### Preconditions
- A multi-step workflow exists (for example create -> confirm -> execute)

### Attack Objective
Verify later steps cannot be called directly, or calling them forces the required validation.

### Attack Steps
1. Without performing step 1, call the API for step 2/3 directly
2. Forge workflow state fields (if any are client-controlled)

### Expected Secure Behavior
- Return 400/409/403
- Server enforces the real state machine and does not trust client-reported state

---

## Scenario 2: One-Time Token Replay (If Applicable)

### Preconditions
- A one-time token exists (invite/reset/redeem/confirmation link)

### Attack Objective
Verify tokens are single-use and become invalid immediately after use.

### Attack Steps
1. Obtain a token
2. Use it successfully once
3. Reuse the same token

### Expected Secure Behavior
- Second and subsequent attempts always fail
- Token consumption/update is atomic

---

## Scenario 3: Idempotency And Duplicate Submissions

### Preconditions
- Operations exist such as create/pay/redeem/send

### Attack Objective
Verify repeated submissions do not cause duplicate side effects.

### Attack Steps
1. Send the same request 10 times (same payload)
2. Send concurrently (see race-condition doc)

### Expected Secure Behavior
- There is an idempotency key and/or server-side deduplication
- Side effects occur at most once

---

## Scenario 4: Rollback And Intermediate Failure States

### Preconditions
- You can trigger mid-flow failures (downstream timeouts, validation failures)

### Attack Objective
Verify failures do not leave exploitable intermediate state (half-completed state).

### Attack Steps
1. Trigger a failure during the workflow
2. Check whether dirty data or reusable tokens are left behind

### Expected Secure Behavior
- Transactions/compensation ensure consistency
- Intermediate state cannot be exploited externally
