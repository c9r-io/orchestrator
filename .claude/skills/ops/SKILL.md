---
name: ops
description: Run tests, inspect daemon and task state, collect logs, and troubleshoot the current repository; use Docker Compose or Kubernetes only when the target repo actually declares those environments.
---

# Operations

Discover the runtime contract before acting. In this repository the default operational surface is a host-running `orchestratord` daemon, not Docker Compose or Kubernetes.

## Agent Orchestrator Triage

1. Inspect repository and daemon state:

   ```bash
   git status --short
   orchestrator daemon status
   orchestrator debug --component daemon
   ```

2. Inspect task state and bounded logs:

   ```bash
   orchestrator task list
   orchestrator task logs <task-id> -n 200
   ```

3. The default runtime root is `~/.orchestratord/`; respect `ORCHESTRATORD_DATA_DIR` when it is set. Prefer CLI queries over reading SQLite or killing a PID directly.
4. For source failures, run the narrowest relevant test first, then broaden to `cargo test --workspace`.
5. For Web UI failures, use scripts from `gui/package.json`, normally `npm test` or `npm run test:e2e` from `gui/`.
6. Use `orchestrator daemon stop` for a graceful stop. Restart only when the user asked for a state-changing operational action.

## Optional Container or Cluster Branch

Use Docker Compose only when a compose file exists in the target repository. Use Kubernetes only when a manifest or kustomization tree exists and the user has placed that cluster in scope. When neither exists, mark those checks not applicable instead of substituting assets from `project-bootstrap`.

For an applicable generated project, the conventional files are `docker/docker-compose.yml`, `scripts/reset-docker.sh`, and `k8s/base/`. Confirm each exact path before running it.

## Evidence and Guardrails

- Collect status, the smallest relevant log tail, and a health or CLI check.
- Never delete the daemon database or runtime directory during routine triage.
- Never send an unscoped signal from a stale PID file.
- Do not restart every service when one bounded component is failing.
- Report the failing command, exit code, and relevant evidence rather than claiming recovery from process existence alone.
