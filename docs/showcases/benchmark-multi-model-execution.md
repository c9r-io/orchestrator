# 多模型 × 多外壳 SDLC Benchmark 执行计划

> **Harness Engineering 执行计划**：本文档是一个 agent 可执行场景，用来展示 orchestrator 这个 control plane 如何组织环境、工作流、约束与反馈闭环，而不是一次性的 prompt 调用。
>
> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果评估，全程自主完成。

## 1. Benchmark 目标

通过控制变量法，在相同的任务目标和 Workflow 下，分别替换 **LLM 模型**和 **Agent 外壳**，评估两个独立维度的性能差异：

- **综合维度**：替换外壳+模型组合（OpenCode/MiniMax, Gemini CLI/Flash, Codex/GPT-5.4-mini），观察不同 Agent 外壳和模型对任务完成度、代码质量的综合影响

## 2. 变量矩阵

| ID | 外壳 | 模型 | Agent Manifest | SecretStore |
|----|------|------|----------------|-------------|
| C1 | OpenCode | MiniMax-M2.7-highspeed | `fixtures/benchmarks/agent-opencode-glm5.yaml` | `fixtures/benchmarks/secrets-glm5.yaml` |
| D1 | Gemini CLI | gemini-3-flash-preview | `fixtures/benchmarks/agent-gemini-pro.yaml` | `fixtures/benchmarks/secrets-gemini.yaml` |
| E1 | Codex CLI | gpt-5.4-mini | `fixtures/benchmarks/agent-codex-gpt54.yaml` | `fixtures/benchmarks/secrets-openai.yaml` |

**控制变量分析组：**

| 对比 | 固定变量 | 变化变量 | 观察目标 |
|------|----------|----------|----------|
| C1 vs D1 vs E1 | 相同 goal & workflow | 外壳+模型 | 综合差异 |

> 可按需扩展：创建新的 Agent + SecretStore manifest 即可。

## 3. 前置条件

执行者（Agent）应首先验证以下条件：

- `orchestrator --version` 和 `orchestratord --version` 可执行
- daemon 正在运行（`orchestrator daemon status`），如未运行则启动：`orchestratord --foreground --workers 2 &`
- 矩阵中涉及的外壳已安装（`opencode --version`、`gemini --version`、`codex --version`）
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

### 6.2 六维评估（Agent 直接执行）

Agent 首先执行 `git diff --stat`。若 diff 为空，任务完成度直接记 0 分，其余维度跳过。

| 维度 | 分值 | 评估标准 |
|------|------|----------|
| **任务完成度** | 0-10 | 是否产出了实质代码变更并达成 goal |
| **代码质量** | 0-10 | 实现是否正确、惯用、简洁 |
| **测试覆盖** | 0-10 | 是否有覆盖新增/变更代码的有意义的单元测试 |
| **执行效率** | 0-10 | 端到端 wall time 相对于任务复杂度 |
| **步骤成功率** | 0-10 | 各 workflow step 是否正常退出 |
| **工程规范** | 0-10 | 错误处理、文档注释、安全标注、lint 整洁度 |

Agent 运行项目的 build/test/lint 命令验证后，输出六维 JSON 评分（总分 0-60）。

### 6.3 输出评估报告

以 markdown 表格 + 雷达图格式输出对比结果：

```markdown
| 组合 | 外壳 | 模型 | 状态 | 耗时 | 轮次 | 完成度 | 质量 | 测试 | 效率 | 成功率 | 规范 | 总分(/60) | 备注 |
|------|------|------|------|------|------|--------|------|------|------|--------|------|-----------|------|
| C1   | OpenCode     | MiniMax-M2.7-highspeed  | | | | | | | | | | | |
| D1   | Gemini CLI   | gemini-3-flash-preview  | | | | | | | | | | | |
| E1   | Codex CLI    | gpt-5.4-mini            | | | | | | | | | | | |
```

最后给出总结分析：

1. **综合对比**（C1 vs D1 vs E1）：六维雷达图对比不同外壳+模型组合的优劣势
2. **综合排名**：所有组合的推荐程度

## 7. 约束

- **控制变量**：每次只改变一个变量（模型或外壳），workflow 和 goal 保持不变
- **环境隔离**：每个组合执行前后恢复干净的 git 状态
- **超时保护**：单次任务 30 分钟超时
- **成本意识**：三个组合均使用低成本模型（MiniMax-highspeed / Flash / mini），单次任务成本可控
- **可复现性**：所有 manifest 版本化在 `fixtures/benchmarks/`

## 8. 实例：一键执行 Benchmark

### 8.1 用户前置准备（手动完成）

在将 prompt 交给 AI 编码 Agent 之前，用户需自行完成以下认证和环境准备：

**认证各 Agent CLI**（按需选择你要测试的外壳）：

| 外壳 | 认证方式 |
|------|----------|
| OpenCode | `opencode auth` 交互式配置 provider 和 API key |
| Gemini CLI | 首次运行 `gemini` 时在工具内完成 Google 账号登录，或设置 `GEMINI_API_KEY` 环境变量 |
| Codex CLI | 首次运行 `codex` 时在工具内完成登录，或设置 `OPENAI_API_KEY` 环境变量 |

**确认环境就绪**：

```bash
# 确认各 CLI 已安装且能正常响应
opencode --version
gemini --version
codex --version

# 确认 orchestrator 已构建安装
orchestrator --version
orchestratord --version

# 确认 SecretStore manifest 中的密钥已填入（非 placeholder）
# 编辑 fixtures/benchmarks/secrets-*.yaml
```

### 8.2 可直接执行的 Prompt

完成上述准备后，在 AI 编码 Agent（如 Claude Code）中粘贴以下 prompt 即可启动全流程：

````
执行 docs/showcases/benchmark-multi-model-execution.md 多模型 benchmark 测试。

## 背景
- 变量矩阵为 3 组：C1 (OpenCode+MiniMax), D1 (Gemini CLI+Flash), E1 (Codex CLI+GPT-5.4-mini)
- Agent manifests / SecretStores / Workflow 位于 fixtures/benchmarks/
- 各 CLI 已认证，遇到认证问题报告给用户

## 执行前清理
1. 重新构建: cargo build --release -p orchestratord -p orchestrator-cli，安装到 ~/.cargo/bin/
2. 重启 daemon: kill 旧进程 → orchestratord --foreground --workers 2
3. 清理 benchmark 项目残留资产:
   - orchestrator task delete --all -p benchmark -f
   - orchestrator get agents/workflows/workspaces -p benchmark → 逐个 delete
4. mkdir -p results

## 执行流程
对 C1 → D1 → E1 逐组执行 showcase 文档中的 5.1-5.7 步骤:
- apply secrets → apply agent → apply workflow
- task create → task watch --timeout 1800
- 收集结果 (task info/event list/task items/task trace -o json)
- git diff 保存到 results/<combo_id>-*
- git checkout/clean 恢复环境（注意保留 results/ 目录）
- delete 当前组 agent（保留 workflow/workspace 共用；如遇 capability 校验错误则一并删除 workflow 重建）

每组之间清理 agent 以避免 capability 冲突。

## 评估
全部组合完成后，按文档 6.1-6.3 节的六维评估标准生成 results/benchmark-report.md。

## 异常处理
- 超时或失败：记录状态后继续下一组
- 认证失败：报告给用户，等待修复后继续
````
