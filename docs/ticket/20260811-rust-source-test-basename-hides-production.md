# rust_source.rb's test-basename filter hides two production modules from every ledger

- **Observed during**: 2026-08-11 product analysis (debt mining); independently recorded in DD-147:399 and DD-163:109, both deferring the fix
- **Severity**: medium (shared governance tooling: every ledger built on `rust_source_files` under-scans the same two production files)
- **Symptom**: production modules whose filenames match `/test.*\.rs\z/` are excluded from governed source scans
- **Status**: open

## Mechanism (at 6678144d, re-verify)

`scripts/lib/rust_source.rb:56` excludes by basename `/test.*\.rs\z/`. Two
named live instances (from DD-163):

- `crates/orchestrator-runner/src/test_env.rs` — declared
  `pub(crate) mod test_env;` with **no cfg(test)**;
- `crates/orchestrator-scheduler/src/scheduler/safety/self_test.rs` —
  unconditional `mod self_test;`, emits the production diagnostic
  `[empty_change_check]`.

Related but deliberately different: `test-error-code-glossary.sh:26-29`
carries its own *narrower* exclusion rule for the same question — two
exclusion predicates for "is this file production" in one repo.

## For ticket-fix

1. Classification: Bug in shared tooling (the fr-governance skill's §5
   explicitly flags inheriting known-defective shared functions as a
   regression). The two DDs park it; this ticket schedules it.
2. Repair direction: exclude by **cfg-tested membership** (files under a
   `#[cfg(test)] mod` / `tests/` dir) rather than basename; or keep basename
   but assert the excluded set contains no file reachable from a non-test
   `mod` declaration — the second is a closure property, not a spelling
   (§4.4 shape 4).
3. Expect ledger movement when the two files enter the scans — re-derive the
   affected counts (rusqlite boundary, coordination coordinates) and update
   ledgers in the same change, per DD-142's discipline.
4. Reconcile or explicitly justify the glossary gate's separate rule.
