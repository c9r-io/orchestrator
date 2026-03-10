# Manifest Fixtures

kubectl-style fixtures for `orchestrator apply -f`.

## Layout

- `bundles/*.yaml`: multi-document manifests for end-to-end QA scenarios.

## Usage

```bash
orchestrator init
orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml --project qa-demo
```

These fixtures replace legacy flat-config bootstrap flows.

## Isolation Rules

- Manifest fixtures are additive setup artifacts. They are not a substitute for deleting or resetting the whole orchestrator control-plane state.
- Do not pair fixture application with database file deletion during routine QA runs.
- Prefer isolated QA projects: delete the project (`orchestrator delete project/<project> --force`) and then apply the fixture needed by that scenario with `apply --project`.
- Bundle fixtures must not be used to hijack unrelated default workspace/workflow expectations for other active runs.

## CLI Probe Fixtures

For Phase 2 CLI regression work, use the dedicated probe bundle:

```bash
orchestrator apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml --project qa-probe
```

It provides fixed workspaces and workflows for:

- task create target resolution
- runtime control (`task info`, `task logs`, `task watch`)
- low-output detection
- trace verbose scope verification

## Sandbox Execution Fixtures

For step-level execution-profile and sandbox QA, use the dedicated bundle:

```bash
orchestrator apply -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml --project qa-sandbox
```

It provides fixed resources for:

- execution-profile routing and host-default compatibility
- sandbox write allow/deny boundaries
- sandbox file-descriptor limit enforcement
- sandbox network deny and unsupported allowlist checks

## Self-Referential Probe Fixtures

For self-referential runtime diagnostics, use the dedicated probe bundle:

```bash
orchestrator apply -f fixtures/manifests/bundles/self-referential-probe-fixtures.yaml
```

It provides a self-referential workspace plus official probe workflows for:

- self-referential runtime control validation
- self-referential low-output detection
- self-referential trace/anomaly verification

These scenarios run directly with `task create/start/watch/trace`; they do not
use `apply --project`, which intentionally forces `self_referential: false`.
