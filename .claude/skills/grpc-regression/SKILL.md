---
name: grpc-regression
description: Run grpcurl checks from inside the Docker Compose network (useful when host-to-container ports are blocked).
---

# gRPC Regression Skill (grpcurl inside Docker network)

When host -> container gRPC access is blocked or flaky, run `grpcurl` from an ephemeral Docker container on the same Docker Compose network.

## Quick Start

```bash
# Smoke checks (customize via env vars)
.claude/skills/tools/grpc-smoke.sh
```

## One-off grpcurl (via Docker network)

```bash
# Example: list services (works only if server reflection is enabled)
.claude/skills/tools/grpcurl-docker.sh -plaintext my-grpc:50051 list

# Example: call a method using a mounted proto
.claude/skills/tools/grpcurl-docker.sh \
  -import-path /proto -proto my.proto \
  -d '{"hello":"world"}' \
  my-grpc:50051 my.pkg.Service/MyMethod
```

## Hardened gRPC Recipe (mTLS + API key + no reflection)

Use this pattern when reflection is disabled and service auth is enabled:

```bash
.claude/skills/tools/grpcurl-docker.sh \
  -cacert /certs/ca.crt -cert /certs/client.crt -key /certs/client.key \
  -H "x-api-key: ${GRPC_API_KEY:-dev-grpc-api-key}" \
  -import-path /proto -proto my.proto \
  -d '{"identity_token":"dummy","tenant_id":"dummy","service_id":"dummy"}' \
  my-grpc:50051 my.pkg.Service/MyMethod
```

Recommended negative/positive regression checks:

1. Missing API key should fail with auth error.
2. API key present should pass gateway auth (business validation may still fail).
3. Wrong client cert should fail TLS handshake.

## Environment Variables

- `GRPC_NETWORK`: Docker network name. If unset, auto-detect from running Compose containers (fallback: require explicit `GRPC_NETWORK`).
- `GRPC_COMPOSE_FILE`: Compose file path used for auto-detect (defaults to `docker/docker-compose.yml` if present).
- `GRPC_IMAGE`: grpcurl image (default: `fullstorydev/grpcurl`)
- `GRPC_MOUNT_PROTO`: Host path to mount to `/proto:ro` (optional). Relative paths are resolved from repo root.
- `GRPC_API_KEY`: Optional API key value for secured endpoints.
