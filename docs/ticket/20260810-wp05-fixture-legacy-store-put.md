# wp05 fixture still authors the retired store_put post-action

- **Observed during**: FR-160 governance, Phase 3 batch D migration verification of `scripts/qa/test-wp05-integration.sh`
- **Severity**: medium (manual-runbook gate cannot pass its first apply; every layer of the WP05 integration matrix is unverified)
- **Symptom**: `Error: resource.apply: [legacy_pipeline_variables_removed] workflow 'wp05-store-spawn-parent' step 'plan' uses a store_put post-action; write from the step instead`
- **Status**: open

## Mechanism

FR-156 retired the pipeline-variable authoring surface and `64542cc7` made
apply reject it. `fixtures/manifests/bundles/wp05-store-spawn.yaml` (and, by
`rg -l 'store_put' fixtures/manifests/bundles/`, also
`qa50-store-io-test.yaml` — the fr156 parity bundle carries it deliberately as
its legacy baseline) still author the retired form, so the gate dies on its
first `apply`. This is §4.4 shape 9 landing on FR-156: the migration was
complete for the surfaces its ratchet derived, and a manual gate's fixture sat
outside that derivation.

Compounding it, the death is silent: `run_orch` swallows stderr unless
`--verbose`, and `set -e` ends the gate between the L1-A banner and any
assertion — no FAIL line, no summary. The diagnostic above is only visible in
a `--verbose` run. That is the truncated-run shape the gate's own FR-149
header describes, still reachable through this path.

## Classification evidence

- Pre-existing: the failing apply is a product-side rejection of an unchanged
  fixture; FR-160's diff touches neither. The error names the FR-156
  enforcement introduced in `64542cc7`.
- The FR-160 teardown is exercised: the daemon was up, the abort path ran
  `gate_daemon_stop`, residue after the run was 0/0/0.

## For ticket-fix

1. Migrate `wp05-store-spawn.yaml` (and audit `qa50-store-io-test.yaml`) to
   the step-level `orchestrator store put` form the diagnostic prescribes.
2. Give `run_orch` a failure mode that prints the swallowed stderr on
   non-zero exit even without `--verbose`, so the next fixture rot fails with
   its diagnostic instead of a bare truncation.
