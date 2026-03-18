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
   cd core && cargo test render_template_replaces_placeholders -- --nocapture
   cd core && cargo test render_template_handles_empty_ticket_paths -- --nocapture
   cd core && cargo test basic_template_context_all_fields -- --nocapture
   cd core && cargo test advanced_template_context_with_upstream -- --nocapture
   cd core && cargo test advanced_template_context_supports_quality_score_replacement -- --nocapture
   cd core && cargo test render_template_with_context_replaces_phase_upstream_and_shared_state -- --nocapture
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
   cd core && cargo test test_agent_context_template -- --nocapture
   cd core && cargo test test_agent_context_render_source_tree_alias -- --nocapture
   cd core && cargo test test_pipeline_vars_escaped_in_template -- --nocapture
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
1. Run:
   ```bash
   cd core && cargo test plan_output_is_propagated_to_qa_doc_gen_template -- --nocapture
   cd core && cargo test spill_large_var_spills_when_over_limit -- --nocapture
   cd core && cargo test spill_large_var_inline_when_small -- --nocapture
   ```
2. Inspect the regression source:
   ```bash
   rg -n "plan_output_is_propagated_to_qa_doc_gen_template|spill_large_var_spills_when_over_limit|spill_large_var_inline_when_small|plan_output_path" core/src/scheduler.rs core/src/scheduler/item_executor/tests.rs
   ```

### Expected
- All listed tests pass.
- The scheduler regression confirms `qa_doc_gen` receives `plan_output` in truncated inline form and a concrete `plan_output.txt` spill-file path.
- The persisted command must not contain the literal string `{plan_output_path}`.
- Spill behavior remains size-aware: small values stay inline, oversized values are written to spill files.

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
   cd core && cargo test detect_unexpanded_template_var_anomaly -- --nocapture
   ```
2. Inspect the anomaly rule implementation:
   ```bash
   rg -n "detect_unexpanded_template_var|UnexpandedTemplateVar" core/src/scheduler/trace/anomaly.rs core/src/scheduler/trace/tests.rs core/src/anomaly.rs
   ```

### Expected
- The anomaly test passes.
- The trace layer classifies leftover placeholders as `unexpanded_template_var`.
- Variable-expansion regressions remain visible even when a command run is otherwise recorded successfully.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Basic template renderer covers core placeholders | PASS | 2026-03-19 | Claude | 6/6 tests pass — all in qa_utils.rs |
| 2 | Agent context renders runtime, pipeline, and escape-sensitive values | PASS | 2026-03-19 | Claude | 3/3 tests pass — all in collab/context.rs |
| 3 | Runtime propagation expands large pipeline variables without leaving placeholders | FAIL | 2026-03-19 | Claude | 3 tests missing (plan_output_is_propagated, spill_large_var_spills, spill_large_var_inline); see ticket qa82_s3 |
| 4 | Every known step ID maps to a covered rendering entry point | PARTIAL | 2026-03-19 | Claude | 21 step IDs verified; phase_runner/item_executor paths in Background table reference non-existent files |
| 5 | Task trace flags leftover unexpanded placeholders | FAIL | 2026-03-19 | Claude | detect_unexpanded_template_var_anomaly test missing; see ticket qa82_s5 |
