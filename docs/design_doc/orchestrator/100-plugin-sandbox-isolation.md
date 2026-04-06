# Orchestrator - Plugin Sandbox Isolation

**Module**: Orchestrator
**Status**: Approved
**Related Plan**: Plugin execution upgraded from policy-gated shell to policy-gated sandboxed execution, reusing the existing ExecutionProfile sandbox infrastructure
**Related QA**: `docs/qa/orchestrator/140-plugin-sandbox-isolation.md`, `docs/qa/orchestrator/136-crd-plugin-system.md`, `docs/qa/orchestrator/137-plugin-policy-governance.md`
**Created**: 2026-04-06
**Last Updated**: 2026-04-06

## Background

CRD plugins (interceptor, transformer, cron) executed shell commands via `Command::new("sh").arg("-c")` with zero isolation — no filesystem, network, resource, or environment sandboxing. Meanwhile, agent step commands went through a mature sandbox system (`build_command_for_profile` → macOS Seatbelt / Linux namespaces + iptables). The plugin policy only gated admission at CRD apply time (no runtime re-check), creating a TOCTOU window. Audit records always logged "allowed" regardless of actual verdict.

## Goals
- Route all plugin execution through the existing sandbox infrastructure
- Close the TOCTOU gap with runtime policy re-check before spawning
- Sanitize sensitive environment variables (AWS_, SSH_, API keys) before plugin execution
- Support per-plugin and policy-level execution profile configuration
- Enhance audit trail with actual sandbox profile and policy verdict

## Non-goals
- Container/WASM-based plugin isolation (future work)
- Unified CommandRunner trait merging step and plugin execution (future refactor)
- Modifying the existing sandbox backends (Seatbelt, Linux Native) themselves

## Scope
- In scope: Plugin execution sandbox integration, PluginPolicy schema extension, CrdPlugin schema extension, runtime policy re-check, env sanitization, audit enhancement, DB migration
- Out of scope: UI changes, new sandbox backend development, agent step execution changes

## Interfaces and Data

### Configuration Changes

**PluginPolicy** (`{data_dir}/plugin-policy.yaml`):
```yaml
execution_profile:        # Optional, default: Host mode
  mode: sandbox
  network_mode: deny
  fs_mode: inherit
  max_memory_mb: 256
  max_processes: 32
env_deny_prefixes:        # Optional, default: builtin list
  - "AWS_"
  - "CUSTOM_SECRET_"
```

**CrdPlugin** (per-plugin override in CRD YAML):
```yaml
plugins:
  - name: verify-sig
    type: interceptor
    phase: webhook.authenticate
    command: "scripts/verify.sh"
    execution_profile:
      mode: sandbox
      network_mode: deny
```

### Database Changes
- Table: `plugin_audit`
- New columns (migration v25): `sandbox_profile TEXT`, `policy_verdict TEXT`
- Both nullable for backward compatibility with existing records
- Migration: `m0025_plugin_audit_sandbox_columns`

## Key Design

1. **Reuse existing sandbox infrastructure** — `build_command_for_profile()` (made `pub`) and `ResolvedExecutionProfile` handle all sandbox backend selection. No parallel sandbox system.

2. **PluginExecutionContext** bundles `RunnerConfig`, `PluginPolicy`, and `db_path` to avoid parameter explosion across the three execution functions.

3. **Profile resolution precedence**: per-plugin `execution_profile` > policy-level `execution_profile` > Host mode (built-in default). This ensures backward compatibility — existing deployments with no execution_profile get exactly the same behavior.

4. **Runtime policy re-check** happens before `build_command_for_profile`, preventing execution if the policy changed to Deny after CRD admission.

5. **Environment sanitization** strips vars matching deny prefixes via `cmd.env_remove()` after command construction but before plugin-specific env injection. Builtin prefixes: `AWS_`, `SSH_`, `GCP_`, `GCLOUD_`, `AZURE_`, `KUBECONFIG`, `GOOGLE_APPLICATION_CREDENTIALS`, `GITHUB_TOKEN`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`.

## Alternatives and Tradeoffs
- **Option A: Move sandbox call to call sites only** — Webhook handler and trigger engine would own sandbox setup. Rejected: duplicates sandbox logic across call sites.
- **Option B: Pass context into plugin execution functions** — Chosen. Clean separation: call sites construct context, plugin functions consume it.
- **Option C: Inline sandbox config in plugins.rs** — Rejected: creates parallel sandbox system, violates DRY.

## Risks and Mitigations
- **Risk:** `build_command_for_profile` with Host mode behaves differently from raw `sh -c`
  - Mitigation: Host mode uses `RunnerConfig.shell` (default `/bin/bash -lc`), which is functionally equivalent. All existing tests pass unchanged.
- **Risk:** `env_remove` race with concurrent env mutations
  - Mitigation: `std::env::vars()` snapshot is taken once; plugin env is set after sanitization.

## Observability
- **Audit trail:** `plugin_audit.sandbox_profile` and `plugin_audit.policy_verdict` populated at runtime
- **Logs:** `tracing::warn` on audit-mode policy warnings at runtime (tagged with `plugin=` and `reason=`)
- **Metrics:** No new metrics; existing sandbox backend labels apply to plugin execution

## Operations / Release
- Config: `plugin-policy.yaml` gains optional `execution_profile` and `env_deny_prefixes` fields
- Migration: v25 adds nullable columns — forward and backward compatible
- Rollback: Columns are nullable; older code ignores them. Reverting code is safe without DB rollback.

## Test Plan
- Unit tests: 13 plugin tests (including 2 new: runtime denial, profile resolution)
- Unit tests: 31 CRD validation tests, 12 plugin policy tests, 50 runner tests
- Integration: Existing webhook and trigger engine tests verify context wiring
- Code inspection: `rg` commands verify absence of raw `Command::new("sh")` in plugins.rs

## QA Docs
- `docs/qa/orchestrator/140-plugin-sandbox-isolation.md` (new — 5 scenarios)
- `docs/qa/orchestrator/136-crd-plugin-system.md` (updated — execution_profile, PluginExecutionContext)
- `docs/qa/orchestrator/137-plugin-policy-governance.md` (updated — new fields, audit columns, migration count)

## Acceptance Criteria
- All plugin execution goes through `build_command_for_profile` (no raw `sh -c`)
- Runtime policy denial blocks execution before process spawn
- Environment sanitization strips builtin + custom deny prefixes
- Audit records contain `sandbox_profile` and `policy_verdict`
- Backward compatible: existing deployments with no execution_profile have identical behavior
- All 1805+ tests pass
