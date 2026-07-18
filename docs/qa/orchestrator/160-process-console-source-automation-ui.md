---
self_referential_safe: true
---

# Orchestrator - Process Console Source Automation UI

**Module**: Orchestrator
**Scope**: Template/badge management, daemon preview/simulation, route diagnosis/replay, RBAC/privacy, and accessible UI regression
**Scenarios**: 5
**Priority**: Critical

## Automated Entry Point

```bash
./scripts/qa/test-source-automation-ui.sh
```

The script uses only deterministic manifests, an isolated daemon, mocked browser Tauri transport, and the local fake Slack adapter. It never contacts a real Slack workspace or invokes a paid coding agent. The real-daemon portion applies `fixtures/manifests/bundles/source-task-routing-fixture.yaml` and a second deterministic template/binding in the same installation.

## Scenario 1: Create, Preview, Bind, And Simulate Two Badges

### Preconditions

- Apply the deterministic source-routing fixture to `{project_id}`.
- Open `#/sources/automations/templates` as Operator.

### Goal

Validate no-YAML management and daemon-equivalent draft behavior for two distinct badge-to-Skill/workflow recipes.

### Steps

1. Select “New”, enter a unique template name, Skill descriptor, workflow/workspace, goal template, and allowlisted variables.
2. Select the configured Slack installation, enter a sample permalink, and select “Render preview”.
3. Select “Review and save”, enter an audit reason, and apply.
4. Open “Badge bindings”, create an exact badge rule for the new template, select channel/role policy, and select “Simulate badge”.
5. Save the binding; repeat with a different badge and template in the same Slack installation.

### Expected

- Preview shows the exact daemon-rendered goal/action and warns that it is side-effect-free; no task, route, Attention, or provider request is created.
- Simulation selects the expected binding/template and trusted Trigger-derived role with `mutation_performed=false` and `network_performed=false`.
- Both badges coexist and select different templates without overlap.
- Unknown variables, missing references, invalid badge/channel policy, and unauthorized roles appear beside the responsible field using daemon diagnostic scope.

## Scenario 2: Optimistic Save And Reversible Lifecycle Controls

### Steps

1. Open the same template/binding in two clients and record its displayed revision.
2. Save a valid change from client A with an audit reason.
3. Attempt to save or suspend the stale draft from client B.
4. Reload, select “Suspend binding”, enter a reason, verify simulation reports suspension, then select “Resume binding” with the new revision.

### Expected

- Create uses `require_absent`; edit/suspend/resume use the normalized expected revision.
- The stale operation returns an explicit reload-and-review error and cannot overwrite current policy.
- Suspend/resume is audited, immediately authoritative, and preserves route/attempt/generation history.
- Read-only users have no save/suspend/resume controls or focusable mutation remnants.

## Scenario 3: Route Diagnosis, Filters, Deep Links, And Reviewed Replay

### Steps

1. Produce or load routes across multiple states, bindings, and task IDs; open “Recent routes”.
2. Apply state, binding, and task filters and select a `needs_attention` route.
3. Inspect stable failure, pinned binding/template hashes, request ID, attempt timeline, and health counts.
4. Follow links to source event, binding, template, Attention, and Process Workspace; follow the Attention link back to the route.
5. Select “Replay”, review pinned-generation consequences, enter a reason, optionally choose current-config adoption, and repeat with a stale route version/idempotency key.

### Expected

- Filters are sent to the daemon and results reload from authoritative route state.
- Every provenance link has a stable internal route and preserves the canonical identifiers.
- Replay is unavailable until a reason exists, sends the displayed positive version and unique idempotency key, and defaults to pinned configuration.
- Stale version fails closed and reloads; duplicate delivery/replay never creates a second canonical task.

### Expected Data State

```sql
SELECT COUNT(DISTINCT task_id)
FROM source_automation_routes
WHERE id = '{route_id}';
-- Expected: 1
```

## Scenario 4: Privacy And Role Boundaries

### Steps

1. Inspect Templates, Badge Bindings, Recent Routes, Events, and linked Attention as ReadOnly, Operator, and Admin.
2. Capture Tauri catalog/list/get/event payloads and browser DOM, `localStorage`, and `sessionStorage`.
3. Search them for signing secret, bot token, SecretStore value/reference, `normalized_json`, raw Slack body/content, attachments, transcript, protected permalink, and rendered goal outside the active preview panel.
4. Attempt direct replay/apply/suspend RPCs with insufficient authority.

### Expected

- ReadOnly can inspect safe policy/route metadata and run non-mutating preview/simulation but cannot mutate or fetch protected Slack permalinks.
- Catalog exposes installation/actor/role policy only; credentials and message content never cross Tauri.
- `SourceEvent` contains bounded reaction provenance and never exposes `normalized_json`.
- DOM/storage and audits contain no forbidden fixture secret/message values; daemon RBAC rejects direct bypass attempts.

## Scenario 5: Accessibility, Responsive Layout, And Regression

### Entry Visibility

The workbench is discoverable through the existing “Sources” primary navigation and its “Automations” subview; it does not add a top-level navigation item.

### Steps

1. Enter through the visible “Sources” primary navigation, verify its active state, then navigate every Sources primary/subview and editor using keyboard only at desktop width and 640 px.
2. Open reviewed save/suspend/replay dialogs; cycle Tab/Shift+Tab, press Escape, and confirm focus restoration.
3. Trigger validation, preview, simulation, watch update, and stale error; inspect labels, `role=alert/status`, status text, and focus visibility.
4. Enable reduced motion and reduced transparency, then run axe on read-only and operator automation pages.
5. Run all Process Console Vitest/Playwright/build tests and the real Tauri/daemon bridge test.

### Expected

- Narrow layout collapses to one column without hiding actions, errors, or selected detail.
- The feature remains discoverable under the existing Sources navigation; Automations does not create or require a separate top-level item.
- Dialog focus is trapped/restored; confirmation is disabled until a non-empty reason exists.
- State/failure meaning is not color-only; controls have accessible names and async results use bounded live semantics.
- Axe reports no serious/critical violations, reduced motion removes transitions, and all existing console navigation/features remain green.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Create, preview, bind, simulate | PASS | 2026-07-18 | Codex | Two templates/bindings plus daemon draft preview/simulation |
| 2 | CAS and reversible lifecycle | PASS | 2026-07-18 | Codex | Real Tauri create-CAS and binding suspend/resume |
| 3 | Route diagnosis and replay | PASS | 2026-07-18 | Codex | Filters, attempts, internal deep links, version/reason/idempotency |
| 4 | Privacy and role boundaries | PASS | 2026-07-18 | Codex | Safe catalog/event projection and DOM/storage scan |
| 5 | Accessibility and regression | PASS | 2026-07-18 | Codex | 70 Vitest, 19 Playwright, axe/narrow/focus/build |
