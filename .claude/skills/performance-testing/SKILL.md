---
name: performance-testing
description: Run lightweight performance/load tests (QPS/latency) against project HTTP endpoints using hey, track regressions, and suggest optimizations. Use when the user asks to benchmark, load test, or measure performance after changes.
---

# Performance Testing

Use `hey` to measure throughput and latency for key endpoints. This skill is project-agnostic and works best when the project exposes health/ready endpoints and one or more critical API endpoints.

## Prerequisites

- `hey` installed (or installable)
- Services running locally (Docker Compose) or reachable in a cluster

## Quick Start

```bash
which hey || brew install hey
```

Start the environment (project-bootstrap default):

```bash
docker compose -f docker/docker-compose.yml up -d --build
```

If the backend is Rust and you are running it outside Docker, run in release mode:

```bash
cd core
cargo run --release
```

## Run Benchmarks

Set the target base URL:

```bash
BASE_URL=http://localhost:8080
```

Smoke benchmark on a cheap endpoint (prefer `/health` if present):

```bash
hey -n 2000 -c 50 "$BASE_URL/health"
```

Benchmark a representative API endpoint (replace path and method/body):

```bash
hey -n 2000 -c 50 -m GET "$BASE_URL/api/v1/items"
```

POST example:

```bash
hey -n 1000 -c 25 -m POST -H "Content-Type: application/json" \
  -d '{"name":"example"}' \
  "$BASE_URL/api/v1/items"
```

## Interpreting Results

Track:
- Throughput: requests/sec
- Latency: p50/p90/p99
- Error rate: non-2xx responses

If numbers regress after a change:
- confirm you are testing the same endpoint and payload
- confirm environment is clean (consider `./scripts/reset-docker.sh` if stateful)
- check logs for slow queries/timeouts
- profile hot paths (language/framework specific)

