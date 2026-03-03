# Orchestrator - Prompt Delivery Abstraction

**Module**: orchestrator
**Scope**: PromptDelivery enum configuration, serde behavior, delivery dispatch, preflight validation
**Scenarios**: 5
**Priority**: High

---

## Background

Agents can now declare a `promptDelivery` mode in their spec to control how the rendered prompt reaches the spawned process. Four modes exist:

- **arg** (default): Legacy `{prompt}` substitution in the shell command string
- **stdin**: Prompt written to the child process stdin, then stdin closed (EOF)
- **file**: Prompt written to a temp file, `{prompt_file}` placeholder replaced in command
- **env**: Prompt injected as the `ORCH_PROMPT` environment variable

The mode is threaded through agent selection → phase runner → spawn.

---

## Scenario 1: Default Prompt Delivery Is Arg

### Preconditions
- Orchestrator binary is built
- A workspace and workflow exist in config

### Goal
Verify that agents without an explicit `promptDelivery` field default to `arg` mode and receive the prompt via `{prompt}` substitution as before.

### Steps
1. Apply a manifest with no `promptDelivery` field:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: default-delivery-agent
   spec:
     command: echo "{prompt}"
     capabilities: ["qa"]
   ```
2. Run:
   ```bash
   ./scripts/orchestrator.sh get agent default-delivery-agent -o yaml
   ```
3. Verify that the output does **not** contain a `promptDelivery` field (skipped when default).
4. Run:
   ```bash
   ./scripts/orchestrator.sh manifest export -o yaml | grep -A5 "default-delivery-agent"
   ```

### Expected
- Agent is created successfully
- YAML output omits `promptDelivery` entirely (serialization skip when default `arg`)
- The agent command template retains `{prompt}` placeholder

---

## Scenario 2: Explicit Stdin Delivery Mode

### Preconditions
- Orchestrator binary is built
- A workspace and workflow exist in config

### Goal
Verify that an agent with `promptDelivery: stdin` is stored correctly, round-trips through export, and the selection function returns `Stdin` delivery mode.

### Steps
1. Apply a manifest with `promptDelivery: stdin`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: stdin-agent
   spec:
     command: cat
     capabilities: ["qa"]
     promptDelivery: stdin
   ```
2. Run:
   ```bash
   ./scripts/orchestrator.sh get agent stdin-agent -o yaml
   ```
3. Verify the output contains `promptDelivery: stdin`.
4. Export and re-apply:
   ```bash
   ./scripts/orchestrator.sh manifest export -o yaml > /tmp/qa-pd-export.yaml
   grep "promptDelivery" /tmp/qa-pd-export.yaml
   ```

### Expected
- Agent stored with `promptDelivery: stdin`
- YAML export includes `promptDelivery: stdin` for this agent
- Round-trip preserves the delivery mode

---

## Scenario 3: File Delivery Mode with Prompt File Placeholder

### Preconditions
- Orchestrator binary is built
- A workspace and workflow exist in config

### Goal
Verify that an agent with `promptDelivery: file` is stored correctly and preflight check does not warn when command contains `{prompt_file}`.

### Steps
1. Apply a manifest:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: file-agent
   spec:
     command: cat {prompt_file}
     capabilities: ["qa"]
     promptDelivery: file
   ```
2. Run:
   ```bash
   ./scripts/orchestrator.sh get agent file-agent -o yaml
   ```
3. Run preflight check:
   ```bash
   ./scripts/orchestrator.sh check all 2>&1
   ```

### Expected
- Agent stored with `promptDelivery: file`
- Command template contains `{prompt_file}`
- Preflight check produces no warnings for this agent's delivery configuration

---

## Scenario 4: Preflight Warns on Misconfigured Delivery

### Preconditions
- Orchestrator binary is built
- A workspace and workflow exist in config

### Goal
Verify that the preflight check system warns on misconfigured prompt delivery combinations.

### Steps
1. Apply a manifest with stdin delivery but `{prompt}` in command:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: misconfig-stdin-agent
   spec:
     command: echo "{prompt}"
     capabilities: ["qa"]
     promptDelivery: stdin
   ```
2. Apply a manifest with file delivery but no `{prompt_file}`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: misconfig-file-agent
   spec:
     command: echo "no-placeholder"
     capabilities: ["qa"]
     promptDelivery: file
   ```
3. Run preflight check:
   ```bash
   ./scripts/orchestrator.sh check all 2>&1
   ```

### Expected
- Warning for `misconfig-stdin-agent`: prompt delivery is `stdin` but command contains `{prompt}` placeholder (placeholder will be ignored)
- Warning for `misconfig-file-agent`: prompt delivery is `file` but command does not contain `{prompt_file}` placeholder
- Both warnings are informational (check does not fail, just warns)

---

## Scenario 5: Env Delivery Mode Serde Round-Trip

### Preconditions
- Orchestrator binary is built
- A workspace and workflow exist in config

### Goal
Verify that `promptDelivery: env` is correctly serialized, deserialized, and round-trips through export/apply.

### Steps
1. Apply a manifest:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: env-agent
   spec:
     command: printenv ORCH_PROMPT
     capabilities: ["qa"]
     promptDelivery: env
   ```
2. Run:
   ```bash
   ./scripts/orchestrator.sh get agent env-agent -o yaml
   ```
3. Verify `promptDelivery: env` in output.
4. Delete and re-apply from export:
   ```bash
   ./scripts/orchestrator.sh manifest export -o yaml > /tmp/qa-pd-env.yaml
   ./scripts/orchestrator.sh delete agent env-agent
   ./scripts/orchestrator.sh apply -f /tmp/qa-pd-env.yaml
   ./scripts/orchestrator.sh get agent env-agent -o yaml
   ```

### Expected
- Agent created with `promptDelivery: env`
- Export YAML contains `promptDelivery: env`
- After delete + re-apply from export, agent still has `promptDelivery: env`

---

## General Scenario: Unit Test Coverage Verification

### Steps
1. Run unit tests:
   ```bash
   cd core && cargo test --lib 2>&1 | tail -5
   ```
2. Run specific prompt-delivery-related tests:
   ```bash
   cd core && cargo test prompt_delivery 2>&1
   cd core && cargo test check_prompt_delivery 2>&1
   ```

### Expected
- All unit tests pass
- Tests for `prompt_delivery_default_is_arg`, `prompt_delivery_serde_roundtrip`, `prompt_delivery_skip_serializing_default` pass
- Tests for `check_prompt_delivery_warns_stdin_with_placeholder`, `check_prompt_delivery_warns_file_without_prompt_file`, `check_prompt_delivery_warns_file_with_arg_placeholder` pass

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Default prompt delivery is arg | ☐ | | | |
| 2 | Explicit stdin delivery mode | ☐ | | | |
| 3 | File delivery mode with prompt_file | ☐ | | | |
| 4 | Preflight warns on misconfigured delivery | ☐ | | | |
| 5 | Env delivery mode serde round-trip | ☐ | | | |
| G | Unit test coverage verification | ☐ | | | |
