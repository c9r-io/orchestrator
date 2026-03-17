---
self_referential_safe: false
---

# QA 64: SecretStore Key Lifecycle

**FR**: FR-012
**Scenarios**: 5

## Preconditions (all scenarios)

- Daemon running: `./target/release/orchestratord --foreground --workers 2`
- CLI available: `./target/release/orchestrator`
- Existing SecretStore resources present (pre-existing or applied via inline YAML below)

---

## Scenario 1: Legacy Key Migration

**Objective**: Verify that m0016 migration correctly imports the existing `primary` key into `secret_keys` table, and existing SecretStore data remains accessible.

### Steps

1. Start daemon (triggers m0016 migration and bootstrap legacy key import).
2. Run `orchestrator secret key list --output json`.
3. Run `orchestrator secret key status`.
4. Verify existing SecretStore resources are stored in the database: `sqlite3 data/agent_orchestrator.db "SELECT name, kind FROM resources WHERE kind='SecretStore';"`.
5. Run `orchestrator secret key history --output json`.

### Expected Result

- `secret key list` returns one key: `key_id: "primary"`, `state: "active"`.
- `secret key status` shows the active key with fingerprint and created_at.
- SecretStore resources exist in the database (confirms data integrity after migration).
- `secret key history` shows `key_created` and `key_activated` events with `actor: "system:migration"` and `detail_json` containing `"source":"legacy_import"`.

---

## Scenario 2: Key Rotation Forward Path

**Objective**: Full rotation lifecycle — generate new key, re-encrypt all data, retire old key.

### Preconditions

- At least one SecretStore resource exists. If not, apply one:
  ```bash
  cat <<'YAML' | orchestrator apply -f -
  apiVersion: orchestrator.dev/v2
  kind: SecretStore
  metadata:
    name: qa64-rotation-test
    project: default
  spec:
    data:
      API_KEY: "sk-qa64-test-rotation"
  YAML
  ```

### Steps

1. Run `orchestrator secret key list` — note the current active key_id.
2. Run `orchestrator secret key rotate`.
3. Run `orchestrator secret key list --output json`.
4. Verify SecretStore resources remain accessible after re-encryption by applying a new one:
   ```bash
   cat <<'YAML' | orchestrator apply -f -
   apiVersion: orchestrator.dev/v2
   kind: SecretStore
   metadata:
     name: qa64-post-rotate-verify
     project: default
   spec:
     data:
       VERIFY_KEY: "post-rotation-ok"
   YAML
   ```
5. Run `orchestrator secret key history --output json`.

### Expected Result

- Step 2: Output confirms rotation complete, showing new key ID (`k-{timestamp}-{hex4}` format) and old key retired.
- Step 3: `secret key list` shows:
  - New key: `state: "active"`, `key_id` starts with `k-`
  - Old key: `state: "retired"`, `retired_at` populated
- Step 4: Apply succeeds (proves encryption with the new active key works).
- Step 5: History shows `key_created`, `key_activated`, `rotate_started`, `rotate_completed` events in chronological order.

---

## Scenario 3: Resume and No-op Resume Guard

**Objective**: Verify `--resume` behavior: succeeds when incomplete rotation exists, returns clear error when no rotation is pending.

### Steps

1. Run `orchestrator secret key rotate --resume` (no incomplete rotation exists).
2. Verify error message indicates no incomplete rotation.

### Expected Result

- Step 1: Returns error containing "no incomplete rotation found" or similar diagnostic.
- The active key and all data remain unchanged.

### Notes

Full interruption-resume testing requires simulating a mid-rotation crash, which is covered by the `full_rotation_lifecycle` unit test in `core/src/secret_key_lifecycle.rs`. This scenario validates the CLI surface and guard logic.

---

## Scenario 4: Key Revocation Safety

**Objective**: Verify revocation guards — active key requires `--force`, and revoked key blocks writes.

### Preconditions

- A fresh rotation has been performed (Scenario 2), so a new active key exists.

### Steps

1. Run `orchestrator secret key list` — note the active key_id.
2. Run `orchestrator secret key revoke <active_key_id>` (without `--force`).
3. Verify rejection with `--force` hint.
4. Run `orchestrator secret key revoke <active_key_id> --force`.
5. Run `orchestrator secret key list --output json` — verify key is revoked.
6. Attempt to apply a new SecretStore:
   ```bash
   cat <<'YAML' | orchestrator apply -f -
   apiVersion: orchestrator.dev/v2
   kind: SecretStore
   metadata:
     name: qa64-post-revoke
     project: default
   spec:
     data:
       BLOCKED_KEY: "should-fail"
   YAML
   ```

### Expected Result

- Step 2: Rejected with error message containing `--force`.
- Step 4: Succeeds, output confirms key revoked.
- Step 5: Key shows `state: "revoked"`, `revoked_at` populated.
- Step 6: Apply fails with error mentioning "no active encryption key" or "write blocked".
- `secret key history` shows `key_revoked` event with `detail_json` containing `"force":true`.

---

## Scenario 5: Audit History Query

**Objective**: Verify complete audit trail and filtering after a create/rotate/revoke sequence.

### Preconditions

- Scenarios 1–4 have been executed (or equivalent operations performed).

### Steps

1. Run `orchestrator secret key history --output json` — full history.
2. Run `orchestrator secret key history -n 3 --output json` — limited results.
3. Run `orchestrator secret key history --key-id primary --output json` — filter by legacy key.

### Expected Result

- Step 1: Events listed in reverse chronological order. Expected event kinds across the session: `key_created`, `key_activated`, `rotate_started`, `rotate_completed`, `key_revoked`.
- Step 2: Returns at most 3 events (most recent first).
- Step 3: Returns only events where `key_id == "primary"` — the legacy key's `key_created` and `key_activated` events from migration.
- Each event includes: `event_kind`, `key_id`, `key_fingerprint`, `actor`, `detail_json`, `created_at`.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Legacy Key Migration | ☐ | | | |
| 2 | Key Rotation Forward Path | ☐ | | | |
| 3 | Resume and No-op Resume Guard | ☐ | | | |
| 4 | Key Revocation Safety | ☐ | | | |
| 5 | Audit History Query | ☐ | | | |
