---
lifecycle: active
related_fr: FR-123
self_referential_safe: true
---

# Orchestrator - Slack Sandbox Continuous Certification

**Module**: Orchestrator / Slack Integration Gateway

**Scope**: Unified live modes, checkpoints, secret custody, provider scenarios,
evidence freshness, and cleanup

**Scenarios**: 5
**Priority**: Critical

---

## Background

The ordinary test suite is network-free. It validates the certification
controller and sanitized provider-boundary recordings, but it does not claim a
new live Slack result.

Live execution is opt-in and uses isolated sandbox workspaces, Gateway,
daemons, and deterministic echo fixtures. Exit code `20` means the run is
waiting at a resumable human/provider checkpoint.

---

## Scenario 1: Network-Free Controller And Recorded Provider Contracts

### Preconditions

- Bash, `jq`, `rg`, `git`, `date`, `stat`, and `mktemp` are installed.
- No Slack, Cloudflare, Gateway, daemon, or AI-provider credential is loaded.

### Goal

Verify the control plane, recorded boundary shapes, and missing-secret behavior
without network access.

### Steps

1. Run:

   ```bash
   ./scripts/qa/test-slack-live-certification.sh
   ```

2. Inspect:

   ```bash
   jq . fixtures/slack/certification/recorded-contracts.json
   jq . config/qa/slack-live-certification-policy.json
   ```

3. Confirm CI executes this command on both Ubuntu and macOS.

### Expected

- Thirteen shell assertions pass.
- The fixture contains exactly sanitized OAuth callback, Events API delivery,
  manifest diff, and Gateway import receipt cases.
- Unsafe env permissions, unknown variables, command substitution, known
  secrets, generic token shapes, and incomplete recordings fail closed.
- A missing live env emits safe `blocked` evidence and exit `20`; ordinary CI
  does not invoke Slack.

---

## Scenario 2: Unified Modes, Stable Checkpoints, And Minimal Environment

### Preconditions

- Copy and populate the reviewed example outside the repository:

  ```bash
  mkdir -p ~/.config/orchestrator/qa
  cp config/qa/slack-live.env.example \
    ~/.config/orchestrator/qa/slack-live.env
  chmod 600 ~/.config/orchestrator/qa/slack-live.env
  ```

- Use only sandbox resources and echo-only fixture workflows.

### Goal

Verify shared, dedicated, and combined runs pause and resume without replaying
completed stages or exporting unrelated secrets.

### Steps

1. Start each mode with a distinct run ID:

   ```bash
   ./scripts/qa/certify-slack-managed-live.sh run \
     --mode {shared|dedicated|both} \
     --run-id {run_id} \
     --env-file ~/.config/orchestrator/qa/slack-live.env
   ```

2. At an OAuth/provider pause, inspect only the safe projection:

   ```bash
   jq '{run_id,mode,status,stages,inventory,cleanup}' \
     "${XDG_STATE_HOME:-$HOME/.local/state}/orchestrator/slack-certification/{run_id}/safe-result.json"
   ```

3. Complete the documented provider action, record the current checkpoint, and
   resume:

   ```bash
   ./scripts/qa/certify-slack-managed-live.sh checkpoint \
     --run-id {run_id} \
     --stage {current_stage} \
     --result pass \
     --evidence-code {safe_code}

   ./scripts/qa/certify-slack-managed-live.sh resume \
     --run-id {run_id} \
     --env-file ~/.config/orchestrator/qa/slack-live.env
   ```

4. Attempt to attest a non-current or automated stage.
5. Inspect the badge smoke process environment from a controlled test harness;
   do not dump the real token value.

### Expected

- `run`, `resume`, and `checkpoint` preserve completed stages and resume the
  first waiting/blocked stage.
- Unknown/non-current stages are rejected.
- Aggregate tests receive no live credentials.
- The badge child receives only process basics, one selected connection,
  channel/actor, two reactions/markers, timeout, and its sandbox driver token.
- Private run state is mode `0600`; the directory is mode `0700`.

---

## Scenario 3: Shared Official-App Live Matrix

### Preconditions

- Complete the shared topology and L0-L11 prerequisites in
  `docs/guide/slack-managed-sandbox-certification-runbook.md`.
- Two sandbox workspaces use the reviewed official App and separate daemon
  data directories.
- Apply the echo-only fixture to both projects:

  ```bash
  orchestrator apply --project {project_a} \
    -f fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml
  orchestrator apply --project {project_b} \
    -f fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml
  ```

### Goal

Certify real shared OAuth, tenant isolation, routing, recovery, revocation, and
disconnect through the unified controller.

### Steps

1. Run `--mode shared`.
2. Complete `shared_oauth` and `shared_multi_workspace`, recording only safe
   evidence codes.
3. Allow `shared_badges` to post one synthetic message and add both configured
   reactions.
4. Execute and attest `shared_cursor_recovery`.
5. Execute revocation/disconnect, verify fail-closed behavior, and attest
   `shared_revocation_disconnect`.
6. Compare connection/task counts and safe cursor transitions with the runbook.

### Expected

- Both installations are active and tenant-isolated before routing.
- Two badges on one message select two different Skill/template/workflow tasks.
- Removing/re-adding one reaction creates no third task.
- A delivery received while the daemon is offline advances the cursor after
  restart, returns lag to zero, and creates one task.
- Revoked/disconnected delivery creates no new route/task while historical
  evidence remains readable.

---

## Scenario 4: Dedicated App Live Matrix And Destructive Review

### Preconditions

- Complete the dedicated addendum in
  `docs/guide/slack-dedicated-app-provisioning.md`.
- Use a new sandbox workspace/App and a fresh Configuration Token supplied only
  through stdin.
- Apply the echo-only fixture:

  ```bash
  orchestrator apply --project {dedicated_project} \
    -f fixtures/manifests/bundles/slack-managed-dedicated-app-fixture.yaml
  ```

### Goal

Certify create-once provisioning, manifest receipt, exact-App OAuth/lifecycle,
same-message routing, recovery, disconnect, and reviewed App deletion.

### Steps

1. Run `--mode dedicated` or continue the dedicated half of `--mode both`.
2. Complete `dedicated_provision_oauth` and
   `dedicated_manifest_receipt`.
3. Let `dedicated_badges` execute the same-message smoke.
4. Execute cursor recovery and exact-App reauthorization; verify generation
   advancement before attesting each checkpoint.
5. Disconnect the installation.
6. Delete the sandbox App only after independent review, then record:

   ```bash
   ./scripts/qa/certify-slack-managed-live.sh checkpoint \
     --run-id {run_id} \
     --stage dedicated_disconnect_delete \
     --result pass \
     --evidence-code app_deleted \
     --confirm-destructive {run_id}
   ```

### Expected

- App creation is not blindly repeated across resume.
- OAuth begins only after the connection-scoped Gateway receipt is verified.
- Permission expansion suspends delivery until exact-App reauthorization.
- Two badges on one message create two distinct tasks; duplicate delivery
  converges.
- Disconnect and App deletion remain separate. A passing deletion checkpoint
  without exact run-ID confirmation is rejected.

---

## Scenario 5: Evidence Freshness, Leakage Scan, Cleanup, And Release Display

### Preconditions

- A completed or interrupted certification run exists.
- `docs/qa/evidence/slack-live-certification-latest.json` contains only reviewed
  safe evidence.

### Goal

Verify safe promotion, time-derived freshness, non-destructive expiry
semantics, and rerunnable cleanup.

### Steps

1. Inspect current reviewed status:

   ```bash
   ./scripts/qa/certify-slack-managed-live.sh status
   ./scripts/qa/certify-slack-managed-live.sh status --json | jq .
   ```

2. Confirm `--require-fresh` passes only when both modes have a fresh PASS.
3. Run cleanup without destructive confirmation:

   ```bash
   ./scripts/qa/certify-slack-managed-live.sh cleanup \
     --run-id {run_id}
   ```

4. Verify local/non-destructive items become cleaned while App/domain/workspace
   review remains visible.
5. After performing reviewed external cleanup, run:

   ```bash
   ./scripts/qa/certify-slack-managed-live.sh cleanup \
     --run-id {run_id} \
     --confirm-destructive {run_id} \
     --mark-external-cleaned
   ```

6. Promote only after status, privacy scan, and cleanup are PASS:

   ```bash
   ./scripts/qa/certify-slack-managed-live.sh promote \
     --run-id {run_id}
   ```

7. Inspect the normal CI and release workflow definitions.

### Expected

- Safe evidence contains no private ID, salt, token, OAuth material, provider
  URL, raw payload, or message body.
- Interrupted runs retain a deterministic cleanup inventory.
- Destructive cleanup stays pending until exact run-ID confirmation.
- `stale` is reported as
  `recertification_required_not_product_regression`.
- Ordinary CI replays recorded fixtures only. Release CI displays status and
  never loads live credentials.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Network-free controller and recorded provider contracts | PASS | 2026-07-25 | Codex | 13/13 shell assertions passed, including from-zero OAuth ordering and safe combined promotion |
| 2 | Unified modes, stable checkpoints, and minimal environment | PASS | 2026-07-25 | Codex | Missing-secret checkpoint, state advance, env allowlist, and `env -i` contract verified |
| 3 | Shared official-App live matrix | PASS | 2026-07-22 | Codex | Fresh reviewed FR-114 L0-L11 record imported with 2026-08-21 expiry |
| 4 | Dedicated App live matrix and destructive review | PASS | 2026-07-22 | Codex | Fresh reviewed FR-115 provider record includes same-message badges, cursor recovery, reauth, disconnect, and delete |
| 5 | Evidence freshness, leakage scan, cleanup, and release display | PASS | 2026-07-25 | Codex | Fresh/stale, redaction, private/safe inventory, explicit cleanup, CI/release status passed |
