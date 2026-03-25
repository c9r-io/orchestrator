# Multi-Model × Multi-Shell SDLC Benchmark Execution Plan

This document defines a repeatable benchmark framework for comparing different LLM models and AI coding shells on identical task goals.

## 1. Variable Matrix

| Dimension | Variable | Control Mechanism |
|-----------|----------|-------------------|
| **Model** | claude-opus-4-6, claude-sonnet-4-6, gpt-4o, gemini-2.5-pro | SecretStore `ANTHROPIC_MODEL` / provider env |
| **Shell** | Claude Code, OpenCode, Codex CLI, Gemini CLI | Agent `spec.command` |
| **Task** | self-bootstrap (linear iteration), self-evolution (competitive selection) | Workflow manifest |

### Predefined Combinations

| ID | Shell | Model | Agent Manifest | SecretStore |
|----|-------|-------|----------------|-------------|
| A1 | Claude Code | Opus 4.6 | `agent-claude-opus.yaml` | `secrets-claude-opus.yaml` |
| A2 | Claude Code | Sonnet 4.6 | `agent-claude-sonnet.yaml` | `secrets-claude-sonnet.yaml` |
| B1 | OpenCode | Opus 4.6 | `agent-opencode-opus.yaml` | `secrets-claude-opus.yaml` |
| C1 | Codex CLI | GPT-4o | `agent-codex-gpt4o.yaml` | `secrets-openai.yaml` |

> Users can extend the matrix by creating additional Agent + SecretStore manifests.

## 2. Prerequisites

### 2.1 Environment Setup

```bash
# Ensure orchestrator and orchestratord are installed
orchestrator --version
orchestratord --version

# Ensure target shells are installed
claude --version       # Claude Code
opencode --version     # OpenCode (for B1)
codex --version        # Codex CLI (for C1)
```

### 2.2 API Key Configuration

Edit `fixtures/benchmarks/secrets-*.yaml` and fill in your API keys:

```bash
# Claude (Anthropic) — uses existing environment variables, no extra config needed
# OpenAI — edit secrets-openai.yaml, fill in OPENAI_API_KEY
# Gemini — edit secrets-gemini.yaml, fill in GEMINI_API_KEY
```

### 2.3 Start Daemon

```bash
orchestratord --foreground --workers 2
```

## 3. Single Benchmark Execution

Example: combination **A1 (Claude Code + Opus)**

### 3.1 Apply Resources

```bash
cd "$ORCHESTRATOR_ROOT"

# Apply SecretStore (model config)
orchestrator apply -f fixtures/benchmarks/secrets-claude-opus.yaml --project benchmark

# Apply Agent (shell + model binding)
orchestrator apply -f fixtures/benchmarks/agent-claude-opus.yaml --project benchmark

# Apply Workflow (with evaluation step)
orchestrator apply -f fixtures/benchmarks/workflow-benchmark-bootstrap.yaml --project benchmark
```

### 3.2 Verify Resources

```bash
orchestrator get workspaces --project benchmark
orchestrator get agents --project benchmark
orchestrator get workflows --project benchmark
```

### 3.3 Create and Run Task

```bash
# Use a uniform goal (same goal for all combinations)
orchestrator task create \
  --project benchmark \
  --workflow benchmark-bootstrap \
  --goal "Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests."

# Record the task ID
TASK_ID=<returned task_id>
```

### 3.4 Monitor Execution

```bash
# Real-time watch
orchestrator task watch "$TASK_ID"

# Follow logs
orchestrator task logs "$TASK_ID" -f

# View step trace
orchestrator task trace "$TASK_ID"

# View item status
orchestrator task items "$TASK_ID"
```

### 3.5 Collect Results

```bash
# Task details (timing, status, cycles)
orchestrator task info "$TASK_ID" -o json > results/A1-task-info.json

# Event stream (benchmark_eval scores)
orchestrator event list --task "$TASK_ID" -o json > results/A1-events.json

# Extract evaluation score
orchestrator event list --task "$TASK_ID" --type step_finished -o json | \
  python3 -c "
import sys, json
events = json.load(sys.stdin)
for e in events:
    p = e.get('payload', {})
    if 'total_score' in str(p):
        print(json.dumps(p, indent=2))
"
```

### 3.6 Cleanup (Optional)

```bash
# Revert git changes to restore pre-benchmark state
git checkout -- .
git clean -fd
```

## 4. Batch Execution

Repeat step 3 for each combination, substituting the corresponding manifest files:

```bash
COMBINATIONS=(
  "A1:agent-claude-opus.yaml:secrets-claude-opus.yaml"
  "A2:agent-claude-sonnet.yaml:secrets-claude-sonnet.yaml"
  "B1:agent-opencode-opus.yaml:secrets-claude-opus.yaml"
  "C1:agent-codex-gpt4o.yaml:secrets-openai.yaml"
)

mkdir -p results

for combo in "${COMBINATIONS[@]}"; do
  IFS=':' read -r id agent_file secret_file <<< "$combo"
  echo "=== Running benchmark: $id ==="

  # Clean workspace
  git checkout -- . && git clean -fd

  # Apply resources
  orchestrator apply -f "fixtures/benchmarks/$secret_file" --project benchmark
  orchestrator apply -f "fixtures/benchmarks/$agent_file" --project benchmark
  orchestrator apply -f fixtures/benchmarks/workflow-benchmark-bootstrap.yaml --project benchmark

  # Create task
  TASK_ID=$(orchestrator task create \
    --project benchmark \
    --workflow benchmark-bootstrap \
    --goal "Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests." \
    2>&1 | grep -oP 'task_id: \K\S+')

  echo "Task $id: $TASK_ID"

  # Wait for completion
  orchestrator task watch "$TASK_ID" --timeout 1800

  # Collect results
  orchestrator task info "$TASK_ID" -o json > "results/${id}-task-info.json"
  orchestrator event list --task "$TASK_ID" -o json > "results/${id}-events.json"
done
```

## 5. Results Comparison

### 5.1 Evaluation Dimensions

| Dimension | Data Source | Description |
|-----------|------------|-------------|
| **Completion** | `task info` → `status` | completed vs failed |
| **Duration** | `task info` → `started_at` / `completed_at` | End-to-end time |
| **Cycles** | `event list` → `cycle_completed` events | Iteration count |
| **Eval Score** | `event list` → `benchmark_eval` step output | 0-100 structured score |
| **Code Quality** | Eval JSON → `code_quality` field | 0-20 subjective score |
| **Diff Size** | `git diff --stat` | Scope of changes |
| **Build/Tests** | Eval JSON → `compilation` / `tests` | Pass/fail |

### 5.2 Comparison Matrix Template

| Combo | Shell | Model | Status | Duration | Cycles | Score | Build | Tests | Lint | Diff | Quality |
|-------|-------|-------|--------|----------|--------|-------|-------|-------|------|------|---------|
| A1 | Claude Code | Opus 4.6 | | | | | | | | | |
| A2 | Claude Code | Sonnet 4.6 | | | | | | | | | |
| B1 | OpenCode | Opus 4.6 | | | | | | | | | |
| C1 | Codex CLI | GPT-4o | | | | | | | | | |

### 5.3 Deep Evaluation (Optional)

For each combination's output, use Claude Code as an independent reviewer:

```bash
# Deep code review of the diff
git diff HEAD~1 | claude -p "Review this diff for code quality, security, performance, and maintainability. Score each dimension 0-10 and provide a total."
```

## 6. Notes

- **Control variables**: Change only one variable (model or shell) at a time
- **Identical goal**: All combinations use the exact same `--goal` string
- **Environment isolation**: `git checkout -- . && git clean -fd` before each run
- **Timeout**: `task watch --timeout 1800` (30 min) to prevent infinite runs
- **Cost awareness**: Opus is ~5x more expensive than Sonnet; GPT-4o is comparable to Sonnet; estimate costs before batch runs
- **Reproducibility**: All manifests are versioned in `fixtures/benchmarks/` for exact reproduction
