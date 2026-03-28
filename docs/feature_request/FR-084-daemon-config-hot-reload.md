# FR-084: Daemon Configuration Hot Reload

## Status: Open
## Priority: P2
## Created: 2026-03-28

## Problem

When new resources (triggers, workflows, agents) are applied via `orchestrator apply`, the running daemon does not pick up the changes until restarted. This causes webhook triggers and other dynamic resources to return "not found" errors even though they exist in the database.

## Acceptance Criteria

1. After `orchestrator apply -f <manifest>`, the daemon's in-memory config_runtime reflects the new resources within 5 seconds without requiring restart.
2. `orchestrator apply` returns a confirmation that the daemon has acknowledged the config change (or a warning if the daemon is not running).
3. No existing task execution is disrupted during config reload.

## Reproduction Steps (from ticket 128-s2-s3-daemon-config-stale)

1. Start daemon: `orchestratord --foreground --workers 2`
2. Apply a webhook trigger: `orchestrator apply -f trigger-manifest.yaml --project default`
3. Fire the webhook: `curl -X POST http://localhost:PORT/webhook/trigger-name`
4. **Expected**: Trigger fires successfully
5. **Actual**: `{"error":"trigger 'trigger-name' not found"}`

## Possible Approaches

- **A**: Daemon watches config DB for changes (polling or SQLite WAL notifications)
- **B**: CLI sends a reload signal (SIGHUP or gRPC `ReloadConfig` RPC) after `apply`
- **C**: Daemon re-reads config on every trigger/webhook request (lazy reload)

## Related

- QA doc: `docs/qa/orchestrator/128-webhook-trigger-infrastructure.md` (S2/S3 blocked)
- Ticket: `128-s2-s3-daemon-config-stale` (closed with FR reference)
