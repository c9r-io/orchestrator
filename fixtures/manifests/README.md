# Manifest Fixtures

kubectl-style fixtures for `orchestrator apply -f`.

## Layout

- `bundles/*.yaml`: multi-document manifests for end-to-end QA scenarios.

## Usage

```bash
./scripts/orchestrator.sh init
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/output-formats.yaml
```

These fixtures replace legacy flat-config bootstrap flows.

## CLI Probe Fixtures

For Phase 2 CLI regression work, use the dedicated probe bundle:

```bash
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml
```

It provides fixed workspaces and workflows for:

- task create target resolution
- runtime control (`task info`, `task logs`, `task watch`)
- low-output detection
- trace verbose scope verification

## Self-Referential Probe Fixtures

For self-referential runtime diagnostics, use the dedicated probe bundle:

```bash
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/self-referential-probe-fixtures.yaml
```

It provides a self-referential workspace plus official probe workflows for:

- self-referential runtime control validation
- self-referential low-output detection
- self-referential trace/anomaly verification

These scenarios run directly with `task create/start/watch/trace`; they do not
use `qa project create`, which intentionally forces `self_referential: false`.
