# Multi-Model x Multi-Shell SDLC Benchmark Execution Plan

> **Harness Engineering execution plan**: this is an agent-executable scenario that shows how the control plane coordinates environment, workflow, guardrails, and feedback loops rather than a one-off agent call.
>
> **Agent Collaboration**: This document is an agent-executable plan. Open this project in an AI coding agent (Claude Code, OpenCode, Codex, etc.) — the agent reads this plan and orchestrates other agents via the orchestrator CLI to collaboratively complete the task, from resource deployment and execution to result evaluation, fully autonomously.

## 1. Benchmark Objective

Using the controlled variable method, under the same task goal and Workflow, substitute different **LLM models** and **Agent shells** to evaluate performance differences across two independent dimensions:

- **Model dimension**: Fix the shell (e.g., Claude Code), swap the model (Opus / Sonnet / GLM-5 / Gemini / GPT-5.4), and observe how model capability affects task completion and code quality
- **Shell dimension**: Fix the model (e.g., Opus 4.6), swap the shell (Claude Code / OpenCode / Codex / Gemini CLI), and observe how the shell toolchain affects execution efficiency and results

## 2. Variable Matrix

| ID | Shell | Model | Agent Manifest | SecretStore |
|----|-------|-------|----------------|-------------|
| A1 | Claude Code | Opus 4.6 | `fixtures/benchmarks/agent-claude-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| B1 | OpenCode | Opus 4.6 | `fixtures/benchmarks/agent-opencode-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| C1 | OpenCode | GLM-5 | `fixtures/benchmarks/agent-opencode-glm5.yaml` | `fixtures/benchmarks/secrets-glm5.yaml` |
| D1 | Gemini CLI | Gemini 3.1 Pro | `fixtures/benchmarks/agent-gemini-pro.yaml` | `fixtures/benchmarks/secrets-gemini.yaml` |
| E1 | Codex CLI | GPT-5.4 | `fixtures/benchmarks/agent-codex-gpt54.yaml` | `fixtures/benchmarks/secrets-openai.yaml` |

**Controlled Variable Analysis Groups:**

| Comparison | Fixed Variable | Changed Variable | Observation Target |
|------------|----------------|------------------|--------------------|
| A1 vs B1 | Opus 4.6 | Claude Code vs OpenCode | Shell difference |
| B1 vs C1 | OpenCode | Opus 4.6 vs GLM-5 | Model difference |
| A1 vs D1 vs E1 | — | All combinations | Overall difference |

> Extensible as needed: simply create new Agent + SecretStore manifests.

## 3. Prerequisites

The executor (Agent) should first verify the following conditions:

- `orchestrator --version` and `orchestratord --version` are executable
- The daemon is running (`orchestrator daemon status`); if not, start it: `orchestratord --foreground --workers 2 &`
- The shells listed in the matrix are installed (`claude --version`, `opencode --version`, `gemini --version`, `codex --version`, etc.)
- API keys in `fixtures/benchmarks/secrets-*.yaml` have been filled in (not `<placeholder>` values)

## 4. Unified Task Goal

All combinations use the exact same goal (controlled variable):

```
Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests.
```

## 5. Execution Flow (Per Combination)

For each combination ID in the matrix, follow these steps:

### 5.1 Environment Preparation

```bash
cd "$ORCHESTRATOR_ROOT"
git stash --include-untracked || true
```

### 5.2 Deploy Resources

```bash
orchestrator apply -f fixtures/benchmarks/<secret_file> --project benchmark
orchestrator apply -f fixtures/benchmarks/<agent_file> --project benchmark
orchestrator apply -f fixtures/benchmarks/workflow-benchmark-bootstrap.yaml --project benchmark
```

Verify:

```bash
orchestrator get agents --project benchmark -o json
orchestrator get workflows --project benchmark -o json
```

### 5.3 Create Task

```bash
orchestrator task create \
  --project benchmark \
  --workflow benchmark-bootstrap \
  --goal "Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests."
```

Record the returned `task_id`.

### 5.4 Monitor Until Completion

```bash
orchestrator task watch <task_id> --timeout 1800
```

If it times out or fails, record the status and proceed to the next combination.

### 5.5 Collect Results

```bash
orchestrator task info <task_id> -o json
orchestrator event list --task <task_id> -o json
orchestrator task items <task_id> -o json
orchestrator task trace <task_id> --json
```

### 5.6 Save Artifact Snapshot

```bash
git diff > results/<combo_id>-diff.patch
git diff --stat > results/<combo_id>-diffstat.txt
```

### 5.7 Restore Environment

```bash
git checkout -- .
git clean -fd
git stash pop || true
```

Repeat steps 5.1–5.7 until all combinations have been executed.

## 6. Evaluation Phase

After all combinations have been executed, the **host agent** (the agent executing this plan — not the target agents being benchmarked) performs a unified evaluation of all artifacts in the `results/` directory.

> **Evaluator independence**: The workflow includes an in-loop `benchmark_eval` step executed by each target agent as a self-check. However, the authoritative six-dimension scores in §6.2 are produced by the host agent examining collected artifacts (diffs, event logs, task traces) post-hoc. This separation ensures the evaluator is independent of the evaluated agent — the same principle as having a referee who isn't also a player.

### 6.1 Quantitative Metrics (Extracted from JSON Results)

| Metric | Data Source |
|--------|------------|
| Completion status | `task info` → `status` (completed/failed) |
| Total duration | `task info` → `started_at` to `completed_at` |
| Execution cycles | `event list` → `cycle_completed` event count |
| Step success rate | `event list` → proportion of `step_finished` with `success: true` |

### 6.2 Six-Dimension Evaluation (Host Agent Post-Hoc)

The **host agent** applies the diff from each combination, runs `git diff --stat`, build/test/lint commands, and inspects the collected JSON artifacts. If the diff is empty, Task Completion scores 0 and remaining dimensions are skipped.

| Dimension | Score | Scoring Rubric |
|-----------|-------|----------------|
| **Task Completion** | 0-10 | 0 = no code changes; 5 = partial implementation missing key requirements; 10 = all requirements addressed with working code |
| **Code Quality** | 0-10 | 0 = syntax errors or broken build; 5 = compiles but non-idiomatic; 10 = correct, idiomatic, concise |
| **Test Coverage** | 0-10 | 0 = no tests; 5 = tests exist but miss edge cases; 10 = comprehensive unit tests covering new/changed code |
| **Execution Efficiency** | 0-10 | Based on wall time from `task info` timestamps: 0 = timeout (>30min); 5 = 10-15min; 10 = under 5min |
| **Step Success Rate** | 0-10 | From `event list` JSON: proportion of `step_finished` events with `success: true`, linearly mapped to 0-10 |
| **Engineering Standards** | 0-10 | 0 = lint failures / missing error handling; 5 = compiles clean; 10 = error handling, doc comments, safety annotations, zero warnings |

The host agent applies each patch, runs `cargo check`, `cargo test`, and `cargo clippy`, then outputs a six-dimension JSON score (total 0-60).

> **Data sources**: Execution Efficiency and Step Success Rate are derived from quantitative data (timestamps, event logs), not subjective judgment. The remaining four dimensions are assessed by the host agent after running the project's toolchain against the actual code output.

### 6.3 Output Evaluation Report

Output the comparison results in markdown table + radar chart format:

```markdown
| Combo | Shell | Model | Status | Duration | Cycles | Completion | Quality | Tests | Efficiency | Success | Standards | Total(/60) | Notes |
|-------|-------|-------|--------|----------|--------|------------|---------|-------|------------|---------|-----------|------------|-------|
| A1    | Claude Code  | Opus 4.6      | | | | | | | | | | | |
| B1    | OpenCode     | Opus 4.6      | | | | | | | | | | | |
| C1    | OpenCode     | GLM-5         | | | | | | | | | | | |
| D1    | Gemini CLI   | Gemini 3.1 Pro| | | | | | | | | | | |
| E1    | Codex CLI    | GPT-5.4       | | | | | | | | | | | |
```

Finally, provide a summary analysis along two dimensions:

1. **Model dimension** (comparing B1 vs C1: same shell OpenCode, different models): impact of model capability on results
2. **Shell dimension** (comparing A1 vs B1: same model Opus, different shells): impact of toolchain on execution efficiency
3. **Overall ranking**: six-dimension radar chart comparison, recommendation ranking of all combinations

## 7. Constraints

- **Controlled variables**: Change only one variable at a time (model or shell); the workflow and goal remain unchanged
- **Environment isolation**: Restore a clean git state before and after each combination execution
- **Timeout protection**: 30-minute timeout per task
- **Cost awareness**: Opus is approximately 5x the cost of Sonnet; confirm budget before batch execution
- **Reproducibility**: All manifests are versioned in `fixtures/benchmarks/`

## 8. Example: One-Click Benchmark Execution

### 8.1 User Prerequisites (Manual Steps)

Before handing the prompt to your AI coding agent, complete the following authentication and setup:

**Authenticate each Agent CLI** (select the shells you want to test):

| Shell | Authentication |
|-------|----------------|
| OpenCode | `opencode auth` — interactive provider and API key setup |
| Gemini CLI | Complete Google account login inside the tool on first run, or set `GEMINI_API_KEY` env var |
| Codex CLI | Complete login inside the tool on first run, or set `OPENAI_API_KEY` env var |

**Verify environment is ready**:

```bash
# Confirm each CLI is installed and responds
opencode --version
gemini --version
codex --version

# Confirm orchestrator is built and installed
orchestrator --version
orchestratord --version

# Confirm SecretStore manifests have real keys (not placeholders)
# Edit fixtures/benchmarks/secrets-*.yaml
```

### 8.2 Ready-to-Execute Prompt

Once the above is done, paste the following prompt into your AI coding agent (e.g., Claude Code) to start the full workflow:

````
Execute the multi-model benchmark test per docs/showcases/benchmark-multi-model-execution.md.

## Context
- Variable matrix: 3 combos (trimmed; expand to 5 per section 2) — C1 (OpenCode+MiniMax), D1 (Gemini CLI+Flash), E1 (Codex CLI+GPT-5.4-mini)
- Agent manifests / SecretStores / Workflow are in fixtures/benchmarks/
- All CLIs are authenticated; report auth failures to the user

## Pre-execution cleanup
1. Rebuild: cargo build --release -p orchestratord -p orchestrator-cli, install to ~/.cargo/bin/
2. Restart daemon: kill old process → orchestratord --foreground --workers 2
3. Clean residual benchmark project assets:
   - orchestrator task delete --all -p benchmark -f
   - orchestrator get agents/workflows/workspaces -p benchmark → delete each
4. mkdir -p results

## Execution flow
Execute showcase doc steps 5.1-5.7 sequentially for C1 → D1 → E1:
- apply secrets → apply agent → apply workflow
- task create → task watch --timeout 1800
- Collect results (task info/event list/task items/task trace -o json)
- Save git diff to results/<combo_id>-*
- git checkout/clean to restore environment (preserve results/ directory)
- Delete current combo's agent (keep workflow/workspace shared; if capability validation errors occur, delete workflow too and recreate)

Clean agent between combos to avoid capability conflicts.

## Evaluation
After all combos complete, generate results/benchmark-report.md per doc sections 6.1-6.3 using the six-dimension evaluation criteria.

## Error handling
- Timeout or failure: record status and continue to next combo
- Auth failure: report to user, wait for fix before continuing
````

### 8.3 Actual Execution Results Reference (2026-04-05)

Below are real results from executing the above prompt on orchestrator v0.3.0 with a trimmed matrix (C1/D1/E1).

> **Evaluation context**: The host agent (Claude Code / Opus 4.6) orchestrated execution of all three target agents, collected artifacts, and produced the final six-dimension scores. The entire flow — from resource deployment through monitoring, artifact collection, and scoring — ran autonomously with zero human intervention.
>
> **Reproducibility**: All manifests are versioned in `fixtures/benchmarks/`. Raw artifacts (diffs, event logs, task traces) are in `results/`. The ready-to-execute prompt in §8.2 is the exact prompt used for this run. A1 and B1 are intentionally left unexecuted — see §2 for the full matrix.

**Six-Dimension Evaluation Overview**

| Combo | Shell | Model | Status | Duration | Completion | Quality | Tests | Efficiency | Success | Standards | Total(/60) | Notes |
|-------|-------|-------|--------|----------|------------|---------|-------|------------|---------|-----------|------------|-------|
| C1 | OpenCode | MiniMax-M2.7 | completed | 5m27s | 2 | 1 | 0 | 8 | 8 | 1 | 20 | Modified 1 test file only, no retry impl |
| D1 | Gemini CLI | Flash-preview | timeout | >44m | 7 | 6 | 4 | 1 | 3 | 5 | 26 | Full implementation but timed out (30min) |
| E1 | Codex CLI | GPT-5.4-mini | completed | 5m14s | 9 | 7 | 5 | 9 | 8 | 6 | 44 | Full implementation, fastest |

**Execution Time Breakdown**

| Combo | plan | implement | self_test | eval | Total |
|-------|------|-----------|-----------|------|-------|
| C1 | 117s | 92s | 0s | 95s | 327s |
| D1 | 1094s | >1590s | — | — | >2685s |
| E1 | 85s | 202s | 0s | 27s | 314s |

**Code Output**

| Combo | Files Changed | Lines +/- | Core Changes |
|-------|---------------|-----------|--------------|
| C1 | 1 | +4/-3 | Only modified a test file |
| D1 | 8 | +278/-76 | connect.rs + CLI integration |
| E1 | 9 | +267/-73 | connect.rs + CLI + GUI integration |

**Conclusion:** E1 (Codex/GPT-5.4-mini) completed the full workflow in 5m14s with a six-dimension score of 44/60, the clear winner. D1 produced comparable code volume to E1 but timed out; C1 failed to complete the actual task.
