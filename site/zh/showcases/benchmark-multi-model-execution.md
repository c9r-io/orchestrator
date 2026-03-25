# 多模型 × 多外壳 SDLC Benchmark 执行计划

本文档定义了一个可重复的 benchmark 框架，用于在相同任务目标下对比不同 LLM 模型和 AI 编码外壳的表现。

## 1. 变量矩阵

| 维度 | 变量 | 控制方式 |
|------|------|----------|
| **模型** | claude-opus-4-6, claude-sonnet-4-6, gpt-4o, gemini-2.5-pro | SecretStore `ANTHROPIC_MODEL` / provider env |
| **外壳** | Claude Code, OpenCode, Codex CLI, Gemini CLI | Agent `spec.command` |
| **任务** | self-bootstrap (线性迭代), self-evolution (竞争选择) | Workflow manifest |

### 预定义组合

| ID | 外壳 | 模型 | Agent Manifest | SecretStore |
|----|------|------|----------------|-------------|
| A1 | Claude Code | Opus 4.6 | `agent-claude-opus.yaml` | `secrets-claude-opus.yaml` |
| A2 | Claude Code | Sonnet 4.6 | `agent-claude-sonnet.yaml` | `secrets-claude-sonnet.yaml` |
| B1 | OpenCode | Opus 4.6 | `agent-opencode-opus.yaml` | `secrets-claude-opus.yaml` |
| C1 | Codex CLI | GPT-4o | `agent-codex-gpt4o.yaml` | `secrets-openai.yaml` |

> 用户可按需扩展矩阵，只需创建新的 Agent + SecretStore manifest。

## 2. 前置条件

### 2.1 环境准备

```bash
# 确保 orchestrator 和 orchestratord 已安装
orchestrator --version
orchestratord --version

# 确保目标外壳已安装
claude --version       # Claude Code
opencode --version     # OpenCode (如测试 B1)
codex --version        # Codex CLI (如测试 C1)
```

### 2.2 API 密钥配置

编辑 `fixtures/benchmarks/secrets-*.yaml`，填入实际的 API 密钥：

```bash
# Claude (Anthropic) — 使用已有的环境变量即可，无需额外配置
# OpenAI — 编辑 secrets-openai.yaml，填入 OPENAI_API_KEY
# Gemini — 编辑 secrets-gemini.yaml，填入 GEMINI_API_KEY
```

### 2.3 Daemon 启动

```bash
orchestratord --foreground --workers 2
```

## 3. 单次 Benchmark 执行步骤

以组合 **A1（Claude Code + Opus）** 为例：

### 3.1 应用资源

```bash
cd "$ORCHESTRATOR_ROOT"

# 应用 SecretStore（模型配置）
orchestrator apply -f fixtures/benchmarks/secrets-claude-opus.yaml --project benchmark

# 应用 Agent（外壳 + 模型绑定）
orchestrator apply -f fixtures/benchmarks/agent-claude-opus.yaml --project benchmark

# 应用 Workflow（含评估步骤）
orchestrator apply -f fixtures/benchmarks/workflow-benchmark-bootstrap.yaml --project benchmark
```

### 3.2 验证资源加载

```bash
orchestrator get workspaces --project benchmark
orchestrator get agents --project benchmark
orchestrator get workflows --project benchmark
```

### 3.3 创建并执行任务

```bash
# 使用统一的目标描述（所有组合使用相同的 goal）
orchestrator task create \
  --project benchmark \
  --workflow benchmark-bootstrap \
  --goal "Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests."

# 记录 task ID
TASK_ID=<返回的 task_id>
```

### 3.4 监控执行

```bash
# 实时观察
orchestrator task watch "$TASK_ID"

# 查看日志
orchestrator task logs "$TASK_ID" -f

# 查看步骤轨迹
orchestrator task trace "$TASK_ID"

# 查看 item 状态
orchestrator task items "$TASK_ID"
```

### 3.5 收集结果

```bash
# 任务详情（耗时、状态、轮次）
orchestrator task info "$TASK_ID" -o json > results/A1-task-info.json

# 事件流（benchmark_eval 分数）
orchestrator event list --task "$TASK_ID" -o json > results/A1-events.json

# 提取评估分数
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

### 3.6 清理（可选）

```bash
# 回滚 git 变更，恢复到 benchmark 前状态
git checkout -- .
git clean -fd

# 删除任务（保留数据库中的结果供对比）
# orchestrator task delete "$TASK_ID" --force
```

## 4. 批量执行

对矩阵中的每个组合重复步骤 3，替换对应的 manifest 文件：

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

  # 清理工作区
  git checkout -- . && git clean -fd

  # 应用资源
  orchestrator apply -f "fixtures/benchmarks/$secret_file" --project benchmark
  orchestrator apply -f "fixtures/benchmarks/$agent_file" --project benchmark
  orchestrator apply -f fixtures/benchmarks/workflow-benchmark-bootstrap.yaml --project benchmark

  # 创建任务
  TASK_ID=$(orchestrator task create \
    --project benchmark \
    --workflow benchmark-bootstrap \
    --goal "Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests." \
    2>&1 | grep -oP 'task_id: \K\S+')

  echo "Task $id: $TASK_ID"

  # 等待完成
  orchestrator task watch "$TASK_ID" --timeout 1800

  # 收集结果
  orchestrator task info "$TASK_ID" -o json > "results/${id}-task-info.json"
  orchestrator event list --task "$TASK_ID" -o json > "results/${id}-events.json"
done
```

## 5. 结果对比

### 5.1 评估维度

| 维度 | 数据来源 | 说明 |
|------|----------|------|
| **完成率** | `task info` → `status` | completed vs failed |
| **总耗时** | `task info` → `started_at` / `completed_at` | 端到端时间 |
| **循环数** | `event list` → `cycle_completed` 事件数 | 迭代轮次 |
| **评估分数** | `event list` → `benchmark_eval` 步骤输出 | 0-100 结构化评分 |
| **代码质量** | 评估 JSON → `code_quality` 字段 | 0-20 主观评分 |
| **Diff 大小** | `git diff --stat` | 变更范围 |
| **编译/测试** | 评估 JSON → `compilation` / `tests` | 是否通过 |

### 5.2 对比矩阵模板

| 组合 | 外壳 | 模型 | 状态 | 耗时 | 循环 | 总分 | 编译 | 测试 | Lint | Diff | 代码质量 |
|------|------|------|------|------|------|------|------|------|------|------|----------|
| A1 | Claude Code | Opus 4.6 | | | | | | | | | |
| A2 | Claude Code | Sonnet 4.6 | | | | | | | | | |
| B1 | OpenCode | Opus 4.6 | | | | | | | | | |
| C1 | Codex CLI | GPT-4o | | | | | | | | | |

### 5.3 深度评估（可选）

对于每个组合的产出代码，可使用 Claude Code 作为独立评估者进行代码审查：

```bash
# 使用 Claude Code 对 diff 进行深度审查
git diff HEAD~1 | claude -p "Review this diff for code quality, security, performance, and maintainability. Score each dimension 0-10 and provide a total."
```

## 6. 注意事项

- **控制变量**：每次只改变一个变量（模型或外壳），其他保持不变
- **相同目标**：所有组合使用完全相同的 `--goal` 字符串
- **环境隔离**：每次执行前 `git checkout -- . && git clean -fd` 恢复干净状态
- **超时控制**：`task watch --timeout 1800`（30 分钟）防止无限运行
- **成本意识**：Opus 比 Sonnet 贵约 5x，GPT-4o 与 Sonnet 相当；批量测试前估算成本
- **可复现性**：所有 manifest 已版本化在 `fixtures/benchmarks/`，可精确复现
