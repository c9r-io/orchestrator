# Manifest Fixtures

kubectl-style fixtures for `orchestrator apply -f`.

## Layout

- `bundles/*.yaml`: multi-document manifests for end-to-end QA scenarios.
- Some bundles are **not** meant to apply cleanly. `coordination-legacy-baseline.yaml` is
  read as text for a measurement and applied only to assert that the daemon still rejects it;
  `crd-test-invalid.yaml`, `s4-invalid-cel.yaml` and `qa105-s1-capture-wrong-level.yaml` are
  invalid on purpose. Each says so in its own header.

## Agent Fixtures Carry Typed Drivers

Every `kind: Agent` document in a tracked YAML file declares `spec.driver`
(`provider: shell, transport: cli` for script agents). The driverless form is
runtime-compatible but deprecated — apply emits
`[legacy_agent_command_deprecated]` — and fixtures are what people copy, so
the corpus must not teach it. `core/src/fixture_driverless_tests.rs` enforces
this over `git ls-files '*.yaml'`; a new driverless Agent document fails the
Rust test suite.

The only exception is a document that a live gate *needs* driverless, because
it asserts the legacy warning or the promotion behavior on that exact
document. Such a document carries a machine-parseable comment inside its own
chunk (right below the `---` separator):

```yaml
---
# fixture-driverless-exempt: <which gate asserts what on this document>
apiVersion: orchestrator.dev/v2
kind: Agent
```

The reason must be non-empty, and the exemption is checked in both
directions: a document that gains `driver:` while keeping the comment fails
too, so stale exemptions cannot linger.

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
