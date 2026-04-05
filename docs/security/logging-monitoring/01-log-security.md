# Logging And Monitoring - Log Security And Audit Coverage Tests (Generic)

**Module**: Logging And Error Handling  
**Scope**: Log injection, audit completeness, sensitive data in logs  
**Scenarios**: 5  
**Risk**: High  
**OWASP ASVS 5.0**: V16 Security Logging and Error Handling

---

## Background

Logging security issues can lead to:
- Erasing or forging attack traces
- Inability to investigate security incidents
- Sensitive data exposure via logging systems

---

## Scenario 1: Log Injection (CRLF/ANSI/Formatting)

### Preconditions
- User-controlled data is logged (body/query/header)
- You can view application logs

### Attack Objective
Verify user input cannot forge new log lines or structured fields.

### Attack Steps
1. Inject `%0a` / `%0d%0a` (newlines) into fields
2. Inject forged log fragments into headers (for example User-Agent)
3. Inject ANSI escape sequences to confuse output (for example `\x1b[31m`)

### Expected Secure Behavior
- With structured logs (JSON recommended), user input is stored only as a field value
- CRLF does not create new log lines
- ANSI sequences are escaped or filtered

---

## Scenario 2: Audit Log Coverage And Immutability

### Preconditions
- You can perform security-sensitive actions (create/delete/grant/config changes, etc.)

### Attack Objective
Verify critical actions are written to audit logs and there is no API to delete/modify audits.

### Attack Steps
1. Perform 3-5 sensitive operations
2. Confirm audit entries exist (actor, target, IP, time, before/after values)
3. Probe for any audit-log delete/update APIs

### Expected Secure Behavior
- Audit coverage is complete for sensitive actions
- No interface exists to delete/modify audit records

### Orchestrator-Specific Notes
- All RPC authorization decisions (allowed and denied) are persisted to `control_plane_audit` in SQLite.
- TCP audit records include `tls_fingerprint` (SHA256 of client certificate) and `subject_id` (URI SAN).  UDS audit records include `peer_exe` (resolved executable path, forensic only) and `role` (effective role from policy or implicit Admin).
- `audit_all_reads: true` in `uds-policy.yaml` enables full audit coverage including read-only RPCs.
- See `docs/qa/orchestrator/58-control-plane-security.md` Scenario 6 for verification steps.

---

## Scenario 3: Sensitive Data Must Not Appear In Logs

### Preconditions
- Access to log output

### Attack Objective
Verify logs do not record plaintext passwords, tokens, secrets, or raw Authorization headers.

### Attack Steps
1. Perform login, token exchange, password change
2. Grep logs for: `password`, `secret`, JWT prefix `eyJ`
3. For orchestrator task logs: execute an agent that references a `SecretStore` via `fromRef` or `refValue`, then verify the secret values are replaced with `[REDACTED]` in captured stdout/stderr (see `docs/qa/orchestrator/38-agent-env-resolution.md` Scenario 5)

### Expected Secure Behavior
- All sensitive values are masked or not logged at all
- Orchestrator `SecretStore` values are automatically collected by `collect_sensitive_values()` and redacted via `redact_text()` in task output logs

### Verification
```bash
docker logs {service_container} 2>&1 | rg -n "password|secret|client_secret|eyJ|Authorization:" || true
```

---

## Scenario 4: Error Logs Must Not Leak Config Or Internal Addresses

### Preconditions
- Able to trigger 500/errors

### Attack Objective
Verify error logs do not include DSNs, redis_url, private keys, internal hosts.

### Verification
```bash
docker logs {service_container} 2>&1 | rg -n "dsn|database_url|redis_url|BEGIN PRIVATE KEY|AKIA" || true
```

---

## Scenario 5: Security Detection And Alerting (If Applicable)

### Preconditions
- The project includes detection/alerting (brute force, anomalous logins, authz-denied alerts, etc.)

### Attack Objective
Verify detection thresholds and alert delivery work.

### Attack Steps
1. Trigger brute force or enumeration patterns
2. Check whether events are recorded and alerts fire

### Expected Secure Behavior
- At minimum, events are logged/audited
- Alerts are generated if the project requires them

