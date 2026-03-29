# 多模型 × 多外壳 SDLC Benchmark 执行计划

> **Harness Engineering 执行计划**：本文档是一个 agent 可执行场景，用来展示 orchestrator 这个 control plane 如何组织环境、工作流、约束与反馈闭环，而不是一次性的 prompt 调用。
>
> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果评估，全程自主完成。

## 1. Benchmark 目标

通过控制变量法，在相同的任务目标和 Workflow 下，分别替换 **LLM 模型**和 **Agent 外壳**，评估两个独立维度的性能差异：

- **模型维度**：固定外壳（如 Claude Code），替换模型（Opus / Sonnet / GLM-5 / Gemini / GPT-5.4），观察模型能力对任务完成度和代码质量的影响
- **外壳维度**：固定模型（如 Opus 4.6），替换外壳（Claude Code / OpenCode / Codex / Gemini CLI），观察外壳工具链对执行效率和结果的影响

## 2. 变量矩阵

| ID | 外壳 | 模型 | Agent Manifest | SecretStore |
|----|------|------|----------------|-------------|
| A1 | Claude Code | Opus 4.6 | `fixtures/benchmarks/agent-claude-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| B1 | OpenCode | Opus 4.6 | `fixtures/benchmarks/agent-opencode-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| C1 | OpenCode | GLM-5 | `fixtures/benchmarks/agent-opencode-glm5.yaml` | `fixtures/benchmarks/secrets-glm5.yaml` |
| D1 | Gemini CLI | Gemini 3.1 Pro | `fixtures/benchmarks/agent-gemini-pro.yaml` | `fixtures/benchmarks/secrets-gemini.yaml` |
| E1 | Codex CLI | GPT-5.4 | `fixtures/benchmarks/agent-codex-gpt54.yaml` | `fixtures/benchmarks/secrets-openai.yaml` |

**控制变量分析组：**

| 对比 | 固定变量 | 变化变量 | 观察目标 |
|------|----------|----------|----------|
| A1 vs B1 | Opus 4.6 | Claude Code vs OpenCode | 外壳差异 |
| B1 vs C1 | OpenCode | Opus 4.6 vs GLM-5 | 模型差异 |
| A1 vs D1 vs E1 | — | 全组合 | 综合差异 |

> 可按需扩展：创建新的 Agent + SecretStore manifest 即可。

## 3. 前置条件

执行者（Agent）应首先验证以下条件：

- `orchestrator --version` 和 `orchestratord --version` 可执行
- daemon 正在运行（`orchestrator daemon status`），如未运行则启动：`orchestratord --foreground --workers 2 &`
- 矩阵中涉及的外壳已安装（`claude --version`、`opencode --version`、`gemini --version`、`codex --version` 等）
- `fixtures/benchmarks/secrets-*.yaml` 中的 API 密钥已填入（非 `<placeholder>` 值）

## 4. 统一任务目标

所有组合使用完全相同的 goal（控制变量）：

```
Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests.
```

## 5. 执行流程（逐组合执行）

对矩阵中的每个组合 ID，按以下步骤执行：

### 5.1 环境准备

```bash
cd "$ORCHESTRATOR_ROOT"
git stash --include-untracked || true
```

### 5.2 部署资源

```bash
orchestrator apply -f fixtures/benchmarks/<secret_file> --project benchmark
orchestrator apply -f fixtures/benchmarks/<agent_file> --project benchmark
orchestrator apply -f fixtures/benchmarks/workflow-benchmark-bootstrap.yaml --project benchmark
```

验证：

```bash
orchestrator get agents --project benchmark -o json
orchestrator get workflows --project benchmark -o json
```

### 5.3 创建任务

```bash
orchestrator task create \
  --project benchmark \
  --workflow benchmark-bootstrap \
  --goal "Implement a retry mechanism for gRPC client connections with exponential backoff and configurable max retries. Add unit tests."
```

记录返回的 `task_id`。

### 5.4 监控至完成

```bash
orchestrator task watch <task_id> --timeout 1800
```

如超时或失败，记录状态后继续下一个组合。

### 5.5 收集结果

```bash
orchestrator task info <task_id> -o json
orchestrator event list --task <task_id> -o json
orchestrator task items <task_id> -o json
orchestrator task trace <task_id> --json
```

### 5.6 保存产出物快照

```bash
git diff > results/<combo_id>-diff.patch
git diff --stat > results/<combo_id>-diffstat.txt
```

### 5.7 恢复环境

```bash
git checkout -- .
git clean -fd
git stash pop || true
```

重复 5.1–5.7 直到所有组合执行完毕。

## 6. 评估阶段

所有组合执行完成后，Agent 应对 `results/` 目录下的全部产出进行统一评估。

### 6.1 定量指标（从 JSON 结果中提取）

| 指标 | 数据来源 |
|------|----------|
| 完成状态 | `task info` → `status` (completed/failed) |
| 总耗时 | `task info` → `started_at` 到 `completed_at` |
| 执行轮次 | `event list` → `cycle_completed` 事件计数 |
| 步骤成功率 | `event list` → `step_finished` 中 `success: true` 的比例 |

### 6.2 代码质量评估（Agent 直接执行）

对每个组合的 `results/<combo_id>-diff.patch`：

1. **编译检查**：`cargo build --release` 是否通过
2. **测试检查**：`cargo test --workspace` 是否通过
3. **Lint 检查**：`cargo clippy --workspace -- -D warnings` 是否通过
4. **Diff 审查**：阅读 patch 文件，评估：
   - 实现是否正确完整
   - 代码是否简洁、惯用
   - 是否有不必要的变更
   - 错误处理是否充分
   - 测试覆盖是否合理

### 6.3 输出评估报告

以 markdown 表格格式输出对比结果：

```markdown
| 组合 | 外壳 | 模型 | 状态 | 耗时 | 轮次 | 编译 | 测试 | Lint | Diff行数 | 代码质量(0-10) | 备注 |
|------|------|------|------|------|------|------|------|------|----------|---------------|------|
| A1   | Claude Code  | Opus 4.6      | | | | | | | | | |
| B1   | OpenCode     | Opus 4.6      | | | | | | | | | |
| C1   | OpenCode     | GLM-5         | | | | | | | | | |
| D1   | Gemini CLI   | Gemini 3.1 Pro| | | | | | | | | |
| E1   | Codex CLI    | GPT-5.4       | | | | | | | | | |
```

最后给出总结分析，分两个维度：

1. **模型维度**（对比 B1 vs C1：同一外壳 OpenCode，不同模型）：模型能力对结果的影响
2. **外壳维度**（对比 A1 vs B1：同一模型 Opus，不同外壳）：工具链对执行效率的影响
3. **综合排名**：所有组合的推荐程度

## 7. 约束

- **控制变量**：每次只改变一个变量（模型或外壳），workflow 和 goal 保持不变
- **环境隔离**：每个组合执行前后恢复干净的 git 状态
- **超时保护**：单次任务 30 分钟超时
- **成本意识**：Opus ≈ 5× Sonnet 成本；批量执行前确认预算
- **可复现性**：所有 manifest 版本化在 `fixtures/benchmarks/`
