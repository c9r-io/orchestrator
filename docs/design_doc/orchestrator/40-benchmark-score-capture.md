# Design Doc #40: Benchmark Score Capture (FR-028)

## Status

Implemented

## Context

`self-evolution` workflow relies on `evo_benchmark` producing a numeric score per candidate and `item_select` choosing the highest `score`.

Before this change, [`docs/workflow/self-evolution.yaml`](docs/workflow/self-evolution.yaml) captured:

```yaml
- var: score
  source: exit_code
```

That was incorrect for agent-backed benchmark steps because the process exit code only expresses success or failure, not the benchmark's `total_score`. As a result, both candidates collapsed to the same score and `item_select` deterministically picked the first item.

## Decision

Extend capture declarations with an optional `json_path` and apply it to `stdout`/`stderr` captures.

## Design

### Capture Schema

[`core/src/config/step.rs`](core/src/config/step.rs) now defines:

- `CaptureDecl.var`
- `CaptureDecl.source`
- `CaptureDecl.json_path: Option<String>`

The field is optional and omitted by default, so existing workflows keep their current behavior.

### Runtime Extraction

[`core/src/scheduler/item_executor/accumulator.rs`](core/src/scheduler/item_executor/accumulator.rs) now:

1. preserves legacy raw capture behavior when `json_path` is absent
2. for `stdout`/`stderr + json_path`, resolves stream-json `result` payloads first
3. parses the resulting JSON and extracts the requested field through `json_extract::extract_field`
4. logs a warning and stores an empty string when the field is missing or the text is not valid JSON

This keeps the scheduler non-fatal for malformed agent output while allowing downstream selection logic to reject non-parseable metrics as before.

### Shared Stream-JSON Helper

[`core/src/json_extract.rs`](core/src/json_extract.rs) now owns `extract_stream_json_result()`, and [`core/src/scheduler/item_generate.rs`](core/src/scheduler/item_generate.rs) reuses the same helper.

This removes duplicate parsing logic and keeps stream-json handling consistent between dynamic item generation and capture extraction.

### Validation Guardrail

[`core/src/config_load/validate/workflow_steps.rs`](core/src/config_load/validate/workflow_steps.rs) now rejects `json_path` on unsupported capture sources such as `exit_code`, `failed_flag`, or `success_flag`.

### Workflow Update

[`docs/workflow/self-evolution.yaml`](docs/workflow/self-evolution.yaml) now captures:

```yaml
- var: score
  source: stdout
  json_path: "$.total_score"
```

That aligns the workflow with the benchmark agent contract.

## Acceptance Mapping

- `CaptureDecl` supports optional `json_path`: implemented in config schema and covered by deserialization tests
- `stdout + json_path` extracts `$.total_score`: implemented in accumulator capture logic and covered for plain JSON and stream-json output
- backward compatibility: legacy captures without `json_path` preserve prior behavior
- `self-evolution.yaml` updated: completed
- different candidate scores drive `item_select`: covered by a regression test that captures two benchmark scores and verifies max selection

## Verification

- `PROTOC=target/debug/build/protobuf-src-4bb380d39c3cf831/out/bin/protoc cargo test --workspace`
- `PROTOC=target/debug/build/protobuf-src-4bb380d39c3cf831/out/bin/protoc cargo clippy --workspace --all-targets -- -D warnings`

Both checks passed on 2026-03-12.
