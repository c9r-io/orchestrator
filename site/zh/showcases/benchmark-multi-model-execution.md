# 多模型 × 多外壳 SDLC Benchmark 执行计划

> **使用方式**：在 Claude Code 中打开本项目，然后要求 Claude Code 读取并执行本执行计划。Claude Code 将自动完成资源部署、任务执行、监控和结果评估的全流程。

## 1. Benchmark 目标

在相同的任务目标下，依次使用不同的 LLM 模型和 AI 编码外壳执行 self-bootstrap workflow，收集执行结果，最后由 Claude Code 进行统一的深度评估和对比分析。

## 2. 变量矩阵

| ID | 外壳 | 模型 | Agent Manifest | SecretStore |
|----|------|------|----------------|-------------|
| A1 | Claude Code | Opus 4.6 | `fixtures/benchmarks/agent-claude-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| A2 | Claude Code | Sonnet 4.6 | `fixtures/benchmarks/agent-claude-sonnet.yaml` | `fixtures/benchmarks/secrets-claude-sonnet.yaml` |
| B1 | OpenCode | Opus 4.6 | `fixtures/benchmarks/agent-opencode-opus.yaml` | `fixtures/benchmarks/secrets-claude-opus.yaml` |
| C1 | Codex CLI | GPT-4o | `fixtures/benchmarks/agent-codex-gpt4o.yaml` | `fixtures/benchmarks/secrets-openai.yaml` |

> 可按需扩展：创建新的 Agent + SecretStore manifest 即可。

## 3. 前置条件

执行者（Claude Code）应首先验证以下条件：

- `orchestrator --version` 和 `orchestratord --version` 可执行
- daemon 正在运行（`orchestrator daemon status`），如未运行则启动：`orchestratord --foreground --workers 2 &`
- 矩阵中涉及的外壳已安装（`claude --version`、`opencode --version` 等）
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

所有组合执行完成后，Claude Code 应对 `results/` 目录下的全部产出进行统一评估。

### 6.1 定量指标（从 JSON 结果中提取）

| 指标 | 数据来源 |
|------|----------|
| 完成状态 | `task info` → `status` (completed/failed) |
| 总耗时 | `task info` → `started_at` 到 `completed_at` |
| 执行轮次 | `event list` → `cycle_completed` 事件计数 |
| 步骤成功率 | `event list` → `step_finished` 中 `success: true` 的比例 |

### 6.2 代码质量评估（Claude Code 直接执行）

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
| 组合 | 外壳 | 模型 | 状态 | 耗时 | 轮次 | 编译 | 测试 | Lint | Diff行数 | 代码质量评分(0-10) | 备注 |
|------|------|------|------|------|------|------|------|------|----------|-------------------|------|
| A1   | ...  | ...  | ...  | ...  | ...  | ...  | ...  | ...  | ...      | ...               | ...  |
```

最后给出总结分析：各组合的优劣势、模型维度和外壳维度的对比发现、推荐配置。

## 7. 约束

- **控制变量**：每次只改变一个变量（模型或外壳），workflow 和 goal 保持不变
- **环境隔离**：每个组合执行前后恢复干净的 git 状态
- **超时保护**：单次任务 30 分钟超时
- **成本意识**：Opus ≈ 5× Sonnet 成本；批量执行前确认预算
- **可复现性**：所有 manifest 版本化在 `fixtures/benchmarks/`
