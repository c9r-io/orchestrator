---
self_referential_safe: true
---

# Orchestrator - Step Variable Expansion Completeness

**Module**: orchestrator
**Scope**: Verify placeholder expansion is correct and complete across renderer helpers, runtime prompt/command wiring, all known step families, and leftover-placeholder anomaly detection
**Scenarios**: 5
**Priority**: High

---

## Background

Variable expansion in the orchestrator is considered complete only when all known workflow steps route through a covered rendering path and no persisted command keeps literal placeholders.

Coverage model:

| Step family | Known step IDs | Rendering path |
|--------|------|-------|
| Builtin or builtin-command steps | `init_once`, `loop_guard`, `self_test` | `crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch.rs` + `AgentContext::render_template_with_pipeline()` |
| Task-scoped capability/template steps | `plan`, `build`, `test`, `lint`, `implement`, `review`, `git_ops`, `qa_doc_gen`, `align_tests`, `doc_governance`, `smoke_chain` | `crates/orchestrator-scheduler/src/scheduler/phase_runner/mod.rs` + `AgentContext::render_template_with_pipeline()` |
| Item-scoped capability/template steps | `qa`, `ticket_scan`, `fix`, `retest`, `qa_testing`, `ticket_fix` | `crates/orchestrator-scheduler/src/scheduler/phase_runner/mod.rs` basic placeholders + pipeline context |

Placeholder families covered by this document:

- Basic placeholders: `{rel_path}`, `{ticket_paths}`, `{phase}`, `{task_id}`, `{cycle}`, `{unresolved_items}`
- Runtime context: `{item_id}`, `{workspace_root}`, `{source_tree}`
- Pipeline vars: `{goal}`, `{diff}`, `{plan_output}`, `{plan_output_path}`, `{build_output}`, `{test_output}`, `{build_errors}`, `{test_failures}`, and arbitrary `{key}` entries in `pipeline.vars`
- Advanced placeholders: `{upstream[i].exit_code}`, `{upstream[i].confidence}`, `{upstream[i].quality_score}`, `{upstream[i].duration_ms}`, `{upstream[i].artifacts[j].content}`, shared-state `{key}`, and `{artifacts.count}`

Design doc: `docs/design_doc/orchestrator/43-step-variable-expansion-governance.md`

---

## Scenario 1: Basic Template Renderer Covers Core Placeholders

### Preconditions
- Rust toolchain is available
- Repository root is `/Volumes/Yotta/c9r-io/orchestrator`

### Goal
Verify the low-level template helpers fully replace basic placeholders and preserve expected behavior for empty ticket lists and shared-state helpers.

### Steps
1. Run:
   ```bash
   cargo test --workspace --lib render_template_replaces_placeholders -- --nocapture
   cargo test --workspace --lib render_template_handles_empty_ticket_paths -- --nocapture
   cargo test --workspace --lib basic_template_context_all_fields -- --nocapture
   cargo test --workspace --lib advanced_template_context_with_upstream -- --nocapture
   cargo test --workspace --lib advanced_template_context_supports_quality_score_replacement -- --nocapture
   cargo test --workspace --lib render_template_with_context_replaces_phase_upstream_and_shared_state -- --nocapture
   ```
2. Review the tested source:
   ```bash
   rg -n "render_template_replaces_placeholders|basic_template_context_all_fields|advanced_template_context_with_upstream|render_template_with_context_replaces_phase_upstream_and_shared_state" core/src/qa_utils.rs
   ```

### Expected
- All listed tests pass.
- The covered assertions prove replacement of `{rel_path}`, `{ticket_paths}`, `{phase}`, `{task_id}`, `{cycle}`, `{unresolved_items}`, `{upstream[0].exit_code}`, `{upstream[0].confidence}`, `{upstream[0].quality_score}`, and shared-state `{key}` placeholders.
- No test leaves a literal placeholder in the expected output strings.

---

## Scenario 2: Agent Context Renders Runtime, Pipeline, and Escape-Sensitive Values

### Preconditions
- Rust toolchain is available

### Goal
Verify `AgentContext::render_template_with_pipeline()` expands runtime context, pipeline vars, source-tree aliases, and shell-escaped content correctly.

### Steps
1. Run:
   ```bash
   cargo test --workspace --lib test_agent_context_template -- --nocapture
   cargo test --workspace --lib test_agent_context_render_source_tree_alias -- --nocapture
   cargo test --workspace --lib test_pipeline_vars_escaped_in_template -- --nocapture
   ```
2. Inspect the implementation and tests:
   ```bash
   rg -n "render_template_with_pipeline|test_agent_context_template|test_agent_context_render_source_tree_alias|test_pipeline_vars_escaped_in_template" core/src/collab/context.rs
   ```

### Expected
- All listed tests pass.
- The tests verify `{task_id}`, `{item_id}`, `{cycle}`, `{phase}`, `{workspace_root}`, and `{source_tree}` replacement.
- Pipeline values containing shell-sensitive content are escaped instead of being injected raw.
- The source-tree alias resolves to the same value as `workspace_root`.

---

## Scenario 3: Runtime Propagation Expands Large Pipeline Variables Without Leaving Placeholders

### Preconditions
- Rust toolchain is available

### Goal
Verify runtime prompt-to-command propagation expands spill-file placeholders such as `{plan_output_path}` and keeps oversized pipeline content under the runner command limit.

### Steps
1. Run auto-capture propagation tests (plan_output → downstream step):
   ```bash
   cargo test -p orchestrator-scheduler --lib auto_capture_extracts_stream_json_result_for_spill -- --nocapture
   cargo test -p orchestrator-scheduler --lib auto_capture_falls_back_to_raw_stdout_for_non_stream_json -- --nocapture
   cargo test -p orchestrator-scheduler --lib auto_capture_stream_json_large_result_spills_only_extracted_text -- --nocapture
   ```

2. Run spill-file size threshold tests:
   ```bash
   cargo test -p orchestrator-scheduler --lib spill_large_var_small_value_inserts_inline -- --nocapture
   cargo test -p orchestrator-scheduler --lib spill_large_var_exactly_at_limit_inserts_inline -- --nocapture
   cargo test -p orchestrator-scheduler --lib spill_large_var_one_byte_over_limit_spills_to_file -- --nocapture
   cargo test -p orchestrator-scheduler --lib spill_large_var_large_value_sets_correct_path_key -- --nocapture
   cargo test -p orchestrator-scheduler --lib spill_large_var_multibyte_boundary -- --nocapture
   ```

3. Review the spill implementation:
   ```bash
   rg -n "fn spill_large_var\b|fn spill_to_file\b|PIPELINE_VAR_INLINE_LIMIT" crates/orchestrator-scheduler/src/scheduler/item_executor/spill.rs crates/orchestrator-config/src/config/pipeline.rs
   ```

### Expected
- All 8 tests pass.
- Auto-capture confirms `plan` step stdout is captured as `plan_output` pipeline var, with stream-json result extraction when available.
- Spill behavior is size-aware: values ≤ 4096 bytes stay inline, oversized values are truncated with a pointer to `{key}.txt` spill file.
- `{plan_output_path}` is always set when auto-capture runs, pointing to the spill file.

---

## Scenario 4: Every Known Step ID Maps to a Covered Rendering Entry Point

### Preconditions
- Repository root is `/Volumes/Yotta/c9r-io/orchestrator`
- Mock fixture manifests are available

### Goal
Verify that no known step type bypasses the documented rendering paths and that the self-bootstrap mock fixture continues to exercise the template-driven step family.

### Steps
1. Verify the canonical known-step inventory:
   ```bash
   rg -n "known IDs|Task-scoped|Item-scoped" docs/qa/orchestrator/30-unified-step-execution-model.md
   rg -n '"(init_once|plan|qa|ticket_scan|fix|retest|loop_guard|build|test|lint|implement|review|git_ops|qa_doc_gen|qa_testing|ticket_fix|doc_governance|align_tests|self_test|smoke_chain)"' crates/orchestrator-config/src/config/step.rs
   ```
2. Verify task-scoped and item-scoped runtime rendering funnels:
   ```bash
   rg -n "render_template_with_pipeline\\(|step_template_prompt|replace\\(\"\\{prompt\\}\"|replace\\(\"\\{rel_path\\}\"|replace\\(\"\\{ticket_paths\\}\"" crates/orchestrator-scheduler/src/scheduler/phase_runner/mod.rs crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch.rs
   ```
3. Verify the mock fixture still contains representative template-driven steps and placeholder-bearing prompts:
   ```bash
   rg -n "name: (plan|qa_doc_gen|implement|ticket_fix|align_tests|qa_testing|doc_governance|review)" fixtures/manifests/bundles/self-bootstrap-mock.yaml
   rg -n "\\{source_tree\\}|\\{plan_output_path\\}|\\{ticket_paths\\}|\\{rel_path\\}|\\{diff\\}" fixtures/manifests/bundles/self-bootstrap-mock.yaml
   ```

### Expected
- The known-step inventory contains all 21 standard step IDs with no undocumented extra step family.
- Rendering code inspection shows builtin-command steps and capability/template steps both pass through `render_template_with_pipeline()` or the phase-runner placeholder replacement path before spawn.
- The self-bootstrap mock fixture continues to exercise template-driven placeholders for representative task-scoped and item-scoped steps.
- Reviewers can map every known step ID to one of the three coverage rows in this document's Background table.

---

## Scenario 5: Task Trace Flags Leftover Unexpanded Placeholders

### Preconditions
- Rust toolchain is available

### Goal
Verify the diagnostic backstop catches persisted commands that still contain template placeholders after rendering.

### Steps
1. Run:
   ```bash
   cargo test --workspace --lib detect_unexpanded_template_var_anomaly -- --nocapture
   ```
2. Inspect the anomaly rule implementation:
   ```bash
   rg -n "detect_unexpanded_template_var|UnexpandedTemplateVar" core/src/anomaly.rs crates/orchestrator-scheduler/src/scheduler/trace/tests.rs
   ```

### Expected
- The anomaly test passes.
- The trace layer classifies leftover placeholders as `unexpanded_template_var`.
- Variable-expansion regressions remain visible even when a command run is otherwise recorded successfully.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Basic template renderer covers core placeholders | PASS | 2026-03-20 | Claude | 6/6 tests pass — all in qa_utils.rs |
| 2 | Agent context renders runtime, pipeline, and escape-sensitive values | PASS | 2026-03-20 | Claude | 3/3 tests pass — all in collab/context.rs |
| 3 | Runtime propagation expands large pipeline variables without leaving placeholders | PASS | 2026-03-20 | Claude | 8/8 tests pass — auto_capture (3) + spill_large_var (5) in orchestrator-scheduler |
| 4 | Every known step ID maps to a covered rendering entry point | PASS | 2026-03-20 | Claude | Code review — all 21 step IDs verified, rendering paths confirmed |
| 5 | Task trace flags leftover unexpanded placeholders | PASS | 2026-03-20 | Claude | detect_unexpanded_template_var_anomaly test passes in trace/tests.rs |
