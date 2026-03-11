# Orchestrator - EnvStore/SecretStore Resources and Agent Env Configuration

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Add EnvStore/SecretStore resources and agent env configuration
**Related QA**: `docs/qa/orchestrator/37-envstore-secretstore-resources.md`, `docs/qa/orchestrator/38-agent-env-resolution.md`
**Created**: 2026-03-03
**Last Updated**: 2026-03-03

---

## Background

Agents currently have no way to receive custom environment variables. The runner layer has `env_allowlist` to control which system env vars pass through, but there is no mechanism to define and inject custom env vars per agent. Teams need to pass database URLs, API keys, and configuration values to agents without hardcoding them in command templates.

## Goals

- Provide two new resource kinds (`EnvStore` / `SecretStore`) for declaring reusable environment variable sets
- Add an `env` field on Agent spec to reference stores and define direct values
- Support three entry forms: direct `name`+`value`, `fromRef` (import all keys), `name`+`refValue` (import single key)
- Resolve agent env entries at runtime and inject into spawned processes
- Redact SecretStore values in logs automatically
- Validate store references at config build time

## Non-goals

- Vault or external secret provider integration (all values are stored in SQLite config)
- Per-step env overrides (env is agent-scoped only)
- Encryption at rest for SecretStore values in the database

---

## Scope

- In scope: resource CRUD (apply/get/delete/export), YAML parsing, agent env resolution, runner injection, config validation, redaction
- Out of scope: UI, external secret backends, per-step env
- Key rotation: now covered by FR-012 (see `docs/design_doc/orchestrator/27-secretstore-key-lifecycle.md`)

---

## Interfaces and Data

### YAML Manifests

```yaml
kind: EnvStore
metadata:
  name: shared-config
spec:
  data:
    DATABASE_URL: "postgres://localhost/mydb"
    LOG_LEVEL: "debug"
---
kind: SecretStore
metadata:
  name: api-keys
spec:
  data:
    OPENAI_API_KEY: "sk-..."
---
kind: Agent
metadata:
  name: coder
spec:
  command: claude -p "{prompt}"
  env:
    - name: LOG_LEVEL
      value: "debug"
    - fromRef: shared-config
    - name: MY_API_KEY
      refValue:
        name: api-keys
        key: OPENAI_API_KEY
```

### Data Changes

- `OrchestratorConfig.env_stores: HashMap<String, EnvStoreConfig>` — single map, `sensitive: bool` flag distinguishes EnvStore (false) from SecretStore (true)
- `AgentConfig.env: Option<Vec<AgentEnvEntry>>` — raw entries, resolved at runtime
- `AgentSpec.env: Option<Vec<AgentEnvEntry>>` — YAML ↔ config passthrough

### Key Types

| Type | Fields | Purpose |
|------|--------|---------|
| `EnvStoreSpec` | `data: HashMap<String, String>` | Shared spec shape for both EnvStore and SecretStore |
| `EnvStoreConfig` | `data: HashMap<String, String>`, `sensitive: bool` | Runtime config distinguishing store kinds |
| `AgentEnvEntry` | `name?`, `value?`, `fromRef?`, `refValue?` | Three mutually exclusive entry forms |
| `AgentEnvRefValue` | `name: String`, `key: String` | Store name + key for single-key import |

### CLI Aliases for delete

| Kind | Aliases |
|------|---------|
| EnvStore | `envstore`, `env-store`, `env_store` |
| SecretStore | `secretstore`, `secret-store`, `secret_store` |

---

## Key Design and Tradeoffs

### 1. Shared storage map with sensitive flag

EnvStore and SecretStore share the same `config.env_stores` HashMap. A `sensitive: bool` flag distinguishes them. This avoids duplicate maps and simplifies resolution — `resolve_agent_env()` looks up one map regardless of kind.

### 2. Shared ResourceSpec variant (serde untagged)

Both kinds use `ResourceSpec::EnvStore(EnvStoreSpec)` because `serde(untagged)` cannot distinguish structurally identical variants. The `ResourceKind` field on `OrchestratorResource` disambiguates during dispatch.

### 3. Three entry forms for agent env

A flat struct with optional fields (`name`, `value`, `fromRef`, `refValue`) is used instead of a tagged enum. This matches Kubernetes-style YAML ergonomics while remaining straightforward to validate.

### 4. Two-phase validation

- **Build time**: `validate_agent_env_store_refs()` verifies that all store names referenced by agents exist in `config.env_stores`
- **Runtime**: `resolve_agent_env()` verifies key existence within stores and produces the final `HashMap<String, String>`

## Alternatives and Tradeoffs

- **Separate maps for EnvStore/SecretStore**: More explicit, but duplicates resolution logic. Rejected for simplicity.
- **Tagged enum for ResourceSpec**: Would avoid the serde(untagged) collision, but breaks the existing untagged pattern used by all other resource specs.
- **Inline env on Agent only (no stores)**: Simpler but forces duplication when multiple agents share the same variables.

## Risks and Mitigations

- **Risk**: Secrets stored in plaintext in SQLite.
  - **Mitigation**: SecretStore values are encrypted at rest (AES-256-GCM-SIV) and redacted in logs via `collect_sensitive_values()` + `redact_text()`. Key rotation, revocation, and audit are governed by FR-012 (`docs/design_doc/orchestrator/27-secretstore-key-lifecycle.md`).
- **Risk**: Store deletion while agents still reference it.
  - **Mitigation**: Build-time validation catches dangling references. Runtime resolution returns an error with a clear message.

---

## Observability

- **Logs**: SecretStore values are collected by `collect_sensitive_values()` and added to runner `redaction_patterns`, ensuring `[REDACTED]` replacement in task output logs.
- **Metrics**: No new metrics. Existing task execution metrics cover env-injected runs.
- **Tracing**: No new spans. Env resolution happens synchronously before spawn.

## Operations / Release

- **Config**: No new env vars for the orchestrator itself.
- **Migration / rollback**: New fields are additive and optional. Existing configs without `env_stores` or agent `env` continue to work (defaults to empty HashMap / None).
- **Compatibility**: Fully backward-compatible. Old YAML manifests parse without changes.

---

## Test Plan

- **Unit tests** (980+ in `cargo test --lib`):
  - `cli_types.rs`: YAML parse tests for EnvStore, SecretStore, Agent with env, Agent without env
  - `resource/env_store.rs`: apply, get_from, delete, validate, to_yaml, skips-sensitive
  - `resource/secret_store.rs`: apply, get_from, delete, validate, to_yaml, skips-non-sensitive
  - `env_resolve.rs`: all 3 entry forms, missing store/key errors, invalid entry, override precedence, sensitive value collection
  - `runner.rs`: extra_env injection, allowlist + extra_env coexistence
  - `config_load/validate.rs`: validate_agent_env_store_refs (valid, missing fromRef, missing refValue, no env)
  - `resource/parse.rs`: delete aliases, kind_as_str for EnvStore/SecretStore
- **Integration tests** (24 in `tests/integration_test.rs`): existing tests pass with new fields

## QA Docs

- `docs/qa/orchestrator/37-envstore-secretstore-resources.md`
- `docs/qa/orchestrator/38-agent-env-resolution.md`

## Acceptance Criteria

- EnvStore and SecretStore resources can be applied, exported, and deleted via CLI
- Agent env entries resolve correctly for all three forms
- Missing store references produce clear validation errors at config build time
- Missing keys produce clear errors at runtime resolution
- SecretStore values are redacted in task output logs
- All existing tests pass without modification (backward-compatible)
