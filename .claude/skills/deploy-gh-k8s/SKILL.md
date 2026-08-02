---
name: deploy-gh-k8s
description: Gate and execute a Kubernetes deployment only when the target repository actually carries the project-bootstrap deployment contract. Use for deploy, release, go-live, or Kubernetes rollout requests.
---

# Deploy GH K8s

Deploy only after proving that the target repository owns a Kubernetes deployment contract.

## Applicability Gate

From the target repository root, require all of these before using Kubernetes:

```bash
test -x deploy/upgrade.sh
test -d k8s/base
kubectl config current-context
```

`deploy/upgrade.sh` and `k8s/base/` are outputs of the `project-bootstrap` template; they are not present in the Agent Orchestrator repository. When either path is absent, report the Kubernetes portion as not applicable. Do not invent manifests, copy hidden template assets into the repository, or treat a successful Rust build as a deployment.

## Workflow

1. Resolve the branch and inspect the latest relevant GitHub Actions run with `gh run list` and `gh run view`.
2. Stop before deployment when required CI is not successful. Diagnose and fix only when the user also authorized implementation.
3. Confirm the current Kubernetes context, namespace, and intended release revision with the user-visible evidence.
4. Run the repository-owned upgrade script:

   ```bash
   ./deploy/upgrade.sh
   ```

5. Monitor rollout state with `kubectl get`, `kubectl rollout status`, events, and bounded log tails.
6. Run the repository-owned readiness or health checks and report the exact deployed revision.

## Agent Orchestrator Repository

This repository ships local binaries and an optional Slack gateway but no `deploy/` or `k8s/` tree. For a release-readiness request here, use `project-readiness`; for daemon operations, use `ops`. Deployment requires a separately supplied target or deployment contract.

## Guardrails

- Never switch Kubernetes context or namespace implicitly.
- Never deploy after failed or pending required CI.
- Prefer bounded logs and exact resource names.
- A failed rollout is a failure, even if some pods are healthy.
