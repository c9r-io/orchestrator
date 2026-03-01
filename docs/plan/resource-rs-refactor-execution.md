# resource.rs 重构任务执行记录

本文档记录使用 Orchestrator self-bootstrap workflow 执行 `resource.rs` 模块拆分重构的完整过程、遇到的问题及解决方案。

---

## 1. 任务目标

> resource.rs 已完成第一阶段拆分，但当前结果仅达到“可用的结构化拆分”，尚未完全满足“完整，优雅，解耦”。
> 下一阶段目标不再是继续机械拆文件，而是完成资源模块的质量收口：补齐语义完整性、消除隐式耦合、收敛重复样板，使 `core/src/resource/` 成为稳定的长期边界。

### 1.1 新目标（质量收口版）

#### A. 完整性（必须达成）

- 修复 `WorkflowConfig -> WorkflowSpec` 的有损转换，确保 `safety` 字段完整往返：
  - `max_consecutive_failures`
  - `auto_rollback`
  - `checkpoint_strategy`
  - `step_timeout_secs`
  - `binary_snapshot`
- 为 workflow 资源补齐真正的 round-trip 验证：`spec -> config -> spec` 后关键字段不能丢失。
- 新增覆盖导出链路的测试，确保 `export_manifest_resources` / `export_manifest_documents` 导出的 workflow manifest 保留完整 `safety` 配置。

#### B. 解耦（必须达成）

- 消除 `export.rs -> workflow.rs -> workflow_convert.rs` 的隐式穿透依赖。
- 将 workflow 的转换职责提炼为明确的共享边界，避免通过 `pub(super) use` 暴露“仅为兄弟模块访问”的内部函数。
- 目标是让调用关系更直观：
  - `workflow.rs` 负责资源生命周期（validate/apply/get/delete）
  - 独立的 converter 负责 `spec <-> config`
  - `export.rs` 只依赖稳定的公共转换接口，不依赖 workflow 模块内部细节

#### C. 优雅性（应达成）

- 减少各 Resource 模块中重复的样板逻辑，重点关注以下重复模式：
  - `metadata` 恢复逻辑
  - `resource_meta` 写回逻辑
  - `build_*` 的 kind/spec 校验模板
  - `get_from` / `delete_from` 的重复结构
- 在不引入过度抽象的前提下，提取少量共享 helper 或统一模式，降低未来新增资源类型的维护成本。
- 控制模块职责清晰度，避免“逻辑拆散但心智模型更复杂”。

### 1.2 验收标准

本轮改造完成后，至少应满足以下条件：

1. `cargo check` 通过，且无新增 warning。
2. `cargo test --lib resource` 全量通过。
3. 新增测试明确覆盖 workflow `safety` 的反向转换与导出保真。
4. `workflow_config_to_spec` 不再使用 `SafetySpec::default()` 直接兜底丢弃实际配置。
5. `export.rs` 不再依赖通过 `workflow.rs` 转手暴露的内部转换函数。
6. 代码审查视角下，`core/src/resource/` 的模块边界能够清晰说明“谁负责资源生命周期，谁负责转换，谁负责导入导出”。

### 1.3 非目标

本轮不追求：

- 再次大规模拆分文件数量
- 引入复杂泛型框架或宏系统来消灭所有重复代码
- 改动 `resource` 模块之外的大范围调用方

本轮重点是“补齐正确性 + 收敛边界 + 提升可维护性”，而不是继续追求更细碎的文件颗粒度。

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
