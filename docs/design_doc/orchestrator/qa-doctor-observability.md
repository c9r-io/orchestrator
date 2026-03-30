# Design Doc: QA Doctor Observability CLI

## Summary

Implements `orchestrator qa doctor` — a CLI subcommand that exposes three
observability metrics derived from the existing `task_execution_metrics` table:

| Metric | Source |
|--------|--------|
| `task_execution_metrics_total` | `COUNT(*)` over entire table |
| `task_execution_metrics_last_24h` | `COUNT(*)` where `created_at >= now - 24h` |
| `task_completion_rate` | `COUNT(status='completed') / COUNT(*)`, 0.0 when empty |

## Motivation

QA-21 Scenario 5 requires verifying that task execution metrics are queryable
through the CLI. The `task_execution_metrics` table was already populated by the
scheduler, but no read path existed.

## Architecture

The implementation follows the standard four-layer pattern used by all
orchestrator CLI commands:

```
CLI (clap)  →  gRPC client  →  Daemon handler  →  Core query
qa.rs          proto           system.rs          qa_doctor.rs
```

- **Proto:** `QaDoctor` RPC with empty request and three-field response.
- **Core:** `qa_doctor::qa_doctor_stats()` — three `SELECT COUNT(*)` queries
  via `AsyncDatabase::reader()`.
- **Daemon:** `system::qa_doctor()` — authorize + call core + map to proto.
- **CLI:** `qa::dispatch()` with table and JSON output (YAML unsupported,
  matching `db` command precedent).

## Output formats

**JSON** nests under `"observability"`:
```json
{
  "observability": {
    "task_execution_metrics_total": 123,
    "task_execution_metrics_last_24h": 45,
    "task_completion_rate": 0.87
  }
}
```

**Table** uses fixed-width columns:
```
METRIC                                   VALUE
task_execution_metrics_total             123
task_execution_metrics_last_24h          45
task_completion_rate                     0.87
```

## FR Reference

FR-088
