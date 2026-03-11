# QA 64: SecretStore Key Lifecycle

**FR**: FR-012
**Scenarios**: 5

## Preconditions (all scenarios)

- Daemon running: `./target/release/orchestratord --foreground --workers 2`
- CLI available: `./target/release/orchestrator`
- At least one SecretStore resource applied for rotation testing

---

## Scenario 1: Legacy Key Migration

**Objective**: Verify that upgrading to m0016 correctly imports the existing primary key.

### Steps

1. Start daemon (triggers migration and bootstrap).
2. Run `orchestrator secret key list --output json`.

### Expected Result

- One key with `key_id: "primary"`, `state: "active"` is listed.
- Existing SecretStore resources decrypt correctly: `orchestrator get secretstore` returns redacted data without errors.
- `orchestrator secret key history` shows `key_created` and `key_activated` events with `actor: "system:migration"`.

---

## Scenario 2: Key Rotation Forward Path

**Objective**: Full rotation lifecycle — generate new key, re-encrypt, retire old key.

### Preconditions

- Apply a SecretStore resource: `orchestrator apply -f fixtures/secretstore-test.yaml`

### Steps

1. Run `orchestrator secret key rotate`.
2. Run `orchestrator secret key list --output json`.
3. Run `orchestrator get secretstore` to verify data is accessible.

### Expected Result

- Output confirms rotation complete with new key ID and old key retired.
- `secret key list` shows:
  - New key: `state: "active"`, `key_id` starts with `k-`
  - Old key: `state: "retired"`
- SecretStore data decrypts correctly with the new key.
- `secret key history` shows `rotate_started` and `rotate_completed` events.

---

## Scenario 3: Rotation Interruption and Resume

**Objective**: Verify that an interrupted rotation can be resumed safely.

### Steps

1. Begin rotation manually (or simulate by interrupting during re-encryption).
2. Run `orchestrator secret key list` — verify old key is `decrypt_only`.
3. Run `orchestrator secret key rotate --resume`.
4. Run `orchestrator secret key list` — verify old key is `retired`.

### Expected Result

- `--resume` completes the re-encryption and retires the old key.
- Data integrity preserved: all SecretStore resources decrypt correctly.
- Running `rotate --resume` when no incomplete rotation exists returns a clear error message.

---

## Scenario 4: Key Revocation Safety

**Objective**: Verify revocation guards and write-blocking behavior.

### Steps

1. Run `orchestrator secret key list` to find the active key ID.
2. Run `orchestrator secret key revoke <active_key_id>` (without `--force`).
3. Run `orchestrator secret key revoke <active_key_id> --force`.
4. Attempt to apply a new SecretStore resource.

### Expected Result

- Step 2: Rejected with error message mentioning `--force`.
- Step 3: Key state changes to `revoked`.
- Step 4: Apply fails with "no active encryption key" error.
- `secret key history` shows `key_revoked` event with `force: true`.

---

## Scenario 5: Audit History Query

**Objective**: Verify complete audit trail after a create/rotate/revoke sequence.

### Steps

1. Start fresh (or use results from previous scenarios).
2. Run `orchestrator secret key history --output json`.
3. Run `orchestrator secret key history --key-id <specific_key> --output json`.

### Expected Result

- Full history shows events in chronological order: `key_created`, `key_activated`, `rotate_started`, `rotate_completed`, `key_revoked` (as applicable).
- `--key-id` filter returns only events for that specific key.
- Each event includes `key_fingerprint`, `actor`, `created_at`, and `detail_json`.
