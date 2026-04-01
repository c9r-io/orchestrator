---
self_referential_safe: true
---

# QA: CRD Plugin System

验证 CRD 插件系统的核心功能：插件定义、验证、拦截器/转换器执行、定时任务调度、内置工具库。

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

**Expected:** Deserialization succeeds; plugins list has 2 entries with correct fields.

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

## Scenario 3: Interceptor plugin execution

**Steps:**
1. Create interceptor plugin with `command: "true"` → exit 0
2. Execute with headers and body → returns Ok
3. Create interceptor plugin with `command: "exit 1"` → exit 1
4. Execute → returns Err with "rejected request"
5. Create interceptor with `command: "sleep 60"` and `timeout: 1`
6. Execute → returns Err with "timed out"

**Expected:** Accept/reject/timeout behavior matches exit code and timeout.

**Verification:** `cargo test --lib -p agent-orchestrator -- crd::plugins`

## Scenario 4: Transformer plugin execution

**Steps:**
1. Create transformer that wraps input JSON
2. Execute with payload → returns transformed JSON
3. Create transformer that outputs invalid JSON
4. Execute → returns Err with "invalid JSON"

**Expected:** Valid JSON output accepted; invalid output rejected.

**Verification:** `cargo test --lib -p agent-orchestrator -- crd::plugins::tests::transformer`

## Scenario 5: Cron plugin scheduling

**Steps:**
1. `rg "crd:" core/src/trigger_engine.rs` — verify CRD cron entries use `crd:{kind}:{name}` format
2. `rg "CronEntryKind::CrdPlugin" core/src/trigger_engine.rs` — verify CRD entries use plugin kind
3. `rg "execute_cron_plugin" core/src/trigger_engine.rs` — verify cron fire calls plugin executor

**Expected:** CRD cron plugins are collected in `build_cron_schedule()` and fired via `execute_cron_plugin()`.

**Verification:** Code inspection + `cargo test --lib -p agent-orchestrator -- trigger_engine`

## Scenario 6: Webhook handler CRD plugin injection

**Steps:**
1. `rg "crd_plugins" crates/daemon/src/webhook.rs` — verify CRD plugin resolution
2. `rg "PHASE_WEBHOOK_AUTHENTICATE" crates/daemon/src/webhook.rs` — verify interceptor execution
3. `rg "PHASE_WEBHOOK_TRANSFORM" crates/daemon/src/webhook.rs` — verify transformer execution

**Expected:** Webhook handler resolves CRD plugins via `crdRef`, executes interceptors before HMAC, executes transformers after body parsing.

**Verification:** Code inspection of `crates/daemon/src/webhook.rs`

## Scenario 7: Built-in tool — webhook-verify-hmac

**Steps:**
1. Compute HMAC-SHA256 of "test body" with secret "mysecret"
2. Run: `orchestrator tool webhook-verify-hmac --secret mysecret --body "test body" --signature <hex>`
3. Verify output: "valid", exit 0
4. Run with wrong signature → "invalid", exit 1

**Expected:** Correct HMAC verification with exit code semantics.

## Scenario 8: Built-in tool — payload-extract

**Steps:**
1. Echo `{"event":{"type":"message"}}` | `orchestrator tool payload-extract --path event.type`
2. Verify output: `message`
3. Echo `{"a":1}` | `orchestrator tool payload-extract --path b.c`
4. Verify output: "path 'b.c' not found", exit 1

**Expected:** Dot-path extraction from JSON stdin.

## Scenario 9: crdRef on trigger webhook config

**Steps:**
1. `rg "crd_ref" crates/orchestrator-config/src/config/trigger.rs` — field exists
2. `rg "crdRef" crates/orchestrator-config/src/cli_types.rs` — YAML rename works
3. Verify YAML parsing: trigger manifest with `webhook.crdRef: SlackIntegration` deserializes

**Expected:** `crdRef` is an optional field on webhook config, serialized as camelCase in YAML.

**Verification:** `cargo test --lib -p orchestrator-config`
