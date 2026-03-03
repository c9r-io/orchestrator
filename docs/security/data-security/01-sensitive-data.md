# Data Security - Sensitive Data Exposure And Masking Tests (Generic)

**Module**: Data Security  
**Scope**: Sensitive data in API responses, logs, error messages, exports, and storage  
**Scenarios**: 5  
**Risk**: Critical  
**OWASP ASVS 5.0**: V11 Cryptography, V14 Data Protection, V16 Security Logging and Error Handling

---

## Background

Sensitive data includes (but is not limited to):
- Credentials: passwords, tokens, API keys, client_secret, private keys
- PII: email, phone, address, id numbers
- Internal details: DSNs, internal hosts, stack traces, configs

---

## Scenario 1: Sensitive Fields In API Responses

### Preconditions
- Identify endpoints involving users/credentials/configuration

### Attack Objective
Verify responses do not include fields that must not be returned (especially hashes/secrets/tokens).

### Attack Steps
1. Call relevant endpoints (profile, admin config, tokens, integrations)
2. Inspect JSON for sensitive fields

### Expected Secure Behavior
- Password hashes are never returned
- Secrets are not returned by default, or are returned masked (for example `****abcd`)
- Tokens are returned only on explicit issuance endpoints and are not leaked via logs/list endpoints

---

## Scenario 2: Error Responses And Stack Trace Leakage

### Preconditions
- Able to trigger 400/500

### Attack Objective
Verify error responses do not leak internal implementation details.

### Attack Steps
1. Send malformed JSON or wrong types
2. Trigger a non-existent resource
3. Inspect error bodies

### Expected Secure Behavior
- No stack traces, SQL, dependency addresses, or config values

---

## Scenario 3: Sensitive Data In Logs

### Preconditions
- Access to application logs (local `docker logs` or centralized logging)

### Attack Objective
Verify logs do not contain credentials, tokens, secrets, or plaintext passwords.

### Attack Steps
1. Perform login/token exchange/password change operations
2. Grep logs for keywords: `password`, `secret`, JWT prefix `eyJ`

### Expected Secure Behavior
- Authorization headers are masked
- Sensitive fields are not logged or are logged as `<REDACTED>`

### Verification
```bash
# Replace with the actual container name
docker logs {service_container} 2>&1 | rg -n "password|secret|eyJ|client_secret|private_key" || true
```

---

## Scenario 4: Export/Backup/Download (If Applicable)

### Preconditions
- Export/report/download/backup functionality exists

### Attack Objective
Verify exports are permissioned and masked as required, preventing data exfiltration beyond scope.

### Attack Steps
1. Attempt export with a low-privilege user
2. Inspect export file fields and record scope
3. Check if download links are guessable (IDOR)

### Expected Secure Behavior
- Exports require explicit privileges and auditing
- Download links have short TTLs or require authentication

---

## Scenario 5: Secret Management (Config And Environment)

### Preconditions
- Access to deployment configuration (k8s manifests, env, compose)

### Attack Objective
Verify secrets are not hardcoded in code/repo and are not exposed via logs or APIs.

### Attack Steps
1. Search the repo for `password=`, `secret=`, `BEGIN PRIVATE KEY`
2. Check whether config uses a secret store (Kubernetes Secret, Vault, etc.)
3. For orchestrator agents: verify secrets are managed via `SecretStore` resources rather than hardcoded in agent `command` templates or direct `env` values

### Expected Secure Behavior
- No hardcoded secrets
- Secrets are injected via secure mechanisms
- Orchestrator agents reference secrets through `SecretStore` resources (via `fromRef` or `refValue`), not literal values
- SecretStore values are redacted (`[REDACTED]`) in task execution logs (see `docs/qa/orchestrator/38-agent-env-resolution.md` Scenario 5)

### Verification
```bash
rg -n "BEGIN PRIVATE KEY|client_secret|api[_-]?key|password\\s*=" -S .
```

