# Design Doc 45: Plan Output Context Overflow Mitigation (FR-036)

## Problem

When a plan agent runs with `--output-format stream-json`, its stdout contains the
full session transcript (all JSONL lines: thinking blocks, tool_use requests,
tool_result responses, and the final result). For a 25-turn plan session this can
exceed 80K tokens.

The auto-capture logic in `dispatch.rs` spills this raw stdout as
`{step_id}_output.txt` (e.g. `plan_output.txt`). Downstream steps (implement,
qa_doc_gen) reference `{plan_output_path}` to read the plan, but the oversized
file exceeds Claude's single-Read tool limit (~25K tokens), causing the downstream
agent to fail with exit=-1. This triggers a degenerate retry loop (FR-035).

## Design Decision

**Extract the `result` field from stream-json output before spilling.**

The auto-capture site in `execute_agent_step()` now calls
`extract_stream_json_result()` on the raw stdout. If extraction succeeds, only
the plan text is spilled; if it fails (non-stream-json agent), the raw stdout is
used as before.

### Why this approach

1. **Minimal change** — one code path modified, no new abstractions
2. **Reuses existing utility** — `extract_stream_json_result()` in `json_extract.rs`
   was already tested and production-ready
3. **Full transcript retained** — the runner persists raw stdout at `stdout_path`
   in the session store, so audit/debugging access is unaffected
4. **Backward compatible** — non-stream-json agents fall back to raw stdout;
   small outputs that fit inline are unaffected

### Alternatives considered

- **Separate summary file** (`plan_summary.txt` alongside `plan_output.txt`):
  rejected as unnecessary complexity; the full transcript already has a separate
  persistence path via `stdout_path`
- **Pipeline variable direct pass** (`SetPipelineVar` post-action): rejected
  because it requires YAML schema changes and the existing auto-capture mechanism
  is the right abstraction level
- **Segmented index file**: rejected as over-engineered; downstream agents
  would need index-parsing logic

## Implementation

### Changed file

`core/src/scheduler/item_executor/dispatch.rs` — `execute_agent_step()`, the
auto-capture block (~line 815):

```rust
let effective_output =
    crate::json_extract::extract_stream_json_result(&output.stdout)
        .unwrap_or_else(|| output.stdout.clone());
spill_large_var(&state.logs_dir, task_id, &output_key, effective_output, ...);
```

### Tests added

- `auto_capture_extracts_stream_json_result_for_spill` — verifies extraction from
  stream-json transcript
- `auto_capture_falls_back_to_raw_stdout_for_non_stream_json` — verifies fallback
- `auto_capture_stream_json_large_result_spills_only_extracted_text` — verifies
  that a large transcript with small result produces a small spill file
