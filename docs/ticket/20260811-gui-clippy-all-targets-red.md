# cargo clippy -p orchestrator-gui --all-targets fails today

- **Observed during**: 2026-08-11 product analysis; self-reported in DD-165:108 and parked
- **Severity**: low (blocks strict-clippy coverage of the GUI crate; the workspace-level gate presumably scopes around it)
- **Symptom**: per DD-165 — `cargo clippy -p orchestrator-gui --all-targets -- -D warnings` fails on `orchestrator-persistence::configure_conn` becoming dead code under narrowed feature unification
- **Status**: open

## For ticket-fix

1. Reproduce the exact failure (the dead-code site was observed as
   `crates/orchestrator-persistence/src/db.rs:152` in an FR-160-era build
   warning — likely the same object).
2. Classification: Bug (a parked build defect). Repair: gate the function
   behind the feature that uses it, or remove it, or reference it — whichever
   the feature graph says is true.
3. Then check what the CI clippy job actually covers: if it excludes the GUI
   crate, note whether that exclusion is declared anywhere (an undeclared
   scope hole is §4.4 shape 2) — fixing the declaration may be part of the
   close.
