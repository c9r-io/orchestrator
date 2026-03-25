# Multi-Model × Multi-Shell SDLC Benchmark Execution Plan

> **How to use**: Open this project in Claude Code, then ask Claude Code to read and execute this plan. Claude Code will autonomously handle resource deployment, task execution, monitoring, and result evaluation.

## 1. Benchmark Objective

Run the self-bootstrap workflow with an identical task goal using different LLM models and AI coding shells. Collect execution results, then have Claude Code perform a unified deep evaluation and comparative analysis.

## 2. Variable Matrix

| ID | Shell | Model | Agent Manifest | SecretStore |
|----|-------|-------|----------------|-------------|
| A1 | Claude Code | Opus 4.6 | `fixtures/benchmarks/agent-claude-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| A2 | Claude Code | Sonnet 4.6 | `fixtures/benchmarks/agent-claude-sonnet.yaml` | `fixtures/benchmarks/secrets-claude-sonnet.yaml` |
| B1 | OpenCode | Opus 4.6 | `fixtures/benchmarks/agent-opencode-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| C1 | Codex CLI | GPT-4o | `fixtures/benchmarks/agent-codex-gpt4o.yaml` | `fixtures/benchmarks/secrets-openai.yaml` |

> Extend the matrix by creating additional Agent + SecretStore manifests.

## 3. Prerequisites

The executor (Claude Code) should first verify:

- `orchestrator --version` and `orchestratord --version` are available
- Daemon is running (`orchestrator daemon status`); if not, start it: `orchestratord --foreground --workers 2 &`
- Shells referenced in the matrix are installed (`claude --version`, `opencode --version`, etc.)
- API keys in `fixtures/benchmarks/secrets-*.yaml` are filled in (no `<placeholder>` values)

## 4. Uniform Task Goal

All combinations use the exact same goal (controlled variable):

```
Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests.
```

## 5. Execution Flow (Per Combination)

For each combination ID in the matrix, execute the following steps:

### 5.1 Prepare Environment

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

If timeout or failure occurs, record the status and proceed to the next combination.

### 5.5 Collect Results

```bash
orchestrator task info <task_id> -o json
orchestrator event list --task <task_id> -o json
orchestrator task items <task_id> -o json
orchestrator task trace <task_id> --json
```

### 5.6 Save Output Snapshot

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

Repeat 5.1–5.7 until all combinations are complete.

## 6. Evaluation Phase

After all combinations are executed, Claude Code should perform a unified evaluation of all outputs in `results/`.

### 6.1 Quantitative Metrics (Extract from JSON Results)

| Metric | Data Source |
|--------|------------|
| Completion status | `task info` → `status` (completed/failed) |
| Total duration | `task info` → `started_at` to `completed_at` |
| Execution cycles | `event list` → `cycle_completed` event count |
| Step success rate | `event list` → `step_finished` with `success: true` ratio |

### 6.2 Code Quality Evaluation (Claude Code Executes Directly)

For each combination's `results/<combo_id>-diff.patch`:

1. **Build check**: Does `cargo build --release` pass?
2. **Test check**: Does `cargo test --workspace` pass?
3. **Lint check**: Does `cargo clippy --workspace -- -D warnings` pass?
4. **Diff review**: Read the patch file and evaluate:
   - Is the implementation correct and complete?
   - Is the code concise and idiomatic?
   - Are there unnecessary changes?
   - Is error handling adequate?
   - Is test coverage reasonable?

### 6.3 Output Evaluation Report

Output a comparison in markdown table format:

```markdown
| Combo | Shell | Model | Status | Duration | Cycles | Build | Tests | Lint | Diff Lines | Quality (0-10) | Notes |
|-------|-------|-------|--------|----------|--------|-------|-------|------|------------|----------------|-------|
| A1    | ...   | ...   | ...    | ...      | ...    | ...   | ...   | ...  | ...        | ...            | ...   |
```

Conclude with a summary analysis: strengths and weaknesses of each combination, comparative findings along the model and shell dimensions, and recommended configuration.

## 7. Constraints

- **Control variables**: Change only one variable (model or shell) at a time; workflow and goal remain constant
- **Environment isolation**: Restore clean git state before and after each combination
- **Timeout protection**: 30-minute timeout per task
- **Cost awareness**: Opus is ~5x Sonnet cost; confirm budget before batch execution
- **Reproducibility**: All manifests are versioned in `fixtures/benchmarks/`
