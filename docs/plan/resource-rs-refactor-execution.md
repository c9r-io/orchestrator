# resource.rs 重构任务执行记录

本文档记录使用 Orchestrator self-bootstrap workflow 执行 `resource.rs` 模块拆分重构的完整过程、遇到的问题及解决方案。

---

## 1. 任务目标

> resource.rs 作为 1,895 行最大单文件，6 种资源类型的逻辑杂糅，需要完整，优雅，解耦的重构

---

## 2. Agent 配置策略

全部使用 MiniMax 模型，优先验证流程可行性，降低实验成本：

| Agent | Runner | Model | 负责阶段 | 选型理由 |
|-------|--------|-------|---------|---------|
| architect | `opencode run` | MiniMax-M2.5-highspeed | plan, qa_doc_gen | 实验阶段，先验证流程 |
| coder | `opencode run` | MiniMax-M2.5-highspeed | implement, ticket_fix, align_tests | 编码执行任务，MiniMax 性价比高 |
| tester | `opencode run` | MiniMax-M2.5-highspeed | qa_testing | QA 场景执行，不需要顶级推理 |
| reviewer | `opencode run` | MiniMax-M2.5-highspeed | doc_governance, review, loop_guard | 文档审计和代码审查 |

配置文件：`docs/workflow/self-bootstrap.yaml`

---

## 3. 执行步骤

### 3.1 构建 & 初始化

```bash
cd /Volumes/Yotta/ai_native_sdlc

# 构建 CLI
cd core && cargo build --release && cd ..

# 初始化运行时（如已有旧数据，先 reset）
./scripts/orchestrator.sh db reset -f --include-config --include-history
./scripts/orchestrator.sh init -f

# 应用配置
./scripts/orchestrator.sh apply -f docs/workflow/self-bootstrap.yaml
```

验证资源加载：

```bash
./scripts/orchestrator.sh get agent
./scripts/orchestrator.sh get workflow
./scripts/orchestrator.sh get workspace
```

预期输出：workspace `self`、agents `architect/coder/tester/reviewer`、workflow `self-bootstrap`。

### 3.2 创建并启动任务

```bash
./scripts/orchestrator.sh task create \
  -n "refactor-resource-rs" \
  -w self -W self-bootstrap \
  --no-start \
  -g "resource.rs 作为1,895行最大单文件，6 种资源类型的逻辑杂糅，需要完整，优雅，解耦的重构" \
  -t core/src/resource.rs

./scripts/orchestrator.sh task start <task_id>
```

### 3.3 监控进度

```bash
# 任务列表
./scripts/orchestrator.sh task list

# 详细状态
./scripts/orchestrator.sh task info <task_id> -o json

# 日志
./scripts/orchestrator.sh task logs <task_id> --tail 50

# 进程监控
ps aux | grep -E "claude|opencode" | grep -v grep
```

### 3.4 检查代码变更

```bash
# git diff
git diff --stat

# 检查新建的 resource/ 模块目录
ls -la core/src/resource/

# 确认编译通过
cd core && cargo check && cargo test --lib
```

---

## 4. Workflow 阶段流水线

### 步骤执行模型

> **Unified Step Execution Model**（设计文档 13）：步骤以字符串 `id` 标识，行为由 `StepBehavior` 数据结构声明，通过 `StepExecutionAccumulator` 驱动统一执行循环。已删除 `WorkflowStepType` 枚举。

### Segment-Based 执行（设计文档 12）

步骤按 `scope` 分组为连续 segment：
- **Task scope**（`scope: task`）：每轮执行一次
- **Item scope**（`scope: item`）：按 QA 文件 fan out

```
Segment 1 (Task):     plan → qa_doc_gen → implement → self_test
Segment 2 (Item):     qa_testing → ticket_fix
Segment 3 (Task):     align_tests → doc_governance
```

### 2-Cycle 策略

| 阶段 | Cycle 1（production） | Cycle 2（validation） |
|------|----------------------|-----------------------|
| plan | ✅ 运行 | ✅ 运行（复审前轮 diff） |
| qa_doc_gen | ✅ 运行 | ✅ 运行 |
| implement | ✅ 运行 | ✅ 运行（迭代改进） |
| self_test | ✅ 运行 | ✅ 运行 |
| qa_testing | ⏭ 跳过（prehook: is_last_cycle） | ✅ fan out per QA file |
| ticket_fix | ⏭ 跳过 | ✅ 仅当有 ticket 时 |
| align_tests | ⏭ 跳过 | ✅ 运行 |
| doc_governance | ⏭ 跳过 | ✅ 运行 |
| loop_guard | ✅ 运行 | ✅ 运行 → 终止 |

---

## 5. 第一轮执行记录（2026-02-27）

### 问题：implement 阶段因权限阻塞而卡住

**现象**：
- plan 和 qa_doc_gen（architect/opus）正常完成，耗时约 10.5 分钟
- implement 阶段（当时使用 claude --model sonnet）运行 25+ 分钟，0 字节 stdout，0 文件变更

**根因**：
- Claude Code 以 `tty: false` 运行，无法交互式批准文件写入权限
- 项目缺少 `.claude/settings.local.json`（只有 `.example` 文件）
- 进程持续消耗 API 调用但无法写入任何文件

**消耗的进程**：

| PID | 阶段 | 模型 | 时长 | 结果 |
|-----|------|------|------|------|
| 71564 | plan | opus | ~6 min | 正常完成 |
| 75607 | qa_doc_gen | opus | ~4.5 min | 完成（未写文件） |
| 78803 | implement | sonnet | ~25 min | 卡住，手动 kill |

**Plan 产出**（architect/opus 的规划结果）：

将 `resource.rs` (1,895 行) 拆分为 7 个文件：

| 文件 | 行数 | 内容 |
|------|------|------|
| `mod.rs` | ~200 | 核心类型、Resource trait、RegisteredResource dispatch、registry |
| `common.rs` | ~50 | 共享 helpers (metadata, apply_to_map, serializes_equal) |
| `workspace.rs` | ~220 | WorkspaceResource impl + builder + conversions + 6 tests |
| `agent.rs` | ~335 | AgentResource impl + builder + conversions + 3 tests |
| `workflow.rs` | ~490 | WorkflowResource impl + builder + conversions + parse helpers + 7 tests |
| `policy.rs` | ~255 | Project + Defaults + RuntimePolicy (3 simpler types grouped) |
| `api.rs` | ~145 | 公共 API: kind_as_str, parse_yaml, export_manifest_* |

### 解决方案

1. 将所有 agent 从 Claude Code 切换为 `opencode run --model minimax-coding-plan/MiniMax-M2.5-highspeed`，避免 Claude Code 的权限问题
2. MiniMax 性价比更高，适合实验阶段

---

## 6. 经验教训

1. **Claude Code 非交互模式的权限问题**：`tty: false` 时 Claude Code 无法获得文件写入授权。解决方案是使用 opencode（无此限制）或配置 `.claude/settings.local.json` 预授权。

2. **成本控制**：实验阶段全部使用 MiniMax 模型验证流程可行性，降低成本。待流程稳定后再评估是否对 plan 阶段升级为 Opus。

3. **进程监控要点**：
   - 检查 stdout 字节数是否增长
   - 检查 CPU 使用率是否合理（持续 < 1% 可能表示卡住）
   - 检查 `git diff --stat` 是否有新变更
   - 检查目标目录是否有新文件产生

4. **卡住进程的处理**：直接 `kill <pid>` 即可，子进程（rust-analyzer、MCP servers）会级联退出。

5. **CLI 入口**：始终使用 `./scripts/orchestrator.sh` 而非直接调用 `./core/target/release/agent-orchestrator`，确保路径和环境变量一致。
