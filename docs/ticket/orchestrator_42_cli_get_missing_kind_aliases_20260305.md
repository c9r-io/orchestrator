# CLI `get` missing kind aliases for StepTemplate, EnvStore, SecretStore

**Status**: FAILED
**Module**: orchestrator
**Related QA Doc**: `docs/qa/orchestrator/42-crd-unified-resource-store.md`
**Priority**: Medium
**Date**: 2026-03-05

---

## Test Content

During CRD unification regression testing, verified that all 9 builtin resource types can be accessed via CLI `get` commands.

## Expected Result

All builtin resource types should support:
- `get <kind>/<name> -o yaml` — retrieve single resource
- `get <plural>` — list resources of that kind

This should work for StepTemplate, EnvStore, and SecretStore, just as it works for Agent, Workspace, and Workflow.

## Reproduction Steps

```bash
# Apply resources
cat <<'EOF' | ./scripts/orchestrator.sh apply -f -
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: test-tmpl
spec:
  prompt: "Do QA"
  description: "Test template"
---
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: test-env
spec:
  data:
    KEY: "value"
---
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: test-secret
spec:
  data:
    SECRET: "val"
EOF

# These all fail:
./scripts/orchestrator.sh get steptemplate/test-tmpl -o yaml
./scripts/orchestrator.sh get envstore/test-env -o yaml
./scripts/orchestrator.sh get secretstore/test-secret -o yaml
./scripts/orchestrator.sh get steptemplates
./scripts/orchestrator.sh get envstores
./scripts/orchestrator.sh get secretstores
```

## Actual Result

All 6 commands return:
```
Error: unknown resource type: steptemplate (supported: ws/workspace, wf/workflow, agent, task, or CRD-defined types)
```
or:
```
Error: unknown list resource type: steptemplates (supported: workspaces, agents, workflows, or CRD-defined types)
```

Note: `apply` and `delete` (via `env-store/<name>`, `secret-store/<name>`, `step-template/<name>`) work correctly. Only `get` is missing the kind resolution for these types.

## Root Cause Analysis

The CLI `get` command in `cli_handler/resource.rs` only resolves a fixed set of kind aliases (workspace, agent, workflow, task) and CRD-defined types. The 6 builtin types that don't have explicit `get` branches (StepTemplate, EnvStore, SecretStore, Project, Defaults, RuntimePolicy) are not routed. Since the CRD unification registers these as builtin CRDs, they could potentially be resolved via the CRD lookup path, but the `get` dispatch doesn't fall through to it for builtin kinds.

## Suggested Fix

Extend the `get` kind resolution in `cli_handler/resource.rs` to:
1. Add explicit aliases for all 9 builtin types, or
2. Fall through to CRD-based kind resolution for any unrecognized kind (since builtin types are now registered as CRDs with short_names and plural forms)
