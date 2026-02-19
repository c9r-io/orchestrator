---
name: ops
description: Run tests, check logs, and troubleshoot services in Docker Compose and Kubernetes environments. Use when debugging local dev env issues, inspecting container/pod logs, restarting services, checking health, or triaging deployment rollouts.
---

# Operations Skill

Troubleshoot projects that follow `project-bootstrap` conventions:
- Local: Docker Compose at `docker/docker-compose.yml`
- Reset: `./scripts/reset-docker.sh`
- K8s: `k8s/base/` and deploy scripts under `deploy/`

This skill should still work on other repos by discovering the correct compose file, namespace, and deployments.

## Quick Triage

1. Identify whether the failure is:
   - local (Docker Compose) or
   - cluster (Kubernetes)
2. Collect the smallest evidence set:
   - service status
   - recent logs
   - health endpoint (if any)
3. Apply the minimal fix:
   - restart one service
   - reset environment (only when state is likely dirty)
   - rollback/roll forward rollout

## Running Tests (Project-Specific)

Prefer existing project scripts.

Common examples:

```bash
# Rust backend (if core/ exists)
cd core && cargo test

# TypeScript frontend (if portal/ exists)
cd portal && npm test

# Playwright E2E (if configured)
cd portal && npx playwright test
```

## Docker Compose (Local)

### Status

```bash
docker compose -f docker/docker-compose.yml ps
```

### Logs

```bash
# all services
docker compose -f docker/docker-compose.yml logs --tail 200

# follow
docker compose -f docker/docker-compose.yml logs -f

# single service (replace <svc>)
docker compose -f docker/docker-compose.yml logs -f <svc>
```

### Restart

```bash
docker compose -f docker/docker-compose.yml restart <svc>
```

### Reset (Use Sparingly)

If data/state is likely dirty (flaky tests, schema changes, unknown env drift):

```bash
./scripts/reset-docker.sh
```

## Kubernetes (Cluster)

### Status

```bash
kubectl get pods -A
kubectl get deploy -A
kubectl get events -A --sort-by=.lastTimestamp | tail -50
```

### Logs

```bash
# by pod
kubectl logs -n <ns> <pod> --tail=200

# by deployment
kubectl logs -n <ns> deployment/<deploy> --tail=200
kubectl logs -n <ns> deployment/<deploy> -f

# all pods of a deployment (by label)
kubectl logs -n <ns> -l app.kubernetes.io/name=<app> -f
```

### Multi-container Scenarios

```bash
# Specific container in multi-container pod
kubectl logs -n <ns> deployment/<deploy> -c <container> -f

# All containers
kubectl logs -n <ns> deployment/<deploy> --all-containers -f
```

### Rollout Debug

```bash
kubectl rollout status -n <ns> deployment/<deploy> --timeout=300s
kubectl describe -n <ns> deployment/<deploy>
kubectl describe -n <ns> pod/<pod>
```

### Rollout Restart

```bash
kubectl rollout restart -n <ns> deployment/<deploy>
```

## Health Checks

If the project exposes health endpoints, verify them after restarts/rollouts.

```bash
curl -sSf http://localhost:8080/health
```

Replace URL/port based on the project's compose/k8s configuration.

## Troubleshooting Patterns

```bash
# Docker: filter error logs
docker logs <container> 2>&1 | grep -E "(ERROR|WARN|panic)"

# Docker: check cache connectivity (if Redis)
docker exec <redis-container> redis-cli ping

# K8s: check resource usage
kubectl top pods -n <ns>

# K8s: recent events sorted by time
kubectl get events -n <ns> --sort-by='.lastTimestamp' | tail -20
```

## Quick Reference

| Task | Docker | Kubernetes |
|------|--------|------------|
| Follow logs | `docker logs -f <container>` | `kubectl logs -f deploy/<deploy> -n <ns>` |
| Last 100 lines | `docker logs --tail 100 <container>` | `kubectl logs --tail=100 deploy/<deploy> -n <ns>` |
| Since 10 min | `docker logs --since 10m <container>` | `kubectl logs --since=10m deploy/<deploy> -n <ns>` |
| Restart | `docker compose restart <svc>` | `kubectl rollout restart deploy/<deploy> -n <ns>` |
| Shell access | `docker exec -it <container> /bin/sh` | `kubectl exec -it deploy/<deploy> -n <ns> -- /bin/sh` |

