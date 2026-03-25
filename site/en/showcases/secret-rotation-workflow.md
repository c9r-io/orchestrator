# Automated Secret Rotation Workflow Execution Plan

> **Agent Collaboration**: This document is an agent-executable plan. Open this project in an AI coding agent (Claude Code, OpenCode, Codex, etc.). After reading this plan, the agent orchestrates other agents via the orchestrator CLI to collaboratively complete the task — from resource deployment and task execution to result verification, all done autonomously.

## 1. Goal

Configure a cron trigger to periodically execute a secret sync workflow. The agent calls platform APIs to fetch the latest secrets and updates the SecretStore via `orchestrator apply`, achieving automated secret rotation.

## 2. Use Cases

- Periodic rotation of Slack Signing Secrets
- GitHub Webhook Secret rotation
- Automatic refresh of API tokens before expiration
- Any scenario requiring periodic SecretStore updates

## 3. Prerequisites

- orchestratord is running with `--webhook-bind` configured
- The corresponding platform integration package (`orchestrator-integrations`) is deployed
- The agent has permission to call platform APIs (API token is in the SecretStore)

## 4. Manifest Examples

### 4.1 Secret Rotation StepTemplate

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: rotate-secrets
spec:
  description: "Rotate platform signing secrets"
  prompt: >-
    Check the current signing secrets for all configured integrations.
    For each platform:
    1. Call the platform API to verify the current secret is still valid
    2. If the platform supports secret regeneration, generate a new secret
    3. Update the SecretStore via: orchestrator apply -f <updated-secrets.yaml>
    4. Verify the new secret works by sending a test webhook
    5. Report which secrets were rotated and which remain unchanged
```

### 4.2 Cron Trigger (Weekly Execution)

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: weekly-secret-rotation
spec:
  cron:
    schedule: "0 2 * * 0"    # Every Sunday at 2:00 AM
    timezone: "Asia/Tokyo"
  action:
    workflow: secret-rotation
    workspace: default
    start: true
```

### 4.3 Complete Workflow

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: secret-rotation
spec:
  max_parallel: 1
  steps:
    - id: rotate
      scope: task
      required_capability: plan
      template: rotate-secrets
      enabled: true
      repeatable: false
  loop:
    mode: fixed
    max_cycles: 1
```

## 5. Execution Flow

1. The cron trigger fires on schedule and creates a task
2. The agent executes the `rotate-secrets` step:
   - Reads the current SecretStore configuration
   - Calls platform APIs to verify/rotate secrets
   - Generates the updated SecretStore YAML
   - Updates the SecretStore via `orchestrator apply -f`
3. New secrets take effect immediately (the webhook handler reads the latest config on every request)
4. The task completes and the rotation results are recorded

## 6. Notes

- During secret rotation, both old and new secrets are kept in the SecretStore (multi-key rotation)
- Remove old secrets only after confirming the new secrets are valid
- It is recommended to run during low-traffic periods (set the cron schedule to early morning hours)
- The agent requires admin-level permissions for the platform API
