# Deployment Pipeline Template

> **Purpose**: Build → test → deploy sequential pipeline — demonstrates ExecutionProfile isolation and safety configuration.

## Use Cases

- CI/CD automation: build, test, deploy three-stage pipeline
- Applying different filesystem/network isolation policies per step
- Safety circuit breaker (auto-stop on consecutive failures)

## Prerequisites

- `orchestratord` is running
- Database initialized (`orchestrator init`)

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/deployment-pipeline.yaml --project deploy
```

### 2. Create and Run a Task

```bash
orchestrator task create \
  --name "deploy-v1" \
  --goal "Deploy version 1.0" \
  --workflow deployment_pipeline \
  --project deploy
```

### 3. Inspect Results

```bash
orchestrator task list --project deploy
orchestrator task logs <task_id>
```

## Workflow Steps

```
build (sandbox) → test (host) → deploy (host)
```

1. **build** — Runs in sandbox mode; only `build/` and `dist/` are writable
2. **test** — Runs in host mode with full access for the test runner
3. **deploy** — Runs in host mode for deployment and health checks

### Key Feature: ExecutionProfile

Different steps use different isolation levels:

```yaml
kind: ExecutionProfile
metadata:
  name: sandbox_build
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  writable_paths:
    - build
    - dist
  network_mode: inherit
```

- `sandbox` mode: restricts filesystem write scope, protecting source code
- `host` mode: full permissions for steps requiring system tools
- `network_mode: inherit`: inherits host network (agents need API access)

### Key Feature: Safety Configuration

```yaml
safety:
  max_consecutive_failures: 1
  auto_rollback: false
```

Stops the workflow after 1 consecutive failure, preventing deployment when the build fails.

## Customization Guide

### Add an Approval Step

Insert a manual approval between test and deploy:

```yaml
- id: approval
  type: approval
  scope: task
  required_capability: review
  enabled: true
```

### Adjust Sandbox Permissions

Modify `writable_paths` for your project structure:

```yaml
writable_paths:
  - target          # Rust projects
  - node_modules    # Node.js projects
  - dist
```

### Enable Auto-Rollback

```yaml
safety:
  max_consecutive_failures: 1
  auto_rollback: true
```

## Further Reading

- [Self-Bootstrap Execution](/en/showcases/self-bootstrap-execution-template) — Production workflow with ExecutionProfile
- [Advanced Features](/en/guide/advanced-features) — ExecutionProfile and safety details
- [Workflow Configuration](/en/guide/workflow-configuration) — Step execution model
