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

Project-specific overlay: FR-097 provider session tokens are opaque runner-only values. Handoff/protobuf/Tauri/CLI responses may expose command-run references and session availability, but never the provider token, transcript, prompt, raw stdout/stderr, or unbounded error text. Include `handoff_snapshots`, `resume_plans`, `resume_executions`, control-plane audit, and daemon logs in the search described below.

FR-109 overlay: binding simulation and action audit expose only project/resource identifiers, trusted resolved role, stable reason codes, and normalized content revisions. The API has no message body or permalink input. Audit/log verification must reject Slack message URLs, bodies, tokens, rendered goals, or provider secrets. See `docs/qa/orchestrator/157-source-task-binding-badge-matching.md` Scenario 4.

FR-098/FR-102 overlay: session List/Get/Resolve responses may expose `session_id`, task relationships, canonical state, diagnostic PID, and lease metadata. They must never expose `cwd`, command text, `input_fifo_path`, `transcript_path`, stdout/stderr paths, output JSON paths, process fingerprints, input bytes, or unredacted transcript content. Search `session_control_actions`, `control_action_audit`, events, CLI JSON/JSONL, Tauri events, and daemon logs; input/transcript content must not be logged. Run `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md`, whose isolated script scans deterministic input markers.

FR-099 overlay: source List/Get/GUI responses may expose normalized provider/install identity, bounded summaries, correlation coordinates, hashes, routing state, and stable errors. They must not expose Slack signing secrets, raw request bodies, action-token signatures, private transcripts, or unbounded provider errors. Search `source_events`, `source_command_actions`, CLI/Tauri payloads, and daemon logs with `docs/qa/orchestrator/146-source-events-and-slack-binding.md`.

FR-107 overlay: reaction reads may additionally expose the closed event type, bounded normalized emoji name, target kind, and stable target ID. They must not expose the source message body, transcript, attachment/file content, target URL, Slack secret, or raw envelope. An ignored reaction card must not imply a task relationship. Run `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md`.

FR-108 overlay: SourceTaskTemplate preview may expose the trusted Skill descriptor, rendered bounded goal, action, revision/hash, and verification warnings. RuntimePolicy redaction must cover every public text field, including initial variables. Audit may retain only canonical hashes and resource identifiers; it must not contain the source URL, rendered goal, source body, provider secret, or sensitive configured values. Preview must not fetch provider content or create durable source/task rows. Run `docs/qa/orchestrator/156-source-task-template-skill-invocation.md`.

FR-103 overlay: `AttentionDelta.notification` may expose only a dedupe key, Attention item/version, bounded title, severity, process ID, and safe deep link. Native notification bodies and in-app fallback events must never include prompt, transcript, source message, stdout/stderr, token, API key, or raw error content. Run the forbidden-field scan in `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md`.

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
