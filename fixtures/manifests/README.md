# Manifest Fixtures

kubectl-style fixtures for `orchestrator apply -f`.

## Layout

- `bundles/*.yaml`: multi-document manifests for end-to-end QA scenarios.

## Usage

```bash
orchestrator init
orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml
```

These fixtures replace legacy flat-config bootstrap flows.

## Isolation Rules

- Manifest fixtures are additive setup artifacts. They are not a substitute for deleting or resetting the whole orchestrator control-plane state.
- Do not pair fixture application with `db reset --include-config` or `db reset --force --include-config` during routine QA runs.
- Prefer isolated QA projects: recreate the project-local scaffold (`qa project reset`, remove `workspace/<project>`, then `qa project create`) and then apply the fixture needed by that scenario.
- Bundle fixtures must not be used to hijack unrelated default workspace/workflow expectations for other active runs.

## CLI Probe Fixtures

For Phase 2 CLI regression work, use the dedicated probe bundle:

```bash
orchestrator apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml
```

It provides fixed workspaces and workflows for:

- task create target resolution
- runtime control (`task info`, `task logs`, `task watch`)
- low-output detection
- trace verbose scope verification

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
use `qa project create`, which intentionally forces `self_referential: false`.
