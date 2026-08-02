---
name: grpc-regression
description: Run gRPC regression checks through the target repository's real transport; use the Orchestrator CLI for its UDS daemon and Docker-network grpcurl helpers only when Compose is actually present.
---

# gRPC Regression

Choose the transport from the repository, not from the skill name.

## Agent Orchestrator Repository

The daemon uses `~/.orchestratord/orchestrator.sock` by default, overridable by `ORCHESTRATORD_DATA_DIR` or `ORCHESTRATOR_SOCKET`. Exercise RPC behavior through the `orchestrator` CLI unless the daemon was explicitly bound to TCP. Start with:

```bash
orchestrator daemon status
orchestrator debug --component daemon
orchestrator check
```

Use `crates/proto/orchestrator.proto` as the canonical service definition. For a specific RPC, prefer the matching CLI command and assert both success and its expected error category.

## Docker Network Branch

The shared helpers `.claude/skills/tools/grpc-smoke.sh` and `.claude/skills/tools/grpcurl-docker.sh` are for a target that actually has a Compose network. Confirm its compose file and network before use; `docker/docker-compose.yml` is a generated-project convention, not a file in this repository.

Recommended checks when that branch applies:

1. Missing credentials fail with the expected authentication status.
2. Valid credentials cross the transport boundary.
3. Wrong client certificates fail the TLS handshake.
4. Reflection-disabled services are called with the target's mounted canonical proto.

Never start containers or pull a grpcurl image merely to test the local UDS daemon.
