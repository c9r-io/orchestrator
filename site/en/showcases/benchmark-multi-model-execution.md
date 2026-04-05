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

After all combinations have been executed, the Agent should perform a unified evaluation of all artifacts in the `results/` directory.

### 6.1 Quantitative Metrics (Extracted from JSON Results)

| Metric | Data Source |
|--------|------------|
| Completion status | `task info` → `status` (completed/failed) |
| Total duration | `task info` → `started_at` to `completed_at` |
| Execution cycles | `event list` → `cycle_completed` event count |
| Step success rate | `event list` → proportion of `step_finished` with `success: true` |

### 6.2 Six-Dimension Evaluation (Executed Directly by Agent)

The agent first runs `git diff --stat`. If the diff is empty, Task Completion scores 0 and remaining dimensions are skipped.

| Dimension | Score | Criteria |
|-----------|-------|----------|
| **Task Completion** | 0-10 | Did the agent produce actual code changes that address the goal |
| **Code Quality** | 0-10 | Is the implementation correct, idiomatic, and concise |
| **Test Coverage** | 0-10 | Are there meaningful unit tests covering new/changed code |
| **Execution Efficiency** | 0-10 | End-to-end wall time relative to task complexity |
| **Step Success Rate** | 0-10 | Did each workflow step exit normally |
| **Engineering Standards** | 0-10 | Error handling, doc comments, safety annotations, lint cleanliness |

The agent runs the project's build/test/lint commands, then outputs a six-dimension JSON score (total 0-60).

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

```bash
# 1. Authenticate each Agent CLI (select the shells you want to test)
opencode auth          # MiniMax API key
gemini auth            # Google AI Studio login
codex auth             # OpenAI API key

# 2. Verify API keys are written into SecretStore manifests
#    Edit fixtures/benchmarks/secrets-*.yaml with real keys
cat fixtures/benchmarks/secrets-glm5.yaml     # Check MiniMax key
cat fixtures/benchmarks/secrets-gemini.yaml   # Check Gemini (usually empty — CLI handles auth)
cat fixtures/benchmarks/secrets-openai.yaml   # Check OpenAI key

# 3. Verify orchestrator is built and installed
orchestrator --version   # Should print version
orchestratord --version

# 4. Verify shell CLIs are installed
opencode --version
gemini --version
codex --version
```

### 8.2 Ready-to-Execute Prompt

Once the above is done, paste the following prompt into your AI coding agent (e.g., Claude Code) to start the full workflow:

````
Execute the multi-model benchmark test per docs/showcases/benchmark-multi-model-execution.md.

## Context
- Variable matrix: 5 combos — A1 (Claude Code+Opus), B1 (OpenCode+Opus), C1 (OpenCode+GLM-5), D1 (Gemini CLI+Gemini 3.1 Pro), E1 (Codex CLI+GPT-5.4)
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
Execute showcase doc steps 5.1-5.7 sequentially for A1 → B1 → C1 → D1 → E1:
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
