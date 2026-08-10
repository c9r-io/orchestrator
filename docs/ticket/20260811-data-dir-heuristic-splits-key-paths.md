# A data directory literally named `data/` splits the daemon's key paths in two

- **Observed during**: ticket-fix verification of `20260811-webhook-gates-write-real-data-dir` (2026-08-11)
- **Severity**: medium (foot-gun: any deployment or harness that names its `ORCHESTRATORD_DATA_DIR` with a trailing `data/` component gets a SecretStore that can never write)
- **Symptom**: with `ORCHESTRATORD_DATA_DIR=$ROOT/data`, every SecretStore apply fails `SecretStore write blocked: no active encryption key (all keys revoked or retired)` — while `orchestrator secret key list` reports the primary key `active`
- **Status**: open

## Mechanism (measured, not read)

Two views of "where do the secrets live" disagree inside one daemon:

- The boot path honours `ORCHESTRATORD_DATA_DIR` and seeds
  `$ROOT/data/secrets/secretstore.key` plus an `active` row in `secret_keys`.
- The repository write path re-derives the data dir from the **db path** via
  `resolve_data_dir_from_db_path` (`crates/orchestrator-security/src/secret_store_crypto.rs:364`),
  whose layout heuristic says: if the db's parent directory is literally named
  `data`, the data root is the **grandparent**. It then looks for keys at
  `$ROOT/secrets/` — which nothing ever wrote — and the keyring loads with
  `active_key = None`.

Reproduced deterministically both ways: `ORCHESTRATORD_DATA_DIR=$ROOT/data` →
apply blocked on first boot and after restart; `ORCHESTRATORD_DATA_DIR=$ROOT/runtime`
→ identical sequence passes. `secret key list` reads through a path that
resolves consistently, so the operator-visible state says `active` while
writes are refused — the diagnostic actively points away from the cause.

This is why the slack/source QA gates all name their isolated dir `runtime/`:
the convention dodges a heuristic nobody wrote down. The webhook gates now do
the same (commit that closes the parent ticket), with the reason in a comment.

## For ticket-fix

The heuristic exists for a real layout (`<root>/data/<db>` split vs flat) and
has a unit test naming both. The repair question is design-level: either make
the write path honour `ORCHESTRATORD_DATA_DIR` when set (one source of truth,
env beats inference), or make boot refuse a data dir whose basename is `data`
when the env var is set (fail loudly at the split point instead of at the
first secret write). Either way the `secret key list`-says-active /
write-says-none divergence deserves its own regression test.
