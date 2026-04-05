---
name: integration-authoring
description: Create or update integration manifest packages in the companion repo (orchestrator-integrations). Use when a user asks to add a new integration (Slack, GitHub, Discord, Jira, etc.), create a webhook trigger for an external service, or extend the integrations library. Routes all integration work to the separate repo while reusing the main project's SDLC infrastructure.
---

# Integration Authoring

Create, update, or extend integration manifest packages in the companion `orchestrator-integrations` repository.

## Repository Layout

```
<org-root>/
├── orchestrator/                  # Main project (this repo)
│   ├── crates/                    # Core engine code
│   ├── docs/showcases/            # Execution plans
│   └── .claude/skills/            # Skills (including this one)
└── orchestrator-integrations/     # Integration packages (companion repo)
    ├── slack/
    ├── github/
    ├── line/
    └── <new-integration>/
```

## When to Use This Skill

- User asks to "add a Jira integration", "create a Discord webhook trigger", etc.
- User asks to update an existing integration (e.g., "add a new GitHub trigger for releases")
- User asks to create a CRD-based integration extension

## Workflow

### 1. Determine Scope

- **New integration**: Create a new directory in `orchestrator-integrations/`
- **Update existing**: Modify files in the existing integration directory
- **CRD extension**: Create CRD manifest + associated triggers (FR-083 pattern)

### 2. Create/Update Integration Package

Work in the companion repo at `../orchestrator-integrations/` (relative to this project root).

Each integration package MUST follow this structure:

```
<platform>/
├── secrets-template.yaml     # SecretStore with placeholder values
├── trigger-<event>.yaml      # One Trigger per event type
├── step-template-<action>.yaml  # Optional StepTemplate for payload handling
└── README.md                 # Platform setup guide
```

### 3. Manifest Standards

**SecretStore template:**
```yaml
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: <platform>-secret
spec:
  data:
    signing_secret: "<your-<platform>-secret>"
```

**Webhook Trigger:**
```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: <platform>-<event>
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: <platform>-secret
      signatureHeader: <platform-specific-header>
    filter:
      condition: "<CEL expression for event filtering>"
  action:
    workflow: handle-<platform>-<event>
    workspace: default
```

### 4. README Standards

Each integration README MUST include:

1. **Prerequisites** — orchestrator version, webhook server (enabled by default on `127.0.0.1:19090`)
2. **Platform Setup** — step-by-step guide to create app/webhook on the platform
3. **Secret Retrieval** — where to find the signing secret/API key
4. **Webhook URL** — exact URL format: `http://<host>:<port>/webhook/<trigger-name>`
5. **Apply Commands** — complete `orchestrator apply -f` commands

### 5. Validate Manifests

After creating/updating manifests:

```bash
# Validate syntax (from the main orchestrator project)
for f in ../orchestrator-integrations/<platform>/*.yaml; do
  orchestrator manifest validate -f "$f" 2>&1 || true
done
```

Note: Triggers may fail validation if referenced workflows don't exist — that's expected. The important thing is no YAML parse errors or unknown field errors.

### 6. Commit to Companion Repo

```bash
cd ../orchestrator-integrations
git add -A
git commit -m "feat: add <platform> integration — <description>"
git push
```

### 7. Update Main Project References (if needed)

If the new integration introduces a pattern worth showcasing:
- Add a showcase doc in `docs/showcases/` (main repo)
- Update `site/` with EN/ZH versions
- Update VitePress sidebar if adding new showcase pages

## Integration Checklist

- [ ] `secrets-template.yaml` with placeholder values (never real secrets)
- [ ] Trigger manifest(s) with `source: webhook` + per-trigger auth
- [ ] CEL filter expression for event type filtering
- [ ] Platform-appropriate signature header
- [ ] README with complete setup guide
- [ ] Manifests parse without YAML errors
- [ ] Committed and pushed to `orchestrator-integrations` repo

## Available Integrations (Current)

| Platform | Directory | Triggers |
|----------|-----------|----------|
| Slack | `slack/` | message, slash-command |
| GitHub | `github/` | push, pr-opened, issue-comment |
| LINE | `line/` | message |
