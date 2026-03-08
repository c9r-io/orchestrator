# Orchestrator - Agent Env Resolution and Runner Injection

**Module**: orchestrator
**Scope**: Agent env entry forms, runtime resolution, runner injection, validation, and secret redaction
**Scenarios**: 5
**Priority**: High

---

## Background

Agents can now declare environment variables via the `env` field in their spec. Three entry forms are supported:

1. **Direct value**: `name` + `value` — sets a literal env var
2. **From ref**: `fromRef` — imports all keys from a named EnvStore/SecretStore
3. **Ref value**: `name` + `refValue` — imports a single key from a named store, optionally renaming it

Resolution happens at runtime via `resolve_agent_env()`. Resolved variables are injected into spawned processes via the `extra_env` parameter on `spawn_with_runner()`. SecretStore values are automatically collected for log redaction.

---

## Scenario 1: Agent with Direct Value Env Entry

### Preconditions
- Orchestrator binary is built
- A workspace and workflow exist in config

### Goal
Verify that an agent with a direct `name` + `value` env entry correctly receives the variable in its spawned process environment.

### Steps
1. Apply the following manifest:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: env-direct-agent
   spec:
     command: printf '%s' "${MY_DIRECT_VAR}"
     env:
       - name: MY_DIRECT_VAR
         value: "hello-direct"
   ```
2. Execute a task using `env-direct-agent`
3. Inspect task stdout output

### Expected
- Agent process stdout contains `hello-direct`
- The env var `MY_DIRECT_VAR` is available inside the spawned shell process

---

## Scenario 2: Agent with fromRef Importing All Store Keys

### Preconditions
- An EnvStore `shared-config` exists with `DATABASE_URL=postgres://localhost/testdb` and `LOG_LEVEL=debug`
- A workspace and workflow exist in config

### Goal
Verify that `fromRef` imports all keys from the referenced store into the agent's environment.

### Steps
1. Apply the EnvStore:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: EnvStore
   metadata:
     name: shared-config
   spec:
     data:
       DATABASE_URL: "postgres://localhost/testdb"
       LOG_LEVEL: "debug"
   ```
2. Apply an agent referencing the store:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: env-fromref-agent
   spec:
     command: printf '%s|%s' "${DATABASE_URL}" "${LOG_LEVEL}"
     env:
       - fromRef: shared-config
   ```
3. Execute a task using `env-fromref-agent`
4. Inspect task stdout output

### Expected
- Agent process stdout contains `postgres://localhost/testdb|debug`
- All keys from `shared-config` are injected into the process environment

---

## Scenario 3: Agent with refValue Importing Single Key with Rename

### Preconditions
- A SecretStore `api-keys` exists with `OPENAI_API_KEY=sk-test-key-123`
- A workspace and workflow exist in config

### Goal
Verify that `name` + `refValue` imports a single key from the referenced store, and the env var name can differ from the store key name.

### Steps
1. Apply the SecretStore:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: SecretStore
   metadata:
     name: api-keys
   spec:
     data:
       OPENAI_API_KEY: "sk-test-key-123"
   ```
2. Apply an agent with a renamed key:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: env-refvalue-agent
   spec:
     command: printf '%s' "${MY_API_KEY}"
     env:
       - name: MY_API_KEY
         refValue:
           name: api-keys
           key: OPENAI_API_KEY
   ```
3. Execute a task using `env-refvalue-agent`
4. Inspect task stdout output

### Expected
- Agent process stdout contains `sk-test-key-123`
- The env var is available as `MY_API_KEY` (renamed from `OPENAI_API_KEY`)

---

## Scenario 4: Config Validation Rejects Missing Store References

### Preconditions
- No store named `nonexistent-store` exists in config

### Goal
Verify that config build-time validation catches agents referencing non-existent stores and produces a clear error message.

### Steps
1. Save a manifest referencing a missing store via `fromRef`:
   ```bash
   cat > /tmp/bad-fromref-agent.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: bad-fromref-agent
   spec:
     command: "echo test"
     env:
       - fromRef: nonexistent-store
   EOF
   ```
2. Apply and observe validation error:
   ```bash
   orchestrator apply -f /tmp/bad-fromref-agent.yaml
   ```
3. Save a manifest referencing a missing store via `refValue`:
   ```bash
   cat > /tmp/bad-refvalue-agent.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: bad-refvalue-agent
   spec:
     command: "echo test"
     env:
       - name: KEY
         refValue:
           name: nonexistent-store
           key: SOME_KEY
   EOF
   ```
4. Apply and observe validation error:
   ```bash
   orchestrator apply -f /tmp/bad-refvalue-agent.yaml
   ```

### Expected
- Step 2: `apply` fails with error containing `"fromRef 'nonexistent-store' references unknown store"`
- Step 4: `apply` fails with error containing `"refValue.name 'nonexistent-store' references unknown store"`
- Neither agent is persisted — validation runs before config is written to the database

---

## Scenario 5: SecretStore Values Redacted in Task Logs

### Preconditions
- A SecretStore `redact-test` exists with `SECRET_TOKEN=super-secret-value-xyz`
- An agent references the SecretStore via `fromRef`
- Runner redaction is active

### Goal
Verify that SecretStore values are collected by `collect_sensitive_values()` and redacted in task output logs. EnvStore values should NOT be redacted.

### Steps
1. Apply stores:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: EnvStore
   metadata:
     name: public-config
   spec:
     data:
       PUBLIC_VAR: "visible-in-logs"
   ---
   apiVersion: orchestrator.dev/v2
   kind: SecretStore
   metadata:
     name: redact-test
   spec:
     data:
       SECRET_TOKEN: "super-secret-value-xyz"
   ```
2. Apply an agent that echoes both values:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: redact-agent
   spec:
     command: echo "public=${PUBLIC_VAR} secret=${SECRET_TOKEN}"
     env:
       - fromRef: public-config
       - fromRef: redact-test
   ```
3. Execute a task and inspect the captured stdout log

### Expected
- Log output contains `visible-in-logs` (EnvStore values are not redacted)
- Log output contains `[REDACTED]` in place of `super-secret-value-xyz`
- The literal string `super-secret-value-xyz` does NOT appear anywhere in task logs

---

## General Scenario: Override Precedence — Later Entries Win

### Steps
1. Via unit test or manifest, configure an agent with overlapping env entries:
   ```yaml
   env:
     - fromRef: store-a        # contains KEY=from-a
     - name: KEY
       value: "direct-override"
   ```
2. Resolve the agent env

### Expected
- `KEY` resolves to `direct-override` (later entry overrides earlier)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Agent with direct value env entry | ☐ | | | |
| 2 | Agent with fromRef importing all store keys | ☐ | | | |
| 3 | Agent with refValue importing single key with rename | ☐ | | | |
| 4 | Config validation rejects missing store references | ☐ | | | |
| 5 | SecretStore values redacted in task logs | ☐ | | | |
| G | Override precedence — later entries win | ☐ | | | |
