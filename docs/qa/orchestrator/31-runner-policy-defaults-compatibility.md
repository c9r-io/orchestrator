# Orchestrator - Runner Policy Defaults and Compatibility

**Module**: orchestrator
**Scope**: Validate backward-compatible policy parsing for explicit unsafe and legacy alias modes
**Scenarios**: 2
**Priority**: High

---

## Background

This document covers the compatibility-focused policy checks split from `docs/qa/orchestrator/21-runner-security-observability.md` to preserve the repository max-scenario rule:

- Explicit `unsafe` mode must continue to work when intentionally selected
- Legacy `policy: legacy` manifests must remain backward-compatible and normalize to `unsafe`

Entry point: `./scripts/run-cli.sh`

---

## Scenario 1: Explicit Unsafe Mode Remains Functional

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure that explicitly setting `policy: unsafe` is accepted, applied, and the task can execute under unsafe mode.

### Steps

1. Apply config with explicit unsafe policy:
   ```bash
   QA_PROJECT="qa-unsafe-explicit"
   ./scripts/run-cli.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/run-cli.sh qa project create "${QA_PROJECT}" --force
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
       ticket_dir: fixtures/ticket
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
   ./scripts/run-cli.sh apply --project "${QA_PROJECT}" -f /tmp/runner-unsafe-explicit.yaml
   ```
2. Verify policy applied:
   ```bash
   ./scripts/run-cli.sh manifest export -f /tmp/unsafe-export.yaml
   cat /tmp/unsafe-export.yaml | grep 'policy:'
   ```
3. Create and run task:
   ```bash
   TASK_ID=$(./scripts/run-cli.sh task create --project "${QA_PROJECT}" --name "unsafe-mode-test" --goal "unsafe mode" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/run-cli.sh task start "${TASK_ID}" || true
   ./scripts/run-cli.sh task info "${TASK_ID}" -o json
   ```

### Expected

- Exported policy is `unsafe`.
- Task executes successfully under unsafe mode (no shell/arg allowlist enforcement).

---

## Scenario 2: Legacy Alias Backward Compatibility

### Preconditions

- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure that `policy: legacy` in YAML manifests is accepted as a backward-compatible alias for `unsafe` and correctly applied.

### Steps

1. Apply config using the legacy alias:
   ```bash
   QA_PROJECT="qa-legacy-alias"
   ./scripts/run-cli.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/run-cli.sh qa project create "${QA_PROJECT}" --force
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
       ticket_dir: fixtures/ticket
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
   ./scripts/run-cli.sh apply --project "${QA_PROJECT}" -f /tmp/runner-legacy-alias.yaml
   ```
2. Verify the policy is normalized to `unsafe` on re-export:
   ```bash
   ./scripts/run-cli.sh manifest export -f /tmp/legacy-alias-export.yaml
   cat /tmp/legacy-alias-export.yaml | grep 'policy:'
   ```

### Expected

- `apply` succeeds without errors.
- Re-exported manifest shows `policy: unsafe` (normalized from `legacy` input).

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Explicit Unsafe Mode Remains Functional | ☐ | | | |
| 2 | Legacy Alias Backward Compatibility | ☐ | | | |
