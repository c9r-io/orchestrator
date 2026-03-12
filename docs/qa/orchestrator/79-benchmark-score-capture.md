# QA #79: Benchmark Score Capture (FR-028)

## Scope

Verify that benchmark scores can be extracted from agent JSON output into pipeline variables, that stream-json output is supported, and that `item_select` can choose the higher-scoring candidate.

## Scenarios

### S-01: Capture schema accepts optional `json_path`

**Steps**:
1. Run `cargo test -p agent-orchestrator capture_decl_deserializes -- --nocapture`

**Expected**:
- capture declarations deserialize both with and without `json_path`
- legacy manifests remain compatible

### S-02: Plain JSON stdout captures benchmark score

**Steps**:
1. Run `cargo test -p agent-orchestrator apply_captures_stdout_json_path_extracts_score -- --nocapture`

**Expected**:
- `source: stdout`
- `json_path: "$.total_score"`
- captured pipeline var `score == "85"` for the fixture output

### S-03: Stream-JSON stdout captures benchmark score

**Steps**:
1. Run `cargo test -p agent-orchestrator apply_captures_stdout_json_path_extracts_stream_json_score -- --nocapture`

**Expected**:
- scheduler resolves the last stream-json `result` payload first
- `score` is extracted from the embedded JSON body

### S-04: Missing JSON field degrades safely

**Steps**:
1. Run `cargo test -p agent-orchestrator apply_captures_stdout_json_path_falls_back_to_empty_string_on_missing_field -- --nocapture`

**Expected**:
- capture does not panic or abort the step
- pipeline variable is recorded as an empty string
- warning logging is emitted by runtime code

### S-05: Higher captured score wins selection

**Steps**:
1. Run `cargo test -p agent-orchestrator benchmark_score_capture_can_drive_item_select_max -- --nocapture`

**Expected**:
- two candidates receive different captured `score` values
- `item_select` with `strategy: max` selects the higher-scoring item

### S-06: Invalid source combinations are rejected

**Steps**:
1. Run `cargo test -p agent-orchestrator validate_workflow_config_rejects_json_path_on_exit_code_capture -- --nocapture`

**Expected**:
- workflow validation rejects `json_path` when used with `exit_code`

### S-07: Workspace regression gates remain green

**Steps**:
1. Run `PROTOC=/Volumes/Yotta/c9r-io/orchestrator/target/debug/build/protobuf-src-4bb380d39c3cf831/out/bin/protoc cargo test --workspace`
2. Run `PROTOC=/Volumes/Yotta/c9r-io/orchestrator/target/debug/build/protobuf-src-4bb380d39c3cf831/out/bin/protoc cargo clippy --workspace --all-targets -- -D warnings`

**Expected**:
- all workspace tests pass
- clippy passes with `-D warnings`

## Result

Verified on 2026-03-12:

- targeted capture and validation regressions passed
- `cargo test --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed
