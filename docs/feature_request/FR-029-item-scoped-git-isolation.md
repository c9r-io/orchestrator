# FR-029: Item-Scoped Git 工作目录隔离

**优先级**: P0
**状态**: Proposed
**目标**: 为 self-evolution workflow 中的 item-scoped 步骤提供 git 级别的工作目录隔离，防止候选方案相互干扰

## 背景与目标

self-evolution workflow 的核心机制是让多个候选方案（item）各自独立实现，然后通过 benchmark 评分竞争选择。这要求每个 item 的实现过程相互隔离，互不干扰。

当前实现中，`evo_implement` 和 `evo_benchmark` 均为 `scope: item` 步骤，按 `max_parallel: 1` 顺序执行。但所有 item 共享同一个 git 工作目录（workspace `root_path: "."`），导致：

1. **approach-a 的实现被 approach-b 覆盖**：approach-a 完成后，approach-b 在同一目录修改同一批文件，approach-a 的代码变更丢失。
2. **benchmark 评估失真**：`evo_benchmark` 在 approach-b 实现后运行时，只能看到 approach-b 的代码，approach-a 的 benchmark 实际评估的也是 approach-b 的代码（或混合状态）。
3. **违反测试计划 5.2 节要求**：计划文档明确要求"没有相互干扰（item-scoped 隔离）"。

这是 self-evolution 机制正确性的基础前提——没有隔离，竞争选择就没有意义。

目标：

- 每个 item 的 `evo_implement` + `evo_benchmark` 步骤在独立的 git 上下文中执行。
- 各 item 的代码变更互不影响。
- `item_select` 选出 winner 后，winner 的变更能干净地应用到主工作目录。
- 方案应适用于任何需要 item 隔离的 workflow，不仅限于 self-evolution。

非目标：

- 不实现完整的并行 git worktree 管理器（本轮仅支持顺序执行）。
- 不改变 `item_select` 或 `generate_items` 的逻辑。
- 不引入容器级隔离（git 级别足够）。

## 实施方案

### 方案 A：Git Branch-Per-Item（推荐）

在 item-scoped segment 执行前，引擎为每个 item 创建独立的 git branch，执行时切换到对应 branch，执行后保存变更到该 branch。

**执行流程**：

```text
evo_plan 完成 → items_generated (approach-a, approach-b)
  │
  ├─ [引擎] git tag evo-base-{task_id}              # 标记基准点
  │
  ├─ item: approach-a
  │   ├─ [引擎] git checkout -b evo-item-approach-a evo-base-{task_id}
  │   ├─ evo_implement (agent 在此 branch 上修改代码)
  │   ├─ [引擎] git add -A && git commit -m "evo: approach-a implementation"
  │   ├─ evo_benchmark (评估此 branch 的代码)
  │   └─ [引擎] 记录 branch name 到 item pipeline_vars
  │
  ├─ item: approach-b
  │   ├─ [引擎] git checkout -b evo-item-approach-b evo-base-{task_id}
  │   ├─ evo_implement (agent 在此 branch 上修改代码)
  │   ├─ [引擎] git add -A && git commit -m "evo: approach-b implementation"
  │   ├─ evo_benchmark (评估此 branch 的代码)
  │   └─ [引擎] 记录 branch name 到 item pipeline_vars
  │
  └─ [引擎] git checkout 回到原始 branch
      │
      select_best → winner = approach-b
      │
      evo_apply_winner
        ├─ [引擎或 agent] git merge evo-item-approach-b
        └─ 清理临时 branch
```

**配置方式**：在 workflow 或 step 级别增加隔离策略配置：

```yaml
# Workflow 级别
spec:
  item_isolation:
    strategy: git_branch        # git_branch | git_worktree | none
    branch_prefix: "evo-item"
    auto_commit: true           # 步骤完成后自动 commit
    cleanup: after_select       # after_select | manual | never
```

或在 step 级别：

```yaml
- id: evo_implement
  scope: item
  isolation:
    strategy: git_branch
```

**涉及文件**：
- `core/src/config/workflow.rs` — 新增 `ItemIsolationConfig` 结构体
- `core/src/scheduler/loop_engine/segment.rs` — item segment 执行前后增加 git 操作
- `core/src/scheduler/item_executor/dispatch.rs` — 执行时注入当前 branch 到 context
- `docs/workflow/self-evolution.yaml` — 配置 item_isolation

**优点**：
- 实现简单，git branch 是轻量操作
- 顺序执行时无冲突风险（`max_parallel: 1`）
- winner 通过 `git merge` 应用，Git 原生支持
- branch 保留完整历史，便于审计

**缺点**：
- 顺序执行时需要频繁 checkout（但 `max_parallel: 1` 下无并发问题）
- 并行执行时需要 worktree（本轮不需要支持）

### 方案 B：Git Worktree-Per-Item

为每个 item 创建独立的 `git worktree`，各 item 在自己的目录中执行。

```bash
git worktree add /tmp/evo-approach-a -b evo-item-approach-a
git worktree add /tmp/evo-approach-b -b evo-item-approach-b
```

**优点**：天然支持并行执行。
**缺点**：需要修改 agent 的工作目录（`{source_tree}` 变量指向 worktree 路径），复杂度更高。

### 方案选择

推荐方案 A（Git Branch-Per-Item）。self-evolution 当前使用 `max_parallel: 1` 顺序执行，branch 方案已足够。未来若需要并行执行可升级为方案 B，两者配置兼容。

## CLI / API 影响

- `orchestrator get workflow` 输出增加 `item_isolation` 字段显示。
- 无其他用户可见接口变更。

## 关键设计决策与权衡

### 自动 commit 时机

item 的 `evo_implement` 步骤完成后立即自动 commit，而非等到整个 item segment 结束。原因：
1. `evo_benchmark` 需要在干净的 git 状态下评估 `git diff --stat`。
2. 避免 uncommitted 变更在 branch 切换时丢失。

### Branch 清理策略

`after_select`：在 `item_select` 完成后清理非 winner 的临时 branch，保留 winner branch 直到 `evo_apply_winner` 完成。这样既避免 branch 泄漏，又保留了调试窗口。

### 对 prompt 中 {source_tree} 的影响

方案 A 中所有 item 共享同一个 `{source_tree}` 路径（因为是同一目录的不同 branch），prompt 模板无需修改。方案 B 则需要将 `{source_tree}` 指向 worktree 路径。

## 风险与缓解

风险：agent 在 `evo_implement` 中执行 `git checkout` 或其他 git 操作导致 branch 状态混乱。
缓解：在 prompt 中明确禁止 agent 执行 git branch/checkout 操作；引擎在步骤执行前后验证当前 branch 是否正确。

风险：自动 commit 包含不想要的文件（如临时文件、日志）。
缓解：使用 `.gitignore` 规则过滤；commit 前检查 `git status` 排除非代码文件。

风险：`git merge` winner branch 时出现冲突。
缓解：winner branch 基于 evo-base tag 创建，merge 回原始 branch 时应为 fast-forward 或简单合并。若冲突，`evo_apply_winner` agent 步骤负责解决。

## 验收标准

- 配置 `item_isolation: { strategy: git_branch }` 时，每个 item 在独立 branch 上执行。
- 两个候选 item 的代码变更互不影响（可通过 `git diff` 验证各 branch 内容独立）。
- `evo_benchmark` 评估时看到的是对应 item 的代码变更（不是其他 item 的）。
- `item_select` 选出 winner 后，winner branch 的变更能应用到主工作目录。
- 临时 branch 在 pipeline 完成后清理。
- 未配置 `item_isolation` 时行为不变（向后兼容）。
- `cargo test --workspace` 通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
