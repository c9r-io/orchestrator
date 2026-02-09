# Authorization Security - Access Control And Privilege Tests (Generic)

**Module**: Authorization  
**Scope**: IDOR, horizontal/vertical privilege escalation, admin boundaries, multi-tenant isolation (if applicable)  
**Scenarios**: 5  
**Risk**: Critical  
**OWASP ASVS 5.0**: V8 Authorization

---

## Background

Access control issues are often not "missing authentication", but:
- Missing resource-level authorization (IDOR)
- List endpoints missing server-side filtering
- Unclear role boundaries leading to privilege escalation
- Multi-tenant isolation gaps enabling cross-tenant access

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

