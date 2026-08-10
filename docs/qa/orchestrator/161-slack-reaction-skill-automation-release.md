---
lifecycle: active
related_fr: FR-113
self_referential_safe: true
---

# Orchestrator - Slack Reaction Skill Automation Release

**Module**: Orchestrator  
**Scope**: Clean-tree aggregate, two-badge signed vertical flow, recovery, migration/rollback, GUI, and release documentation  
**Scenarios**: 5  
**Priority**: Critical

## Automated Entry Point

```bash
./scripts/qa/test-slack-skill-automation-release.sh
```

The aggregate uses only isolated data roots, deterministic echo agents, a local fake Slack API, and a compatible repository worktree. It never connects to a real Slack workspace or paid AI provider. The vertical scenario applies:

```bash
orchestrator apply --project {project_id} \
  -f fixtures/manifests/bundles/slack-skill-automation-release-fixture.yaml
```

## Scenario 1: Clean Current-HEAD Aggregate Preserves Slice Ownership

### Preconditions

- `git status --porcelain` is empty.
- `bash`, `cargo`, `curl`, `git`, `jq`, `npm`, `openssl`, `python3`, `rg`, `sqlite3`, and `tee` are installed.

### Goal

Prove one release command builds current artifacts and executes repository and FR-107 through FR-112 acceptance without hiding the failing owner.

### Steps

1. Run `./scripts/qa/test-slack-skill-automation-release.sh`.
2. Confirm fresh daemon, CLI, Rust GUI, and Web GUI builds run before daemon-backed fixtures.
3. Confirm workspace tests, strict Clippy, frontend coverage, Playwright, production build, and documentation lint pass.
4. Confirm the coordinator invokes the six owning slice scripts once and reports their FR owner and duration.
5. Repeat with a dirty worktree and confirm the gate rejects it before starting a fixture.

### Expected

- Current HEAD is the exact tested candidate.
- Failure is fail-fast and prints the owning gate plus retained log path.
- Standalone slice scripts remain executable with their dependency behavior unchanged.
- Success prints the complete gate summary and deletes logs unless `KEEP_RELEASE_QA=1` is set.

## Scenario 2: Signed Two-Badge Flow, Identity Convergence, And Recovery

### Preconditions

- Apply `fixtures/manifests/bundles/slack-skill-automation-release-fixture.yaml` to `{project_id}`.
- Start the isolated fake Slack API and daemon through `scripts/qa/test-slack-skill-automation-vertical.sh`.

### Goal

Prove the complete authenticated Slack-to-task path for two Skills and every required recovery boundary.

### Steps

1. Send signed `reaction_added` deliveries for `agent-implement` and `agent-docs` against the same message; verify HTTP 200 and durable `source_events` rows.
2. Wait for distinct routes and completed tasks; verify binding/template hashes, Skill goal prefix, workflow, permalink, source binding, and canonical action audit.
3. Send four concurrent event IDs for one message/badge/binding identity and verify one route/task.
4. Send the fake-API 429 fixture, wait for `retrying`, stop and restart the daemon, and verify the same route completes.
5. Apply the invalid outbound credential, verify `needs_attention`, restore it, run template preview and binding simulation, then replay with reason/version/idempotency/current-config adoption.

### Expected

- Provider acknowledgement follows durable persistence and asynchronous routing.
- `agent-implement` selects `$ticket-fix`/`slack-release-implement`; `agent-docs` selects `$qa-doc-gen`/`slack-release-docs`.
- The two different bindings on the same message retain two automation SourceBindings and create two distinct tasks.
- Duplicate and concurrent delivery never create a second canonical task for one automation identity.
- Restart consumes the persisted retry checkpoint without requiring another Slack event.
- Reviewed replay advances generation, creates one task, and resolves the linked Attention item.

### Expected Data State

```sql
SELECT message_ts, reaction, COUNT(DISTINCT id), COUNT(DISTINCT task_id)
FROM source_automation_routes
WHERE project_id = '{project_id}'
GROUP BY message_ts, reaction;
-- Expected: every message/reaction identity has one route and one task

SELECT message_ts, COUNT(*) AS bindings, COUNT(DISTINCT task_id) AS tasks
FROM source_automation_routes
WHERE project_id = '{project_id}'
  AND reaction IN ('agent-implement', 'agent-docs')
GROUP BY message_ts;
-- Expected for the primary two-badge fixture: one row | bindings=2 | tasks=2

SELECT r.id, r.status, r.generation, a.state
FROM source_automation_routes r
LEFT JOIN attention_items a ON a.source_route_id = r.id
WHERE r.id = '{replayed_route_id}';
-- Expected: routed | generation 2 | resolved
```

## Scenario 3: Populated Upgrade And Compatible Binary Rollback Preserve Data

### Preconditions

- Use only the file-backed migration tests and isolated vertical database.
- The default previous ref is `58166a9f52681878d4fd80c67b06a25e14a26c62` (the
  v0.5.0 release commit), or set an explicitly reviewed `FR113_PREVIOUS_REF`.
- **Pin-advance rule** (added after the 0.3.1-era pin rotted): the pin is "the
  previous release", advanced to the prior release's commit at each release and
  qualified **by running this gate**, never by reading. A pin left behind fails
  as a 500 from the old daemon once the schema moves past its window — measured
  2026-08-10 against schema 37, rollback-disabled.code=500.

### Goal

Verify forward migration and normal binary rollback retain Console and source automation evidence without a down migration.

### Steps

1. Run `cargo test -p agent-orchestrator populated_v26_process_console_upgrade_preserves_entities_and_rebuilds_metrics --lib`.
2. Run `cargo test -p agent-orchestrator populated_v33_source_automation_upgrade_preserves_route_and_provenance --lib`.
3. Let the vertical script create an SQLite `.backup` and require `PRAGMA quick_check` to return `ok`.
4. Disable `reactionRouting`, build the compatible previous daemon in an isolated worktree, and start it against the schema-34 database.
5. Read tasks/routes, send one disabled reaction, return to the current daemon, and compare task, route, source, migration, and integrity counts.

### Expected

- Existing task, Session, Attention, handoff, source binding, audit, metrics, route, task, and request-ID associations are preserved.
- Migration 34 normalizes `completed` to `routed` and backfills generation 1 plus route change version 1.
- The previous daemon creates no new automation route/task while writers are disabled.
- Migrations 33-34 and all created task/source/route evidence remain present after current daemon recovery.
- No `DROP`, migration-row deletion, schema version fabrication, or routine backup restore occurs.

### Expected Data State

```sql
SELECT version, name FROM schema_migrations WHERE version IN (33, 34);
-- Expected: 33=m0033_source_automation_routes, 34=m0034_source_automation_operations

SELECT route_id, generation, binding_name, template_name
FROM source_automation_route_generations
WHERE route_id = '{route_id}';
-- Expected: one preserved generation-1 row or later reviewed generations
```

## Scenario 4: Visible Console Entry, Real Tauri Provenance, RBAC, And Accessibility

### Entry Visibility

The feature is reachable from visible primary navigation through "Sources" → "Automations". It does not require a direct hash URL or add another top-level destination.

### Steps

1. Navigate through "Sources" → "Automations" → "Recent routes" and open a routed item.
2. Follow the source event, Attention, binding/template, and Process Workspace links; verify the matching identifiers.
3. Run `live_slack_skill_release_crosses_tauri_provenance_boundary` against the real isolated daemon.
4. Run frontend coverage and Playwright suites as Operator and ReadOnly at desktop and 640 px widths.
5. Run axe, keyboard dialog focus, reduced-motion/transparency, DOM/storage redaction, and direct-RPC role assertions.

### Expected

- Production Tauri handlers join route, protected route, source event, task binding, task detail, and timeline through gRPC.
- Safe route list/get omits permalink; only Operator+ protected route retrieval returns it.
- ReadOnly sees no hidden/focusable mutation or protected-link control, and daemon RBAC rejects direct bypass.
- No signing secret, bot token, `normalized_json`, message body, rendered goal, or protected URL appears in safe projections or browser storage.
- There are no serious/critical axe violations and the narrow layout retains every required action and error.

## Scenario 5: Guide, Changelog, And Diagnostic Privacy Are Release-complete

### Steps

1. Follow `docs/guide/slack-reaction-skill-automation.md` without consulting a design document.
2. Validate its setup, preview, simulation, enable, inspect, diagnose, suspend, credential rotation, backup, upgrade, smoke, stop-loss, and rollback commands against current CLI help.
3. Verify `CHANGELOG.md` covers capability, migrations 33-34, Slack permissions/secrets, compatibility, privacy defaults, and non-goals.
4. Inspect aggregate and retained vertical logs for all fixture signing secrets/tokens, private Slack host/URL, rendered goal, and raw payload markers.
5. Run `./scripts/qa-doc-lint.sh`.

### Expected

- A new operator can configure two badge automations and identify every normal/error state from the guide alone.
- Normal rollback is forward-only and never deletes created tasks or additive source data.
- Logs contain stable gate/state diagnostics only; failure retention does not weaken the privacy boundary.
- Documentation lint and the CLI guide-contract gate pass.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Clean current-HEAD aggregate and slice ownership | PASS | 2026-07-18 | Codex | 16-gate clean-tree aggregate passed in 381 seconds |
| 2 | Signed two-badge flow, identity convergence, and recovery | PASS | 2026-07-18 | Codex | Isolated vertical QA passed all 12 gates |
| 3 | Populated upgrade and compatible rollback | PASS | 2026-07-18 | Codex | Migration regressions and real previous daemon passed |
| 4 | Console entry, Tauri, RBAC, privacy, and accessibility | PASS | 2026-07-18 | Codex | Real Tauri boundary and all 19 Playwright tests passed |
| 5 | Guide, changelog, and diagnostic privacy | PASS | 2026-07-18 | Codex | Guide contract, doc lint, and retained-log privacy scan passed |
