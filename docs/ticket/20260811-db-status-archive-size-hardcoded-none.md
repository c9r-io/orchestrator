# db status reports archive_size from a hardcoded None

- **Observed during**: 2026-08-11 product analysis; this is the repository's only code-marked TODO
- **Severity**: low (an observability field that can never reflect a configured archive location)
- **Symptom**: `core/src/service/system.rs:168` — `None, // TODO: archive dir from config` — `db_status` passes `None` as the archive dir to `db_maintenance::database_size_info`, so the archive figure in `orchestrator db status` is structurally absent regardless of configuration
- **Status**: open

## For ticket-fix

1. Check whether an archive dir is configurable at all (config surface for
   `db_maintenance`); if yes, thread it; if no, decide whether the field
   should exist in the RPC response — a field that is always `None` is a
   promise the API cannot keep.
2. Classification: likely Bug (small); if the archive-dir config itself does
   not exist, it may be a feature gap → route per the ticket-fix contract.
3. Whatever the outcome, remove or fulfil the TODO — it is the last
   `TODO|FIXME|HACK|XXX` marker in 193k lines of tracked Rust, and zero is a
   nicer invariant than one.
