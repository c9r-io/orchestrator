# DD-095: SecretStore Key Emergency Recovery

## Context

When all secret keys reach terminal state (retired/revoked), the system has no CLI-accessible recovery path. `secret key rotate` requires an existing active key, making it impossible to recover without direct database manipulation.

## Design

### `secret key bootstrap` command

A new `bootstrap` subcommand under `secret key` that creates a fresh active encryption key when no active key exists. This is an emergency recovery mechanism, not a normal operational command.

**Preconditions:**
- `query_active_key_record()` must return `None` (fails with clear error if an active key already exists)

**Behavior:**
- Generates a new key ID via `generate_key_id()`
- Creates key file under `secrets/keys/<key_id>.key` with `0o700` directory permissions
- Inserts a new `Active` record into `secret_keys` table
- Emits `KeyBootstrapped` audit event with actor `cli:bootstrap`
- Returns the new key record

**Limitation:** Previously encrypted SecretStore resources remain encrypted with the old key. They cannot be decrypted with the bootstrapped key. The operator must re-apply those resources after bootstrapping. This is expected — the emergency recovery restores the ability to perform new SecretStore operations.

### Enhanced `secret key revoke` warning

When the user attempts to revoke the last (only) active key without `--force`, the error message now explicitly warns that SecretStore will become inoperable and suggests `secret key bootstrap` as the recovery path.

## Files Modified

- `crates/orchestrator-security/src/secret_key_lifecycle.rs` — `bootstrap_key()`, enhanced `revoke_key()`
- `crates/orchestrator-security/src/secret_key_audit.rs` — `KeyBootstrapped` variant
- `crates/proto/orchestrator.proto` — `SecretKeyBootstrap` RPC
- `crates/daemon/src/server/secret.rs` — `secret_key_bootstrap()` handler
- `crates/daemon/src/server/mod.rs` — RPC dispatch
- `crates/cli/src/cli.rs` — `Bootstrap` subcommand
- `crates/cli/src/commands/secret.rs` — CLI handler

## Audit Trail

New `KeyBootstrapped` audit event kind (`key_bootstrapped`) records each bootstrap operation with reason detail.
