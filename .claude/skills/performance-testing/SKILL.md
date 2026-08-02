---
name: performance-testing
description: Run lightweight, repeatable latency and throughput checks against real repository surfaces; benchmark Orchestrator CLI/daemon or Slack gateway behavior here and use HTTP hey tests only for an explicitly running endpoint.
---

# Performance Testing

Benchmark the same revision, configuration, data shape, command, and transport before and after a change. Do not compare debug and release builds.

## Choose a Real Surface

For this repository:

- CLI/daemon latency: time a read-only command such as `orchestrator task list -o json` against the same daemon and data set.
- Scheduler behavior: use deterministic mock agents under `fixtures/manifests/bundles/`; do not use live AI benchmark manifests without explicit approval because they consume external credits.
- Slack gateway HTTP baseline: when `orchestrator-slack-gateway` is explicitly running, its health endpoint is `/healthz`.
- Web UI performance: use the tooling declared by `gui/package.json` and a production build when rendering cost is the target.

Use `hyperfine` for command latency when installed. Otherwise run a bounded shell loop with `/usr/bin/time` and preserve raw samples.

## HTTP Load Checks

Use `hey` only after the user identifies a reachable, non-production target and safe endpoint. Start with a cheap health endpoint and a low request/concurrency bound, then increase deliberately:

```bash
hey -n 200 -c 10 http://127.0.0.1:8080/healthz
```

Never guess API routes, credentials, or a Docker Compose environment. A generated project may carry `docker/docker-compose.yml`; a benchmark script is usable only when the target repository actually owns one.

## Evidence

Record:

- pinned git revision and release/debug build mode
- target command or URL, payload, data size, and transport
- sample count and concurrency
- throughput, p50/p90/p99 latency, and error rate
- warm-up policy and machine/load caveats

Treat a regression as actionable only after repeating both baseline and candidate under comparable conditions. Report raw evidence and avoid universal QPS ratings detached from the tested workload.
