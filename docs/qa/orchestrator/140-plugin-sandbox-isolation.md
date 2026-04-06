---
self_referential_safe: true
---

# QA-140: Plugin Sandbox Isolation

**Module**: Orchestrator
**Scope**: CRD plugin execution sandbox isolation — sandboxed command building, runtime policy re-check (TOCTOU defense), environment sanitization, execution profile precedence, enhanced audit records.
**Scenarios**: 5
**Priority**: High

---

## Background

CRD plugins (interceptor, transformer, cron) previously executed via raw `Command::new("sh").arg("-c")` with no sandbox isolation. This change routes plugin execution through the existing `build_command_for_profile` sandbox infrastructure (macOS Seatbelt / Linux Native), adds runtime policy re-check to close the TOCTOU gap between CRD apply and execution, sanitizes sensitive environment variables, and records sandbox profile + policy verdict in audit logs.

Key files:
- `core/src/crd/plugins.rs` — `PluginExecutionContext`, sandboxed execution
- `crates/orchestrator-config/src/plugin_policy.rs` — `execution_profile`, `env_deny_prefixes`
- `crates/orchestrator-config/src/crd_types.rs` — `CrdPlugin.execution_profile`
- `core/src/crd/validate.rs` — execution profile validation at apply time
- `core/src/db.rs` — `PluginAuditRecord` with `sandbox_profile`, `policy_verdict`

---

## Database Schema Reference

### Table: plugin_audit (post-migration v25)
| Column | Type | Notes |
|--------|------|-------|
| sandbox_profile | TEXT | Nullable; name of sandbox profile applied at runtime |
| policy_verdict | TEXT | Nullable; `allowed`, `denied`, or `audit_warning` at runtime |

---

## Scenario 1: Runtime policy denial blocks plugin execution (TOCTOU defense)

### Preconditions
- Plugin policy set to `Deny` mode at runtime (simulating policy change after CRD was applied)

### Goal
Verify that plugin execution re-checks policy before spawning the command, blocking execution even if the CRD was admitted under a previous policy.

### Steps
1. Create interceptor plugin with `command: "scripts/verify.sh"`
2. Construct `PluginExecutionContext` with `PluginPolicyMode::Deny`
3. Call `execute_interceptor()` with the plugin and deny-mode context

### Expected
- Execution fails with error containing "denied at runtime by plugin policy"
- No child process is spawned (denial happens before `build_command_for_profile`)

### Verification
```bash
cargo test -p agent-orchestrator -- crd::plugins::tests::runtime_policy_denial_blocks_execution
```

---

## Scenario 2: Execution profile resolution precedence

### Preconditions
- Default `PluginPolicy` (no `execution_profile` set)
- Plugin with explicit `execution_profile: { mode: sandbox }`

### Goal
Verify per-plugin execution profile overrides the policy-level default, and absent profiles fall back to Host mode.

### Steps
1. Create plugin with `execution_profile: Some(ExecutionProfileConfig { mode: Sandbox, .. })`
2. Resolve profile via `resolve_plugin_profile()` → mode is `Sandbox`
3. Create plugin with `execution_profile: None`
4. Resolve profile with default policy (no `execution_profile`) → mode is `Host`
5. Set policy `execution_profile: Some(ExecutionProfileConfig { mode: Sandbox, .. })`, plugin has `None`
6. Resolve → mode is `Sandbox` (inherits from policy)

### Expected
- Per-plugin override takes precedence over policy default
- Policy default takes precedence over built-in Host fallback
- When both are absent, Host mode is used (backward compatible)

### Verification
```bash
cargo test -p agent-orchestrator -- crd::plugins::tests::profile_resolution_prefers_plugin_override
```

---

## Scenario 3: Environment sanitization strips sensitive variables

### Preconditions
- `PluginPolicy` with default `env_deny_prefixes` (builtin: `AWS_`, `SSH_`, `GCP_`, `GCLOUD_`, `AZURE_`, `KUBECONFIG`, `GOOGLE_APPLICATION_CREDENTIALS`, `GITHUB_TOKEN`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`)

### Goal
Verify that `build_plugin_command()` removes environment variables matching deny prefixes before plugin execution.

### Steps
1. Inspect `build_plugin_command()` in `core/src/crd/plugins.rs`:
   ```bash
   rg "env_remove" core/src/crd/plugins.rs
   rg "effective_env_deny_prefixes" core/src/crd/plugins.rs
   ```
2. Verify builtin deny prefixes in `plugin_policy.rs`:
   ```bash
   rg "BUILTIN_ENV_DENY_PREFIXES" crates/orchestrator-config/src/plugin_policy.rs
   ```
3. Verify custom prefixes override builtins:
   - Create policy with `env_deny_prefixes: ["CUSTOM_"]`
   - Call `effective_env_deny_prefixes()` → returns `["CUSTOM_"]` (builtins NOT included)

### Expected
- `build_plugin_command` iterates `std::env::vars()` and calls `cmd.env_remove()` for each matching prefix
- Builtin prefixes cover: `AWS_`, `SSH_`, `GCP_`, `GCLOUD_`, `AZURE_`, `KUBECONFIG`, `GOOGLE_APPLICATION_CREDENTIALS`, `GITHUB_TOKEN`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`
- Custom `env_deny_prefixes` fully replaces builtins (same pattern as `denied_patterns`)

### Verification
```bash
rg "BUILTIN_ENV_DENY_PREFIXES" crates/orchestrator-config/src/plugin_policy.rs
rg "env_remove" core/src/crd/plugins.rs
```

---

## Scenario 4: Sandbox command building routes through build_command_for_profile

### Preconditions
- Plugin with default execution profile (Host mode)

### Goal
Verify that plugin execution uses `build_command_for_profile` instead of raw `Command::new("sh")`.

### Steps
1. Verify NO direct `Command::new("sh")` in plugin execution path:
   ```bash
   rg 'Command::new\("sh"\)' core/src/crd/plugins.rs
   ```
   → Expected: No matches
2. Verify `build_command_for_profile` is called:
   ```bash
   rg "build_command_for_profile" core/src/crd/plugins.rs
   ```
   → Expected: 1 match in `build_plugin_command()`
3. Run existing plugin tests to verify Host-mode backward compatibility:
   ```bash
   cargo test -p agent-orchestrator -- crd::plugins::tests::interceptor_accepts_on_exit_zero
   cargo test -p agent-orchestrator -- crd::plugins::tests::transformer_returns_modified_json
   cargo test -p agent-orchestrator -- crd::plugins::tests::cron_plugin_success
   ```

### Expected
- No raw `sh -c` in plugins.rs execution path
- All plugin commands go through sandbox infrastructure
- Host mode (default) behaves identically to previous raw shell execution

---

## Scenario 5: Enhanced audit records include sandbox_profile and policy_verdict

### Preconditions
- Database with migration v25 applied (m0025_plugin_audit_sandbox_columns)

### Goal
Verify that runtime plugin execution logs the sandbox profile name and policy verdict in the `plugin_audit` table.

### Steps
1. Verify migration count:
   ```bash
   cargo test -p agent-orchestrator -- migration::tests::all_migrations_count_matches_expected
   ```
   Expected: 25 migrations
2. Verify audit record construction in `audit_plugin_execution()`:
   ```bash
   rg "sandbox_profile:" core/src/crd/plugins.rs
   rg "policy_verdict:" core/src/crd/plugins.rs
   ```
   → Expected: Both fields populated with actual values (not hardcoded "allowed")
3. Verify DB insert includes new columns:
   ```bash
   rg "sandbox_profile" core/src/db.rs
   ```
   → Expected: Field in struct + INSERT statement
4. Verify migration step exists:
   ```bash
   rg "m0025_plugin_audit_sandbox_columns" core/src/persistence/migration_steps.rs
   ```

### Expected
- Migration v25 adds `sandbox_profile TEXT` and `policy_verdict TEXT` columns to `plugin_audit`
- Runtime execution audit records include: `sandbox_profile="plugin:{name}"`, `policy_verdict="allowed"|"denied"|"audit_warning"`
- Apply-time audit records retain `sandbox_profile=NULL`, `policy_verdict=NULL` (backward compatible)

### Expected Data State
```sql
-- After plugin execution:
SELECT sandbox_profile, policy_verdict FROM plugin_audit
WHERE action = 'plugin_execute' ORDER BY created_at DESC LIMIT 1;
-- Expected: sandbox_profile = 'plugin:{name}', policy_verdict = 'allowed'
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Runtime policy denial (TOCTOU defense) | ☐ | | | |
| 2 | Execution profile resolution precedence | ☐ | | | |
| 3 | Environment sanitization | ☐ | | | |
| 4 | Sandbox command building | ☐ | | | |
| 5 | Enhanced audit records + migration v25 | ☐ | | | |
