# Webhook Integration Template

> **Harness Engineering template**: this showcase demonstrates one concrete capability slice of orchestrator as a control plane for agent-first software delivery.
>
> **Purpose**: Webhook-driven external platform integration — demonstrates webhook triggers, per-trigger signature authentication, CEL payload filtering, CRD plugin system, and integration manifest packages.

## Use Cases

- Receive GitHub push/PR events, auto-trigger code review or security scan
- Receive Slack messages/commands, drive agent responses
- Receive LINE messages, drive customer service automation
- Any scenario requiring webhook callbacks to trigger agent workflows

## Prerequisites

- `orchestratord` running (webhook server enabled by default on `127.0.0.1:19090`; use `--webhook-bind <ADDR>` to change)
- Database initialized (`orchestrator init`)
- Optional: `orchestrator-integrations` repository cloned

## Steps

### 1. Deploy an Integration Package (GitHub Example)

```bash
# Clone the integrations repository
git clone https://github.com/c9r-io/orchestrator-integrations.git

# Prepare secrets
cp orchestrator-integrations/github/secrets-template.yaml secrets.yaml
# Edit secrets.yaml with your GitHub Webhook Secret
vim secrets.yaml

# Deploy resources
orchestrator apply -f secrets.yaml
orchestrator apply -f orchestrator-integrations/github/trigger-push.yaml
```

### 2. Configure GitHub Webhook

In your GitHub repository Settings > Webhooks:
- **Payload URL**: `http://<your-host>:19090/webhook/github-push`
- **Content type**: `application/json`
- **Secret**: must match the `webhook_secret` value in your SecretStore
- **Events**: `push`

### 3. Manual Test

```bash
# Simulate a webhook request with HMAC signature
SECRET="your-webhook-secret"
BODY='{"ref":"refs/heads/main","commits":[{"message":"test"}]}'
SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "$SECRET" -hex | awk '{print "sha256="$NF}')

curl -X POST http://127.0.0.1:19090/webhook/github-push \
  -H "Content-Type: application/json" \
  -H "X-Hub-Signature-256: $SIG" \
  -d "$BODY"
```

### 4. Check Results

```bash
orchestrator get task
orchestrator task logs <task_id>
```

## Key Features

### Per-Trigger Signature Authentication

Each trigger can independently configure its own SecretStore reference and signature header:

```yaml
kind: SecretStore
metadata:
  name: github-webhook
spec:
  data:
    webhook_secret: "your-github-secret"
---
kind: Trigger
metadata:
  name: github-push
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: github-webhook          # SecretStore reference
      signatureHeader: X-Hub-Signature-256  # Platform signature header
    filter:
      condition: "payload_ref != ''"     # CEL filter
  action:
    workflow: handle-push
    workspace: default
```

Signature verification supports multi-key rotation — all values in the SecretStore are tried, any match is accepted.

### CEL Payload Filtering

Use CEL expressions to match only the events you care about:

```yaml
# Match only main branch push
filter:
  condition: "payload_ref == 'refs/heads/main'"

# Slack: match only event_callback type
filter:
  condition: "payload_type == 'event_callback'"

# GitHub: match only PR opened
filter:
  condition: "payload_action == 'opened'"
```

Top-level fields from the webhook JSON body are automatically injected as `payload_<field>` CEL variables.

### CRD Plugin Authentication

For authentication scenarios beyond HMAC-SHA256 (e.g., Slack v0 signatures, custom token validation), use CRD plugins to define custom auth logic:

```yaml
kind: CustomResourceDefinition
metadata:
  name: slackintegrations.integrations.orchestrator.dev
spec:
  kind: SlackIntegration
  plural: slackintegrations
  group: integrations.orchestrator.dev
  versions:
    - name: v1
      schema:
        type: object
  plugins:
    # Custom signature verification — replaces built-in HMAC
    - name: verify-slack-v0
      type: interceptor
      phase: webhook.authenticate
      command: "scripts/verify-slack-v0-sig.sh"
      timeout: 5

    # Payload normalization — converts platform-specific format to standard JSON
    - name: normalize-payload
      type: transformer
      phase: webhook.transform
      command: "scripts/normalize-slack-payload.sh"
      timeout: 5

    # Periodic token refresh
    - name: refresh-token
      type: cron
      schedule: "0 */6 * * *"
      command: "scripts/refresh-slack-token.sh"
```

Link the trigger to CRD plugins via `crdRef`:

```yaml
kind: Trigger
metadata:
  name: slack-events
spec:
  event:
    source: webhook
    webhook:
      crdRef: SlackIntegration    # Enable CRD plugins
  action:
    workflow: handle-slack
    workspace: default
```

**Plugin execution flow**:

```
Webhook request arrives
  -> CRD interceptor (webhook.authenticate) — custom auth
  -> Parse JSON body
  -> CRD transformer (webhook.transform) — payload normalization
  -> CEL filter — event filtering
  -> Trigger workflow
```

### Plugin Policy Governance

CRD plugin commands execute as shell processes in the daemon context. To prevent privilege escalation, all plugin commands are subject to a **plugin policy** (`{data_dir}/plugin-policy.yaml`).

**Default behavior**: Allowlist mode with an empty allowlist — all plugin commands are **blocked** until you explicitly permit them.

Create `~/.orchestratord/plugin-policy.yaml`:

```yaml
mode: allowlist                          # deny | allowlist | audit
allowed_command_prefixes:
  - scripts/                             # permit scripts/ directory
  - /usr/local/bin/orchestrator-plugins/ # permit system-installed plugins
max_timeout_secs: 30                     # cap per-plugin timeout
enforce_on_hooks: true                   # also enforce on lifecycle hooks
```

**Policy modes**:

| Mode | Behavior |
|------|----------|
| `allowlist` | Only commands matching `allowed_command_prefixes` are accepted. Built-in denied patterns (curl, wget, nc, eval, base64) are always blocked. |
| `deny` | All CRDs with plugins are rejected. |
| `audit` | All commands are accepted, but violations are logged as warnings. Use for migration. |

**RBAC elevation**: Applying a CRD that contains plugins or lifecycle hooks requires **Admin** role (not Operator). With the default UDS transport, configure `uds-policy.yaml` to restrict agent access:

```yaml
# ~/.orchestratord/control-plane/uds-policy.yaml
max_role: operator   # agents cannot apply CRDs with plugins
```

**Audit trail**: Every plugin apply and execution is logged to the `plugin_audit` SQLite table:

```sql
SELECT created_at, action, crd_kind, plugin_name, command, result
FROM plugin_audit ORDER BY created_at DESC LIMIT 10;
```

### Built-in Tool Library

CRD plugin scripts can call `orchestrator tool` built-in utilities:

```bash
# HMAC signature verification
orchestrator tool webhook-verify-hmac \
  --secret "$SECRET" --body "$BODY" --signature "$SIG"

# JSON path extraction (reads from stdin)
echo '{"event":{"type":"message"}}' | \
  orchestrator tool payload-extract --path event.type

# Atomic SecretStore key update
orchestrator tool secret-rotate my-store my-key --value "new-secret"
```

## Available Integration Packages

| Platform | Signature Header | Repository Path |
|----------|-----------------|-----------------|
| GitHub | `X-Hub-Signature-256` | `orchestrator-integrations/github/` |
| Slack | `X-Slack-Signature` | `orchestrator-integrations/slack/` |
| LINE | `X-Line-Signature` | `orchestrator-integrations/line/` |

Each package includes:
- `secrets-template.yaml` — SecretStore template
- `trigger-*.yaml` — Pre-configured webhook triggers
- `step-template-*.yaml` — Optional payload parsing StepTemplates
- `README.md` — Platform-specific setup guide

## Customization

### Adding a New Platform Integration

1. Create a SecretStore for your platform's signing secret
2. Create a Trigger with webhook source, pointing to the SecretStore
3. If custom auth is needed, define a CRD with interceptor plugins

### Multiple Webhooks Side-by-Side

A single daemon can receive webhooks from multiple platforms simultaneously, each trigger independently authenticated:

```
POST /webhook/github-push     -> github-webhook SecretStore
POST /webhook/slack-events     -> slack-signing SecretStore + CRD plugins
POST /webhook/line-message     -> line-channel SecretStore
```

## Further Reading

- [Scheduled Scan Template](scheduled-scan) — Cron trigger example
- [FR Watch Template](fr-watch) — Filesystem trigger + CEL filtering example
- [Secret Rotation Workflow](secret-rotation-workflow) — Secret rotation example
