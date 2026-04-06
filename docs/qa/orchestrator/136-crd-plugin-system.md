---
self_referential_safe: true
---

# QA: CRD Plugin System

Verifies the CRD plugin system core: plugin definitions, validation, interceptor/transformer execution with sandbox context, cron scheduling, built-in tool library.

## Scenario 1: CRD with plugins deserializes correctly

**Steps:**
1. Parse YAML with `plugins` field:
   ```yaml
   kind: CustomResourceDefinition
   metadata:
     name: slackintegrations.integrations.orchestrator.dev
   spec:
     kind: SlackIntegration
     plural: slackintegrations
     group: integrations.orchestrator.dev
     versions:
       - name: v1
         schema: { type: object }
     plugins:
       - name: verify-sig
         type: interceptor
         phase: webhook.authenticate
         command: "scripts/verify.sh"
         timeout: 3
       - name: rotate
         type: cron
         schedule: "0 0 * * *"
         command: "scripts/rotate.sh"
         timezone: "Asia/Taipei"
   ```
2. Parse YAML with per-plugin `execution_profile`:
   ```yaml
   plugins:
     - name: sandboxed-verify
       type: interceptor
       phase: webhook.authenticate
       command: "scripts/verify.sh"
       execution_profile:
         mode: sandbox
         network_mode: deny
   ```

**Expected:** Deserialization succeeds; plugins list has correct fields; `execution_profile` is `None` when absent, `Some(...)` when present.

**Verification:** `cargo test --lib -p orchestrator-config -- crd`

## Scenario 2: Plugin validation rejects invalid definitions

**Steps:**
1. Apply CRD with duplicate plugin names → rejected
2. Apply CRD with unknown plugin type (e.g., `type: unknown`) → rejected
3. Apply CRD with interceptor missing `phase` → rejected
4. Apply CRD with cron plugin missing `schedule` → rejected
5. Apply CRD with cron plugin with invalid cron expression → rejected
6. Apply CRD with empty command → rejected

**Expected:** Each case returns a validation error with a clear message.

**Verification:** `cargo test --lib -p agent-orchestrator -- crd::validate`

## Scenario 3: Interceptor plugin execution via PluginExecutionContext

**Steps:**
1. Create interceptor plugin with `command: "true"` → exit 0
2. Execute with `PluginExecutionContext` (Audit-mode policy, default runner) → returns Ok
3. Create interceptor plugin with `command: "exit 1"` → exit 1
4. Execute → returns Err with "rejected request"
5. Create interceptor with `command: "sleep 60"` and `timeout: 1`
6. Execute → returns Err with "timed out", entire process group killed

**Expected:** Accept/reject/timeout behavior matches exit code and timeout. All execution goes through `PluginExecutionContext` with sandbox-aware command building.

**Verification:** `cargo test --lib -p agent-orchestrator -- crd::plugins`

## Scenario 4: Transformer plugin execution

**Steps:**
1. Create transformer that wraps input JSON
2. Execute with payload via `PluginExecutionContext` → returns transformed JSON
3. Create transformer that outputs invalid JSON
4. Execute → returns Err with "invalid JSON"

**Expected:** Valid JSON output accepted; invalid output rejected.

**Verification:** `cargo test --lib -p agent-orchestrator -- crd::plugins::tests::transformer`

## Scenario 5: Cron plugin scheduling and webhook handler integration

**Steps:**
1. `rg "CronEntryKind::CrdPlugin" core/src/trigger_engine.rs` — verify CRD entries use plugin kind
2. `rg "PluginExecutionContext" core/src/trigger_engine.rs` — verify context is constructed before execution
3. `rg "PluginExecutionContext" crates/daemon/src/webhook.rs` — verify interceptor and transformer use context

**Expected:**
- Trigger engine constructs `PluginExecutionContext` with `self.state.plugin_policy` and fires cron plugins through it
- Webhook handler constructs `PluginExecutionContext` for both interceptor and transformer phases

**Verification:** Code inspection + `cargo test --lib -p agent-orchestrator -- trigger_engine`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | CRD plugin deserialization + execution_profile | ☐ | | | |
| 2 | Plugin validation rejects invalid definitions | ☐ | | | |
| 3 | Interceptor execution via PluginExecutionContext | ☐ | | | |
| 4 | Transformer execution | ☐ | | | |
| 5 | Cron scheduling + webhook handler integration | ☐ | | | |
