# Design Doc 27: SecretStore Key Lifecycle Governance

**FR**: FR-012
**Status**: Implemented
**Author**: system
**Created**: 2026-03-12

## Overview

SecretStore key lifecycle governance adds key state machine, rotation, revocation, and audit trail capabilities to the existing secret encryption system.

## Key State Machine

```
Active ──→ DecryptOnly ──→ Retired (terminal)
  │              │
  │(--force)     │
  ▼              ▼
Revoked       Revoked (terminal)
(terminal)
```

**States**:
- `active` — used for both encryption and decryption; exactly one active key at any time
- `decrypt_only` — can decrypt historical data but not encrypt new data; created during rotation
- `retired` — terminal state after successful rotation completion; all data migrated away
- `revoked` — terminal state after explicit revocation; key file preserved but unused

## Rotation Flow

1. `begin_rotation`: Generate new key file → `data/secrets/keys/{key_id}.key`; new key = `active`, old key = `decrypt_only`
2. `re_encrypt_all_secrets`: Single transaction re-encrypts all `SecretStore` resources and `resource_versions` from old key to new key
3. `complete_rotation`: Verify no data references old key → old key = `retired`

Interruption safety: Old key remains `decrypt_only` (still readable). Resume with `secret key rotate --resume`.

## Revocation

- `revoke_key(key_id, force)`: Transitions key to `revoked`
- Active key revocation requires `--force` and blocks all subsequent SecretStore writes
- Revoked keys cannot decrypt; historical data encrypted with revoked key becomes inaccessible

## Audit Trail

`secret_key_audit` table captures:
- `key_created`, `key_activated` — key lifecycle events
- `rotate_started`, `rotate_completed` — rotation milestones
- `key_revoked` — revocation events
- `decrypt_failed` — operational failures
- `missing_key_diagnostic` — startup diagnostics

## KeyRing

`KeyRing` aggregates all non-terminal keys for multi-key decryption:
- Active key → encryption + decryption
- DecryptOnly keys → decryption only
- `SecretEncryption::from_keyring()` enables envelope-based key dispatch

## Database Schema (m0016)

- `secret_keys` table: key metadata with state machine columns
- `secret_key_audit` table: append-only audit log
- Legacy key import during bootstrap populates both tables

## Backward Compatibility

1. `ensure_secret_key()` preserved — delegates to `load_keyring().active_key()` internally
2. `SecretEncryption::from_key()` preserved — single-key callers unchanged
3. Legacy `key_id: "primary"` envelopes decrypt correctly via keyring lookup
4. New keys use `k-{timestamp}-{random4}` format
5. Key files: base64 AES-256 key, permissions 0600, format unchanged

## CLI Surface

```
orchestrator secret key status    # Active key info
orchestrator secret key list      # All keys with state
orchestrator secret key rotate    # Begin + complete rotation
orchestrator secret key rotate --resume  # Resume incomplete rotation
orchestrator secret key revoke <key_id> [--force]  # Revoke key
orchestrator secret key history [-n limit] [--key-id <id>]  # Audit log
```

## gRPC RPCs

- `SecretKeyStatus`, `SecretKeyList`, `SecretKeyRotate`, `SecretKeyRevoke`, `SecretKeyHistory`

## Key Files

| File | Purpose |
|------|---------|
| `core/src/secret_key_lifecycle.rs` | State machine, KeyRing, rotation, revocation |
| `core/src/secret_key_audit.rs` | Audit event types and DB operations |
| `core/src/secret_store_crypto.rs` | Multi-key decrypt dispatch, `from_keyring` |
| `core/src/persistence/migration_steps.rs` | m0016 schema |
| `core/src/service/bootstrap.rs` | Legacy import, startup diagnostics |
| `crates/cli/src/commands/secret.rs` | CLI dispatch |
| `crates/daemon/src/server/secret.rs` | gRPC handler |
