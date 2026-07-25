---
lifecycle: active
---

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

FR-116 extends that boundary to explicit Agent drivers. `SessionRef` has no serialization/display contract; provider `session_id`, `thread_id`, and equivalent nested fields must be replaced before raw artifact persistence. `driver_started` may expose only `session_available`, and tool/assistant events must pass through SecretStore/runner redaction before insertion. Provider material must not enter `command_runs.session_id`, gRPC/DTOs, Attention detail, or Action Audit. `driver.rawArgs` must never contain credentials and is allowed only behind unsafe mode, Admin authorization, explicit acknowledgement, and the canonical `agent.driver.raw_args.apply` audit action. Run `docs/qa/orchestrator/164-agent-driver-abstraction.md` and `scripts/qa/test-agent-driver-abstraction.sh` for the deterministic privacy scan. Codex resume certification additionally uses an isolated temporary `CODEX_HOME`, replaces live thread/item identifiers before fixture output, rejects UUIDs in the committed fixture, and destroys its copied `auth.json` on every exit path; see `docs/qa/orchestrator/166-codex-session-resume-conformance.md`.

FR-109 overlay: binding simulation and action audit expose only project/resource identifiers, trusted resolved role, stable reason codes, and normalized content revisions. The API has no message body or permalink input. Audit/log verification must reject Slack message URLs, bodies, tokens, rendered goals, or provider secrets. See `docs/qa/orchestrator/157-source-task-binding-badge-matching.md` Scenario 4.

FR-110 overlay: the outbound Slack token is resolved from SecretStore only inside the daemon provider adapter. Route persistence may retain the protected validated permalink and credential store/key names, but never the credential value or provider response body. Read-only source projections and semantic timeline events omit the permalink; `SourceAutomationRouteGet` returns it only to `operator+`. The canonical action audit stores route/source/binding/template/task identifiers and hashes, not the permalink, rendered goal, raw webhook body, message body, or token. Run the full redaction/provenance scan in `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md` Scenario 2.

FR-111 overlay: operational list/get/watch/status and route Attention may expose stable route/source/task references, binding/template identity, safe state/error family, generation/version, attempt budget, and bounded timing metadata. They must omit installation/message coordinates, permalink, rendered goal, lease token, credential value, raw provider error/body, and Slack message content. Process metrics may label only allowlisted provider/result/error families; retention expires protected permalinks and old attempt/change detail while preserving audit provenance. See `docs/qa/orchestrator/159-source-automation-reliability-operations.md` Scenarios 2-4.

FR-112 overlay: the GUI catalog may expose configured installation ID, Trigger name, actor identifiers/roles, template/binding fields, and normalized revisions, but never SecretStore references/values or protected permalinks. `SourceEvent` Tauri projection must omit `normalized_json`; rendered goal and sample permalink live only in the active preview state and are never persisted to DOM-backed storage. Scan Tauri snapshots, DOM, local/session storage, audits, and logs with `docs/qa/orchestrator/160-process-console-source-automation-ui.md` Scenario 4.

FR-113 release overlay: retained aggregate evidence may include gate names, owners, durations, route/task identifiers, stable status/error families, migration versions, and a local artifact path. It must not include signing secrets, outbound credentials, raw Slack bytes, message URLs, rendered goals, or task/source content. Compatible rollback preserves the protected database rows instead of exporting them. Run `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md` Scenarios 3-5.

FR-114 overlay: official app client/signing secrets and installation bot tokens exist only encrypted in the independent Gateway store; the daemon persists only an encrypted installation pairing envelope. OAuth code/state, poll/pairing secret, raw Slack body, workspace name, private URL, and token must be absent from daemon config, protobuf/CLI/Tauri safe projections, GUI DOM/storage, tasks, metrics, audit, and routine logs. Gateway/daemon backup evidence must not export decrypted rows. Scan both processes and browser boundaries with `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md` Scenarios 1, 4, and 5.

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
