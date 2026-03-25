# Design Doc 88: Integration Manifest Packages

## FR Reference

FR-082

## Design Decisions

### Separate Repository

Integration manifests live in `c9r-io/orchestrator-integrations`, not in the main repo:
- Keeps core repo focused on the orchestration engine
- Integration packages can be versioned independently
- Users clone/copy only the integrations they need

### Standard Package Structure

Each integration follows a consistent layout:
- `secrets-template.yaml` — SecretStore with placeholder values
- `trigger-*.yaml` — Pre-configured webhook triggers with per-trigger auth + CEL filter
- `step-template-*.yaml` — Optional payload parsing templates
- `README.md` — Platform-specific setup guide (app creation, secret retrieval, webhook URL config)

### Per-Trigger Auth (from FR-081)

All trigger manifests use `webhook.secret.fromRef` for platform-specific signature verification:
- Slack: `X-Slack-Signature` header, HMAC-SHA256
- GitHub: `X-Hub-Signature-256` header, HMAC-SHA256
- LINE: `X-Line-Signature` header, HMAC-SHA256

### Secret Rotation Showcase

A separate showcase document (`docs/showcases/secret-rotation-workflow.md`) demonstrates automated key rotation using a cron trigger + agent workflow.

## Integrations Delivered

| Platform | Triggers | Secret Header |
|----------|----------|---------------|
| Slack | message, slash-command | X-Slack-Signature |
| GitHub | push, pr-opened, issue-comment | X-Hub-Signature-256 |
| LINE | message | X-Line-Signature |
