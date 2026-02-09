---
name: reset-local-env
description: Reset the local Docker Compose development environment to a clean state (rebuild images, drop volumes).
---

# Reset Local Environment Skill (Docker Compose)

Reset a local Docker Compose environment to a clean state.

## When to Use

Use this when:
- Need to reset local environment
- Need to clean Docker state
- Fixing dirty data issues
- Starting fresh with a clean development setup
- Encountering persistent errors after code changes
- Database schema changes require clean migration
- Switching between branches with incompatible changes

---

## Quick Start

Run the reset script to get a clean local environment:

```bash
./scripts/reset-docker.sh
```

This script performs:
1. Stop and remove all containers
2. Remove project images (force rebuild)
3. Remove volumes (clean data)
4. Build all images from scratch
5. Start all services

---

## Assumptions / Conventions

This skill assumes the project was bootstrapped with `project-bootstrap`, which generates:
- `docker/docker-compose.yml`
- `scripts/reset-docker.sh` (calls Compose using `docker/docker-compose.yml`)

---

## Manual Steps (if script unavailable)

```bash
cd /path/to/project

# Stop and remove containers
docker compose -f docker/docker-compose.yml down --remove-orphans

# Remove images
docker rmi <project>-core <project>-portal

# Remove volumes
docker volume ls

# Rebuild and start
docker compose -f docker/docker-compose.yml build --no-cache
docker compose -f docker/docker-compose.yml up -d

# Wait for services
sleep 30
docker compose -f docker/docker-compose.yml ps
```

---

## Service Ports (Project-Specific)

Ports and credentials are project-specific. Use the generated `docker/docker-compose.yml` (and any `.env`) as the source of truth.

---

## Verification Steps

After reset, verify services are healthy:

```bash
# Check all containers are running
docker compose -f docker/docker-compose.yml ps

# If your services expose health endpoints, test them here.
# Example:
# curl http://localhost:8080/health
```

---

## Common Issues

### Containers won't start

```bash
# Check logs
docker compose -f docker/docker-compose.yml logs

# Check specific service
docker logs <container-name>
```

### Port conflicts

```bash
# Check what's using the ports
lsof -i :3000
lsof -i :8080
lsof -i :4000

# Kill conflicting processes or change ports in docker-compose.yml
```

### Volume permission issues

```bash
# Remove volumes with sudo if needed
docker volume ls
sudo docker volume rm <volume-name>
```
