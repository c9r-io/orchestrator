# Orchestrator - EnvStore and SecretStore Resource CRUD

**Module**: orchestrator
**Scope**: EnvStore/SecretStore resource apply, get, delete, export, and validation
**Scenarios**: 5
**Priority**: High

---

## Background

Two new resource kinds (`EnvStore` and `SecretStore`) allow declaring reusable environment variable sets in YAML manifests. Both share the same `spec.data` shape (`HashMap<String, String>`) but differ in the `sensitive` flag: EnvStore is non-sensitive, SecretStore is sensitive (values are redacted in logs).

Both resources use the standard `apply` / `manifest export` / `delete` CLI commands and are stored in the unified `env_stores` config map.

---

## Scenario 1: Apply EnvStore and SecretStore via Multi-Document YAML

### Preconditions
- Orchestrator binary is built and accessible via `./scripts/run-cli.sh`
- A clean project environment exists

### Goal
Verify that both EnvStore and SecretStore resources can be applied from a single multi-document YAML file, and that they appear in the exported manifest with correct kind labels.

### Steps
1. Create a multi-document YAML file `test-env-stores.yaml`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: EnvStore
   metadata:
     name: shared-config
   spec:
     data:
       DATABASE_URL: "postgres://localhost/testdb"
       LOG_LEVEL: "debug"
   ---
   apiVersion: orchestrator.dev/v2
   kind: SecretStore
   metadata:
     name: api-keys
   spec:
     data:
       OPENAI_API_KEY: "sk-test-key-123"
   ```
2. Run `./scripts/run-cli.sh apply -f test-env-stores.yaml`
3. Run `./scripts/run-cli.sh manifest export`

### Expected
- Apply outputs `Created` for both `shared-config` and `api-keys`
- Export includes a document with `kind: EnvStore`, `name: shared-config`, and `data` containing `DATABASE_URL` and `LOG_LEVEL`
- Export includes a document with `kind: SecretStore`, `name: api-keys`, and `data` containing `OPENAI_API_KEY`

### Expected Data State
```sql
-- Verify env_stores in config (SQLite stores serialized OrchestratorConfig)
-- EnvStore entry should have sensitive=false, SecretStore should have sensitive=true
-- Check via manifest export output rather than direct DB query
```

---

## Scenario 2: Apply Idempotency — Unchanged on Re-Apply

### Preconditions
- Scenario 1 completed successfully (both stores exist in config)

### Goal
Verify that re-applying the same manifest produces `Unchanged` status for both resources (idempotent apply).

### Steps
1. Run `./scripts/run-cli.sh apply -f test-env-stores.yaml` again (same file from Scenario 1)

### Expected
- Output shows `Unchanged` for `shared-config`
- Output shows `Unchanged` for `api-keys`
- No data is modified in the config

---

## Scenario 3: Delete EnvStore and SecretStore by Kind Alias

### Preconditions
- Both `shared-config` (EnvStore) and `api-keys` (SecretStore) exist in config

### Goal
Verify that delete works with all supported kind aliases, and that deleting one kind does not affect the other.

### Steps
1. Run `./scripts/run-cli.sh delete env-store shared-config`
2. Run `./scripts/run-cli.sh manifest export` — verify `shared-config` is absent
3. Re-apply `shared-config` from Scenario 1 YAML
4. Run `./scripts/run-cli.sh delete secret-store api-keys`
5. Run `./scripts/run-cli.sh manifest export` — verify `api-keys` is absent, `shared-config` still present

### Expected
- Step 1: delete returns success
- Step 2: export does not contain `shared-config`; `api-keys` still present
- Step 4: delete returns success
- Step 5: export does not contain `api-keys`; `shared-config` present

---

## Scenario 4: Validate Rejects Empty Resource Name

### Preconditions
- Orchestrator binary is built

### Goal
Verify that applying an EnvStore or SecretStore with an empty name produces a validation error.

### Steps
1. Create `bad-env-store.yaml`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: EnvStore
   metadata:
     name: ""
   spec:
     data:
       KEY: "value"
   ```
2. Run `./scripts/run-cli.sh apply -f bad-env-store.yaml`

### Expected
- Apply fails with an error message containing "name" or "empty"
- No resource is created in config

---

## Scenario 5: EnvStore and SecretStore Isolation — Cross-Kind Get/Delete

### Preconditions
- An EnvStore named `cross-test` is applied
- A SecretStore named `cross-test-secret` is applied

### Goal
Verify that `get_from` for EnvStore skips sensitive entries, and `get_from` for SecretStore skips non-sensitive entries. Also verify that `delete` for the wrong kind returns false.

### Steps
1. Apply a manifest with:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: EnvStore
   metadata:
     name: cross-test
   spec:
     data:
       ENV_VAR: "public-value"
   ---
   apiVersion: orchestrator.dev/v2
   kind: SecretStore
   metadata:
     name: cross-test-secret
   spec:
     data:
       SECRET_VAR: "secret-value"
   ```
2. Verify via unit tests that:
   - `EnvStoreResource::get_from(config, "cross-test-secret")` returns `None` (sensitive entry)
   - `SecretStoreResource::get_from(config, "cross-test")` returns `None` (non-sensitive entry)
   - `EnvStoreResource::delete_from(config, "cross-test-secret")` returns `false`
   - `SecretStoreResource::delete_from(config, "cross-test")` returns `false`

### Expected
- Cross-kind access is correctly blocked by the `sensitive` flag
- Each resource kind only operates on entries matching its sensitivity level

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Apply EnvStore and SecretStore via multi-document YAML | ☐ | | | |
| 2 | Apply idempotency — Unchanged on re-apply | ☐ | | | |
| 3 | Delete EnvStore and SecretStore by kind alias | ☐ | | | |
| 4 | Validate rejects empty resource name | ☐ | | | |
| 5 | EnvStore/SecretStore isolation — cross-kind get/delete | ☐ | | | |
