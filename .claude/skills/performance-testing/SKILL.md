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

If the project has a `scripts/benchmark.sh`, use it:

```bash
# Quick test (lower concurrency range)
./scripts/benchmark.sh

# Full test (higher concurrency range)
./scripts/benchmark.sh full

# Custom target URL
BASE_URL=http://your-server:8080 ./scripts/benchmark.sh
```

Otherwise, run `hey` directly. Set the target base URL:

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

## Recommended Test Endpoints

| Endpoint | Purpose | Measures |
|----------|---------|----------|
| `/health` | Pure compute | Framework baseline QPS |
| `/ready` | DB + Cache | I/O bound performance |
| `/api/v1/{resource}` | Business logic | Real API performance |

## Output Metrics

- **Max Stable QPS**: Highest throughput before degradation
- **Best Concurrency**: Optimal concurrent connections
- **P50/P99 Latency**: Response time percentiles

## Performance Rating

| Rating | /health QPS | Notes |
|--------|-------------|-------|
| Excellent | > 30,000 | Optimal performance |
| Good | > 10,000 | Production ready |
| Fair | > 5,000 | Check bottlenecks |
| Low | < 5,000 | Likely debug mode |

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

## Troubleshooting

| Issue | Solution |
|-------|----------|
| Low QPS | Ensure `--release` flag (Rust), check resource usage |
| Service not running | Start via Docker Compose or `cargo run --release` |
| DB/Cache issues | `docker compose ps`, restart services |

