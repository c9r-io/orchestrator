# CLI `get agent/<name> -o yaml` returns empty metadata name

**Status**: FAILED
**Module**: orchestrator
**Related QA Doc**: `docs/qa/orchestrator/42-crd-unified-resource-store.md`
**Priority**: Low
**Date**: 2026-03-05

---

## Test Content

During CRD unification regression testing, verified that `get agent/<name> -o yaml` returns correct resource representation including metadata.

## Expected Result

```yaml
metadata:
  name: regression-agent-a
  version: null
  cost: null
capabilities:
- plan
command: echo {prompt}
```

The `metadata.name` field should contain the actual resource name.

## Reproduction Steps

```bash
cat <<'EOF' | ./scripts/orchestrator.sh apply -f -
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: test-agent-meta
spec:
  command: "echo {prompt}"
  capabilities: ["plan"]
EOF

./scripts/orchestrator.sh get agent/test-agent-meta -o yaml
```

## Actual Result

```yaml
metadata:
  name: ''
  version: null
  cost: null
capabilities:
- plan
command: echo {prompt}
selection:
  strategy: cost_based
  weights: null
```

The `metadata.name` field is empty string `''` instead of `test-agent-meta`.

## Root Cause Analysis

The `get agent/<name>` path in `cli_handler/resource.rs` calls `AgentResource::get_from()` which returns `OrchestratorResource { metadata, spec }`. The metadata is constructed from `metadata_from_store()` which reads from the ResourceStore's `CustomResource.metadata`. However, the YAML output serializes the `AgentSpec` (which has its own nested `metadata` field from `AgentSpec.metadata: AgentMetadata`), not the resource-level metadata. The `AgentMetadata` struct has `name`, `version`, `cost` fields — and the `name` field in `AgentMetadata` is not populated during `agent_config_to_spec()` conversion (it defaults to empty string).

This is a pre-existing display issue — the `name` in the YAML output comes from `AgentSpec.metadata.name` (the agent's self-declared name), not from the resource envelope's `metadata.name`. The resource name is only shown in the response header, not in the spec body.

## Suggested Fix

In `agent_config_to_spec()` or in the `get` display path, populate `AgentSpec.metadata.name` from the resource name, or change the output format to wrap the spec in a full resource envelope that includes the resource-level metadata.
