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
