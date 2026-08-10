# default_env_allowlist is defined twice in one crate, and three fixtures drifted

- **Observed during**: 2026-08-11 product analysis (debt mining)
- **Severity**: medium (a change to one default silently misses the other; fixture drift already shipped)
- **Symptom**: two byte-identical definition blocks; three fixtures missing `TERM`; two fixtures disagree on whether `ORCHESTRATOR_SOCKET` belongs in the step-env set
- **Status**: open

## Mechanism (at 6678144d, re-verify)

- `fn default_env_allowlist()` — and the whole `default_shell_arg` /
  `default_allowed_shells` / `default_allowed_shell_args` /
  `default_redaction_patterns` block — exists identically in
  `crates/orchestrator-config/src/cli_types.rs:308-342` **and**
  `crates/orchestrator-config/src/config/runner.rs:39-65`.
- Fixture drift (rg `env_allowlist` over `*.yaml`):
  `source-task-template-fixture.yaml`, `process-console-vertical-flow.yaml`,
  `handoff-safe-resume.yaml` list `PATH HOME USER LANG` — **missing `TERM`**;
  `wp05-store-spawn.yaml` includes `ORCHESTRATOR_SOCKET`,
  `fr156-pipeline-variable-parity.yaml` does not — and DD-169's Known limits
  marks exactly this variable set as load-bearing for the CLI-store pattern
  ("the one respect in which the migration is a step back").

## For ticket-fix

1. Classification: Bug (duplication) + config drift. Collapse the two
   definition blocks to one (config/runner.rs is the semantic home;
   cli_types re-exports), fix the three fixtures, and decide the
   `ORCHESTRATOR_SOCKET` question once — recording it in the fixture comments
   both ways.
2. Consider whether a drift check belongs anywhere (a fixture allowlist that
   is a strict subset of the default minus a named reason) — a decision, not
   an obligation.
3. Verify by running the affected gates (template, console-flow, handoff,
   wp05, fr156 parity).
