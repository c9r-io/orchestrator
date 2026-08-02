---
name: project-readiness
description: Verify release readiness from the current repository's actual Rust workspace, React/Tauri GUI, governance gates, and GitHub Actions; add Docker or Kubernetes checks only when those assets exist.
---

# Project Readiness

Produce a revision-pinned, evidence-backed readiness verdict. A missing deployment surface is `not applicable`, not an implicit pass and not permission to synthesize infrastructure.

## Agent Orchestrator Baseline

1. Record revision and worktree state with `git rev-parse HEAD` and `git status --short`.
2. Run the local readiness helper:

   ```bash
   .claude/skills/project-readiness/scripts/check_local.sh
   ```

3. Verify the Rust workspace with the repository's required commands: formatting check, workspace tests, and Clippy with warnings denied.
4. Verify the Web UI from `gui/` with the scripts declared in `gui/package.json`; include build and tests, and E2E when UI behavior is in scope.
5. Run `scripts/qa-doc-lint.sh` so documentation and governance contracts are included.
6. Inspect the latest relevant GitHub Actions run with `gh`, and reconcile its required gates with the pinned local revision.

The helper does not reset data, start a daemon, or contact a cluster. State-changing smoke checks require explicit scope.

## Optional Generated-Project Checks

If the target repository contains `docker/docker-compose.yml`, `scripts/reset-docker.sh`, `k8s/base/`, or `deploy/`, verify those exact surfaces. These are `project-bootstrap` outputs and are absent from the Agent Orchestrator repository.

- Compose: render config, start only when authorized, check declared health state, and capture bounded logs.
- Kubernetes: prefer offline kustomize/render checks before any cluster action.
- Deployment scripts: inspect supported dry-run behavior before execution.

## Verdict

Report PASS, FAIL, or NOT APPLICABLE for:

- revision/worktree
- Rust format, tests, and lint
- Web UI tests and build
- governance docs/gates
- GitHub Actions
- runtime smoke checks
- container/Kubernetes surfaces

For every failure, include the exact command, exit code, smallest useful error excerpt, and rerun command.
