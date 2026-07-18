# Agent Process Console v1 Operations

This runbook is the operator contract for upgrading, verifying, stopping, and rolling back the local-first Agent Process Console v1. The daemon remains authoritative for state and migrations; the CLI and Tauri GUI are clients and must never edit SQLite directly.

This document retains the original Console-specific migrations 27-32 boundary. Deployments enabling Slack Reaction Skill Automation must additionally qualify migrations 33-34 and follow `docs/guide/slack-reaction-skill-automation.md`.

## Supported Upgrade Boundary

- The release gate proves a populated schema 26 database upgrades through migrations 27-32 without losing task, Session, Attention, handoff, source binding, or action-audit identity.
- Databases older than schema 26 must first be upgraded by an intermediate supported release.
- Console migrations are additive and forward-only. A normal rollback keeps migrations 27-32 and their tables.
- CLI, daemon, and GUI should be deployed from the same release. Additive gRPC fields and retained tables permit a short rolling mismatch, but mutations should wait until all clients are current.
- Desktop packaging and distribution remain deferred to FR-076; this runbook covers the local daemon, CLI, and existing Tauri development/runtime surface.

Required tools are `bash`, `cargo`, `git`, `jq`, `npm`, `rg`, `sqlite3`, and `tee`. Before starting, verify enough free space for the database backup, build products, and temporary QA fixtures:

```bash
command -v bash cargo git jq npm rg sqlite3 tee
df -h "${ORCHESTRATORD_DATA_DIR:-$HOME/.orchestratord}"
git status --short
```

The release gate requires a clean worktree.

## Pre-upgrade Backup And Drain

1. Stop accepting new tasks while allowing active work to reach a safe boundary:

   ```bash
   orchestrator daemon maintenance --enable
   orchestrator daemon status
   orchestrator task list -o json
   ```

2. Inspect active Sessions and tasks. Ask agents to stop at an idempotent/checkpointed boundary; do not terminate a live writer merely to accelerate the upgrade.
3. Locate the database with `orchestrator db status -o json`, then set explicit paths:

   ```bash
   DB_PATH="$(orchestrator db status -o json | jq -r '.db_path')"
   BACKUP_PATH="${DB_PATH}.pre-console-v1.$(date +%Y%m%d%H%M%S).backup"
   sqlite3 "$DB_PATH" 'PRAGMA quick_check;'
   sqlite3 "$DB_PATH" ".backup '$BACKUP_PATH'"
   chmod 600 "$BACKUP_PATH"
   shasum -a 256 "$BACKUP_PATH"
   sqlite3 "$BACKUP_PATH" 'PRAGMA quick_check;'
   ```

   Both checks must return `ok`. Use SQLite `.backup`; do not copy a live database file with `cp`.

4. Record current state for comparison:

   ```bash
   orchestrator db status -o json > /tmp/orchestrator-db-before.json
   orchestrator db migrations list -o json > /tmp/orchestrator-migrations-before.json
   ```

5. After active work is drained, stop the daemon:

   ```bash
   orchestrator daemon stop
   orchestrator daemon status
   ```

## Upgrade And Migration Verification

Install the daemon, CLI, and GUI bundle atomically according to the local distribution method, then start `orchestratord`. Startup applies pending migrations through the normal migration kernel.

```bash
orchestratord --foreground --workers 2
# In another terminal:
orchestrator db status -o json | jq -e '.is_current == true and .current_version >= 32'
orchestrator db migrations list -o json \
  | jq -e 'all(.migrations[] | select(.version >= 27 and .version <= 32); .applied == true)'
```

Migration 31 presence is an identity/capability check, not `MAX(version) == 31`: `m0031_control_action_audit` and the `control_action_audit` schema must exist even when migration 32 or later additive migrations are present.

## Feature Rollout Order

Apply RuntimePolicy through the daemon. Session read/control are global `_system` decisions; project policies cannot override them.

```bash
orchestrator apply --project _system -f runtime-policy.yaml
```

Use this order, verifying each domain before enabling the next:

1. Enable `attention_inbox_enabled` and `handoff_enabled`; keep mutating recovery off. Verify Attention reads, timeline evidence, and handoff generation.
2. Keep `_system.session_read_enabled=true` and `_system.session_control_enabled=false`. Verify Session list/get/read and transcript redaction.
3. Verify process metrics with `orchestrator metrics process --project {project} --window 24h --bucket 1h -o json`. Metrics collection defaults may remain enabled because payload content is excluded.
4. Enable `source_ingest_enabled` only for validated projects/providers. Verify signature/replay handling and source binding before accepting production events.
5. Start `action_audit_mode=compatibility`, update every mutating client to send the canonical action context, inspect `orchestrator audit list --project {project} -o json`, then switch to `enforced`.
6. Enable `mutating_resume_enabled` after reviewed-resume smoke tests. Leave `elevated_resume_enabled=false` unless an operator has explicitly reviewed a non-idempotent boundary.
7. Enable `_system.session_control_enabled` last. Verify one writer lease, fencing, exactly-once input, and safe close.
8. Build the GUI with the desired `VITE_CONSOLE_ATTENTION`, `VITE_CONSOLE_PROCESSES`, `VITE_CONSOLE_SESSIONS`, `VITE_CONSOLE_SOURCES`, and `VITE_CONSOLE_SYSTEM` values. Omitted values default to enabled; set a domain to `false` for a build-time stop-loss.

Finally disable maintenance mode:

```bash
orchestrator daemon maintenance --disable
```

## Smoke And Compatibility Checks

Run these checks for every rollout project:

```bash
orchestrator task list --project {project} -o json
orchestrator attention list --project {project} -o json
orchestrator agent session list -o json
orchestrator audit list --project {project} -o json
orchestrator metrics process --project {project} --window 24h --bucket 1h -o json
```

Open the GUI and verify Attention → Process Workspace → evidence/handoff and the Sessions, Sources, and System → Operations destinations. A supported deployment has a current daemon, current CLI, and current GUI. A previous client may read retained additive data during a short rollout, but do not use it for new mutations after audit enforcement is enabled.

Before release, execute the complete clean-tree gate:

```bash
./scripts/qa/test-process-console-release.sh
```

Set `KEEP_RELEASE_QA=1` only while diagnosing a failure; default execution deletes isolated logs and fixtures.

## Domain Stop-loss

| Symptom | Immediate action | State retained |
|---|---|---|
| Attention lag/failure | Disable `attention_inbox_enabled`; stop the Attention projector and use task/timeline reads | Attention rows, cursor, and task events |
| Handoff/resume regression | Disable `mutating_resume_enabled` and `elevated_resume_enabled`; keep handoff read/generation only if healthy | Snapshots, plans, executions, audit joins |
| Session control regression | Apply `_system.session_control_enabled=false`; preserve read access if safe | Sessions, transcript offsets, leases, action audit |
| Source routing regression | Disable `source_ingest_enabled` and suspend external triggers/webhooks | Source events, bindings, routing/audit state |
| Audit client incompatibility | Stop mutations and return to `compatibility` only while clients are upgraded; never bypass authorization | Canonical and domain audit rows |
| Metrics/projector regression | Disable optional/UI metric collection, stop rebuild/prune, and rely on authoritative domain reads | Observations and rollups; product behavior continues |
| GUI-only regression | Deploy the prior GUI bundle or rebuild the affected `VITE_CONSOLE_*` domain as `false` | All daemon state and APIs |

## Normal Binary Rollback

A binary rollback is not a database rollback.

1. Enable maintenance mode and stop external source delivery.
2. Apply fail-closed RuntimePolicy: `source_ingest_enabled=false`, `_system.session_control_enabled=false`, `mutating_resume_enabled=false`, and `elevated_resume_enabled=false`. Stop optional projectors/metrics maintenance.
3. Allow active idempotent work to drain, then stop the daemon.
4. Install the previous daemon/CLI/GUI binaries and start the previous daemon against the existing database.
5. Keep migrations 27-32, all Console tables, and all unknown additive columns. Do not run `DROP`, delete migration catalog rows, or fabricate a lower schema version.
6. Verify daemon health, database status, task/timeline reads, and the domains supported by the previous binary. Keep mutation flags off until compatibility is confirmed.

If the previous binary cannot safely open the additive schema, stop it and forward-fix with the current binary. Do not improvise a down migration.

## Disaster Database Restore

Restore the backup only when startup migration failed, `PRAGMA quick_check` reports corruption, or the current database is otherwise proven unusable. Do not restore merely because a feature or GUI regressed.

1. Stop the daemon and preserve the failed database and its WAL/SHM files for diagnosis.
2. Verify the backup checksum and `PRAGMA quick_check` result.
3. Move the failed database aside; restore the verified backup to the exact configured path with owner-only permissions.
4. Start the binary compatible with the backup schema, verify `db status` and migrations, then upgrade through the supported path.
5. Reconcile any work accepted after the backup from external/audit evidence; a restore intentionally loses post-backup writes.

Never merge SQLite files manually or delete Console migration records to make a binary start.
