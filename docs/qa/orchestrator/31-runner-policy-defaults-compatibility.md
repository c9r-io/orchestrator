# Orchestrator - Runner Policy Defaults and Compatibility

**Module**: orchestrator
**Scope**: Validate secure-by-default runner policy initialization and backward-compatible policy parsing
**Scenarios**: 3
**Priority**: High

---

## Background

This document covers the compatibility-focused policy checks split from `docs/qa/orchestrator/21-runner-security-observability.md` to preserve the repository max-scenario rule:

- Default initialization should remain `allowlist` (secure-by-default)
- Explicit `unsafe` mode must continue to work when intentionally selected
- Legacy `policy: legacy` manifests must remain backward-compatible and normalize to `unsafe`

Entry point: `./scripts/orchestrator.sh`

---

## Scenario 1: Default Initialization Produces Allowlist Policy

### Preconditions

- CLI built from latest source.
- No prior configuration in DB (clean state).

### Goal

Ensure that default initialization (without explicit RuntimePolicy in the manifest) produces an `allowlist` runner policy with populated shell/arg lists.

### Steps

1. Reset and initialize with a manifest that has no RuntimePolicy:
   ```bash
   ./scripts/orchestrator.sh db reset -f --include-config --include-history
   ./scripts/orchestrator.sh init -f
   ./scripts/orchestrator.sh apply -f docs/workflow/self-bootstrap.yaml
   ```
2. Export current configuration:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/default-policy-export.yaml
   cat /tmp/default-policy-export.yaml | grep -A5 'policy:'
   ```

### Expected

- Exported YAML contains `policy: allowlist`.
- `allowed_shells` list is non-empty (contains at least `/bin/bash`).
- `allowed_shell_args` list is non-empty (contains at least `-lc`).

---

## Scenario 2: Explicit Unsafe Mode Remains Functional

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure that explicitly setting `policy: unsafe` is accepted, applied, and the task can execute under unsafe mode.

### Steps

1. Apply config with explicit unsafe policy:
   ```bash
   ./scripts/orchestrator.sh db reset --force
   ./scripts/orchestrator.sh init --force
   cat > /tmp/runner-unsafe-explicit.yaml << 'YAML'
   runner:
     policy: unsafe
     executor: shell
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: .
       qa_targets: [docs/qa]
       ticket_dir: docs/ticket
   agents:
     mock:
       metadata:
         name: mock
       capabilities: [qa]
       templates:
         qa: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[]}'"
   workflows:
     qa_only:
       steps:
         - id: qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
         guard:
           enabled: false
           stop_when_no_unresolved: true
       finalize:
         rules: []
   YAML
   ./scripts/orchestrator.sh apply -f /tmp/runner-unsafe-explicit.yaml
   ```
2. Verify policy applied:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/unsafe-export.yaml
   cat /tmp/unsafe-export.yaml | grep 'policy:'
   ```
3. Create and run task:
   ```bash
   TASK_ID=$(./scripts/orchestrator.sh task create --project default --name "unsafe-mode-test" --goal "unsafe mode" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/orchestrator.sh task start "${TASK_ID}" || true
   ./scripts/orchestrator.sh task info "${TASK_ID}" -o json
   ```

### Expected

- Exported policy is `unsafe`.
- Task executes successfully under unsafe mode (no shell/arg allowlist enforcement).

---

## Scenario 3: Legacy Alias Backward Compatibility

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure that `policy: legacy` in YAML manifests is accepted as a backward-compatible alias for `unsafe` and correctly applied.

### Steps

1. Apply config using the legacy alias:
   ```bash
   ./scripts/orchestrator.sh db reset --force
   ./scripts/orchestrator.sh init --force
   cat > /tmp/runner-legacy-alias.yaml << 'YAML'
   runner:
     policy: legacy
     executor: shell
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: .
       qa_targets: [docs/qa]
       ticket_dir: docs/ticket
   agents:
     mock:
       metadata:
         name: mock
       capabilities: [qa]
       templates:
         qa: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[]}'"
   workflows:
     qa_only:
       steps:
         - id: qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
         guard:
           enabled: false
           stop_when_no_unresolved: true
       finalize:
         rules: []
   YAML
   ./scripts/orchestrator.sh apply -f /tmp/runner-legacy-alias.yaml
   ```
2. Verify the policy is normalized to `unsafe` on re-export:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/legacy-alias-export.yaml
   cat /tmp/legacy-alias-export.yaml | grep 'policy:'
   ```

### Expected

- `apply` succeeds without errors.
- Re-exported manifest shows `policy: unsafe` (normalized from `legacy` input).

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Default Initialization Produces Allowlist Policy | ✅ | 2026-03-02 | cursor | init + self-bootstrap.yaml (no RuntimePolicy) -> export shows `policy: allowlist` with populated lists |
| 2 | Explicit Unsafe Mode Remains Functional | ✅ | 2026-03-02 | cursor | `policy: unsafe` apply succeeds, export shows `unsafe`, task executes normally |
| 3 | Legacy Alias Backward Compatibility | ✅ | 2026-03-02 | cursor | `policy: legacy` apply succeeds, re-export normalized to `policy: unsafe` |
