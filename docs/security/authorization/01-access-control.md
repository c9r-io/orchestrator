---
lifecycle: active
---

# Authorization Security - Access Control And Privilege Tests (Generic)

**Module**: Authorization  
**Scope**: IDOR, horizontal/vertical privilege escalation, admin boundaries, multi-tenant isolation (if applicable)  
**Scenarios**: 5  
**Risk**: Critical  
**OWASP ASVS 5.0**: V8 Authorization

Project-specific FR-109 overlay: `SourceTaskBinding` read/simulate uses read access; apply/delete/suspend/resume requires Operator authority and canonical audit. Admin authority plus explicit `--force --force-references` is required to atomically remove bindings that reference a Trigger or SourceTaskTemplate. Matching never trusts a request role: the external actor ID is mapped through the same-project Trigger `actorRoles`, and unknown actors inherit no privilege. See `docs/qa/orchestrator/157-source-task-binding-badge-matching.md`.

FR-110 overlay: `SourceEventList/Get` may expose route status, binding/template identity, and hashes to `read_only+`, but never the Slack permalink. `SourceAutomationRouteGet` requires `operator+` and is the only source-automation read that returns the protected permalink. Sources/timeline UI must omit the route fetch and link entirely for read-only users; daemon RBAC remains authoritative. The outbound Slack actor role is derived from Trigger `actorRoles`, not the UI/control-plane caller. See `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md` Scenario 4.

FR-111 overlay: `SourceAutomationList/Get/Watch/Simulate/StatusGet` require `read_only+` and expose safe operational projections only. `SourceAutomationReplay/Ignore`, binding suspend/resume, and Trigger suspend/resume require `operator+` plus canonical audit context; replay/ignore additionally require a reason, positive expected route version, and idempotency key. Current-config adoption must re-authorize the original external actor and same stable binding. Generic admin `SourceReplay` must reject automation-linked events. See `docs/qa/orchestrator/159-source-automation-reliability-operations.md` Scenarios 2-4.

FR-112 overlay: Process Console automation editors may expose safe catalog/config metadata and daemon preview/simulation to `read_only+`, but save, suspend/resume, replay, and ignore controls must be absent below Operator. Direct Tauri/RPC invocation remains daemon-authorized. Protected Slack permalink retrieval stays Operator-only and is never part of catalog/list/get/watch. Reviewed mutations carry reason plus resource revision or route version. See `docs/qa/orchestrator/160-process-console-source-automation-ui.md` Scenarios 2 and 4.

FR-114 overlay: `SourceConnectionCatalog/List/Get/Watch` expose only safe project-scoped state to `read_only+`. Connect, intent status/cancel, reauthorize, disconnect, and transfer require Admin at the daemon even when GUI controls are absent. Installation delivery/proxy additionally requires the exclusive Gateway owner daemon and current pairing/generation. Cross-project list/get/watch, cross-installation claim/proxy/ack, old-owner transfer calls, and target handoff claims must fail closed. Run `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md` Scenarios 2 and 4.

---

## Background

Access control issues are often not "missing authentication", but:
- Missing resource-level authorization (IDOR)
- List endpoints missing server-side filtering
- Unclear role boundaries leading to privilege escalation
- Multi-tenant isolation gaps enabling cross-tenant access

Project-specific overlay: for FR-097, verify `HandoffGet` and `ResumeBoundaryList` allow `read_only+`, while `HandoffGenerate`, `ResumePlan`, and `ResumeExecute` require `operator+`. The server must derive the actor from mTLS/UDS identity and ignore any client attempt to self-report an actor or role. See `docs/qa/orchestrator/144-handoff-and-safe-resume.md`.

FR-098/FR-102/FR-105 overlay: `AgentSessionList/Get/Read/ResolvePid` and reader Attach allow `read_only+`; writer Attach, Heartbeat, SendInput, writer Detach, and Close require `operator+` and the global `_system` `session_control_enabled` policy. Ordinary project RuntimePolicy resources must never override that global gate. Read-only UI must omit writer, input, and close controls rather than merely hiding them. Verify trusted transport roles and denial request IDs with `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md`, and verify deterministic policy authority with `docs/qa/orchestrator/152-session-runtime-policy-authority.md`.

FR-099 overlay: `SourceEventList/Get` and `SourceBindingList` require `read_only+`; `SourceEventIngest` and `SourceBind` require `operator+`; `SourceReplay` requires `admin`. Slack actors do not inherit the control-plane caller role: the daemon resolves `actorRoles` from the trusted Trigger installation and defaults unknown users to `read_only`. Verify privileged external commands fail closed and record the resolved role with `docs/qa/orchestrator/146-source-events-and-slack-binding.md`.

---

## Scenario 1: Resource-Level Authorization (IDOR)

### Preconditions
- Two identities: user A and user B (different privileges or different resource ownership)
- A resource endpoint exists: `GET/PUT/DELETE /api/v1/{resource}/{id}`

### Attack Objective
Verify user A cannot access/modify a resource not owned by them.

### Attack Steps
1. User B creates/owns a resource `{id_b}`
2. User A directly accesses/modifies `{id_b}`
3. Observe response and auditing

### Expected Secure Behavior
- Return 403 or 404 (per project policy)
- Optionally do not leak resource existence
- Audit the unauthorized attempt (recommended)

---

## Scenario 2: Server-Side Filtering On List Endpoints

### Preconditions
- A list endpoint exists: `GET /api/v1/{resource}?...`

### Attack Objective
Verify list results include only resources visible to the current principal.

### Attack Steps
1. User A calls the list endpoint
2. Verify each returned resource satisfies ownership/permission constraints
3. Attempt bypass via filter params (for example passing another `owner_id`)

### Expected Secure Behavior
- Server enforces filtering; do not trust client filters
- Out-of-scope filters are rejected with 400 or ignored (per project policy)

---

## Scenario 3: Privilege Escalation (Vertical)

### Preconditions
- At least two roles exist (for example `user` and `admin`)

### Attack Objective
Verify low-privilege users cannot perform high-privilege operations.

### Attack Steps
1. Use a low-privilege token against admin endpoints (create/delete/config changes)
2. Attempt privilege escalation via request body fields (for example `role=admin`)
3. Call APIs backing "hidden in frontend only" buttons

### Expected Secure Behavior
- 403
- Server ignores/rejects self-reported privilege fields

---

## Scenario 4: Multi-Tenant Isolation (If Applicable)

### Preconditions
- The system has a tenant/org/workspace concept

### Attack Objective
Verify cross-tenant access is forbidden.

### Attack Steps
1. User A belongs to tenant 1
2. Obtain tenant 2 resource ids (logs, URLs, guessing, timing side channels)
3. User A attempts to access tenant 2 resources

### Expected Secure Behavior
- 403 or 404
- List endpoints do not leak other-tenant data

---

## Scenario 5: Admin Boundaries And Break-Glass Capabilities

### Preconditions
- Multiple admin tiers exist (for example workspace admin vs platform admin)

### Attack Objective
Verify boundaries are clear and high-risk operations have additional protection.

### Attack Steps
1. Lower-tier admin attempts platform-level operations
2. Higher-tier admin performs a high-risk operation and verify confirmation/audit behavior

### Expected Secure Behavior
- Boundary operations return 403
- High-risk operations have strong auditing (actor, target, IP, before/after values)
