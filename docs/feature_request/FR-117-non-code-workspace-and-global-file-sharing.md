# FR-117: 非代码 Workspace 与全局文件共享 — 通用 Agent 场景解耦

## 优先级: P1

## 状态: Proposed

## 依赖

- FR-093：沙箱可配置读取路径白名单（`readable_paths`，本 FR 的 per-profile 层基础）
- FR-091：Linux Sandbox Filesystem Isolation Backend
- FR-044：Sandbox 写入拒绝检测与 `writable_paths`
- FR-116：Agent Driver 抽象（apply-time capability 校验模式复用；non-QA item 终态走 driver 工具自判）
- `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`：MCP `mark_done` 工具（非文件驱动 item 的收敛信号）
- FR-114 / FR-113：Managed Slack Connection 与 reaction 驱动的 task 创建（本 FR 的首要非代码触发源）

## 计划闭环产物

- `docs/design_doc/orchestrator/128-non-code-workspace-and-global-file-sharing.md`
- `docs/qa/orchestrator/165-non-code-workspace-and-global-file-sharing.md`
- `docs/security/authorization/02-file-sharing-ceiling.md`（新增：全局路径天花板即访问控制边界）
- `docs/security/file-security/02-workspace-home-isolation.md`（新增：`work_dir` 兼作 HOME 的逃逸防护）
- `docs/guide/non-code-workspace.md`（EN + ZH）
- `fixtures/manifests/bundles/non-code-workspace-fixture.yaml`
- `scripts/qa/test-non-code-workspace.sh`
- `docs/architecture.md`、`docs/guide/02-resource-model.md`、`CHANGELOG.md`（更新）

## Background

当前 Workspace 隐含一个「代码项目」前提：`root_path`、`qa_targets`、`ticket_dir` 均为必填字段（`crates/orchestrator-config/src/config/safety.rs:130`），task 的 item 由扫描 `qa_targets` 文件生成（`core/src/task_ops.rs:96-136`）。这排除了任何不涉及代码库的 agent 场景。

### 目标场景

仓库管理员用 Slack 与销售沟通。销售卖出货物后发消息；管理员对该消息打 badge，触发 agent 读取 Slack 消息、查询在库数量、给出回复建议，由人决策。全程不涉及任何代码库——需要的仅是**授权的用户空间 skills** 与一个可选的数据/工作目录。

### 关键发现：git 并非硬前提

勘察表明「git 前提」多数已优雅降级或按配置门控，真正耦合不在 git：

| git 使用点 | 现状 |
|---|---|
| checkpoint/rollback（`safety/checkpoint.rs`） | 仅 `CheckpointStrategy::GitTag` 才调用；**默认 `None`**（`loop_engine/cycle_safety.rs:23`） |
| handoff 快照（`core/src/handoff.rs:24`） | git 失败返回 `"workspace-non-git"` |
| invariant 保护文件检查（`scheduler/invariant.rs:118`） | git 失败返回 `None`（不阻塞） |
| item 隔离 worktree（`loop_engine/isolation.rs`） | 仅配置 `item_isolation` 时 |
| self_referential | 强制 git_tag，仅自举场景 |

真正的硬耦合是两处：**(a) Workspace schema 的 QA/代码味必填字段；(b) item 生成 = 扫描 qa_targets 文件**。因此本 FR 的实质是「把 Workspace 从 code-repo 假设中解耦，让 filesystem/git/QA-file 各自成为可选维度」，而非「去掉 git」。

## Product Decision

**扩展 Workspace，不新建资源 kind；另增一个 daemon 级全局文件共享配置。**

用户答案将 code-repo 与非代码 workspace 的差异压缩为少数可选字段——属同一资源的退化情形，而非两类本质不同的对象。新建独立 CRD 会复制 agents/workflows/health_policy/artifacts_dir 装配并波及 registry、writeback、GUI、资源分发 12 处，不划算。

作用域分层：

```
daemon 全局 (类似 uds-policy.yaml)      ← 新增：FileSharing 天花板 + 全局 skills
  └── Workspace (kind + 可选 work_dir)   ← 扩展：per-project
        └── ExecutionProfile            ← 现有：readable/writable_paths，须 ⊆ 全局天花板
```

### 工作目录与 HOME（依据 UX 决策）

- 每个 task **总是有 cwd**：若定义了可选 `work_dir` 则用它，否则 daemon 分配临时目录作 cwd。因此 runner/sandbox 的 spawn 路径（本就要求 cwd）无需重构。
- 当 `work_dir` 存在时，`work_dir` 兼作 agent 的 `~/`（HOME），`<work_dir>/.claude/skills` 之类成为**本地 skill 源**。

### Skills 两来源

- **全局**：daemon FileSharing 配置声明的用户空间全局 skills 目录，所有 task 只读挂载。
- **本地**：`work_dir`（当存在时）作为 HOME，其下的 skill 目录。

## Goals

- `root_path`（更名/别名为 `work_dir`）、`qa_targets`、`ticket_dir` 改为可选。
- Workspace 新增 `kind` 判别式：`code_repo`（默认，向后兼容）| `task`。
- 非代码 task 采用单一隐式 item 模型；终态由 agent 经 MCP `mark_done` 工具自判，不依赖 QA 的 CEL finalize 规则。
- 新增 daemon 级 FileSharing 全局配置：`globalSkills` 目录 + `shareableRoots` 天花板。
- 沙箱路径解析统一：cwd/HOME、只读集（全局 skills + profile readable）、可写集（work_dir + profile writable），全部受 `shareableRoots` 子集约束。
- apply 时能力门控（复用 FR-116 模式）拒绝所有不合法组合。
- 两条安全不变量写入独立 security 测试文档并纳入验收。

## Non-goals

- 移除或削弱 code_repo workspace 的 git/checkpoint/self_referential 能力（本 FR additive）。
- 为非代码场景提供 QA 文件 item 模型或 ticket 扫描。
- 在没有全局天花板约束的情况下允许任意宿主路径访问。
- 把 `work_dir` 兼作 HOME 扩展成完整的多用户 home 管理。
- 修改 Slack/source 触发链路本身（已由 FR-113/114 承载）；本 FR 只让其目标 workspace 可为非代码。
- 引入容器/Docker 运行时；沙箱仍是 OS 原生（Seatbelt / Linux namespaces）。

## Design

### Workspace 扩展

```yaml
kind: Workspace
metadata: { name: warehouse-ops }
spec:
  kind: task              # code_repo（默认） | task
  work_dir: ~/warehouse-data   # 可选；缺省 → daemon 分配临时 HOME
  # 无 qa_targets / ticket_dir
```

`kind: code_repo` 保持现状语义与全部现有字段，默认值确保既有 manifest 零改动。

### Daemon 全局 FileSharing 配置（新增）

```yaml
# ~/.orchestratord/file-sharing.yaml
fileSharing:
  globalSkills:
    - path: ~/.orchestrator/skills   # 所有 task 只读挂载
  shareableRoots:                    # 天花板：workspace/profile 路径须为其子集
    - ~/.orchestrator/skills
    - ~/warehouse-data
```

### 运行时沙箱解析

- **cwd / HOME** = `work_dir`（有）或分配的临时目录（无）。
- **只读挂载** = 全局 skills 目录 ∪ profile `readable_paths`。
- **可写** = `work_dir` ∪ profile `writable_paths`。
- 上述任何路径必须 ⊆ `shareableRoots`；`HOME`/`XDG_*` 环境变量强制指向 `work_dir`，不泄漏真实用户 home。

### Item 模型

非代码 task 生成单一隐式 item，`goal`/`prompt` 即工作内容。终态收敛信号来自 agent 调用 MCP `mark_done`（DD-101），或 driver 终局 `Finished` 事件（FR-116），不走 QA 的 `qa_ran`/`qa_exit_code` finalize 规则。

## Apply-time Gating（复用 FR-116 capability 校验）

- `kind: task` + `checkpoint_strategy: git_tag`（或 `git_stash`）→ 拒绝。
- `self_referential: true` + `kind: task` → 拒绝。
- `kind: task` + 非空 `qa_targets`/`ticket_dir` → 拒绝（语义不适用）。
- `kind: code_repo` 且既无 `work_dir` 也无 `root_path` → 拒绝。
- workspace/profile 声明的任一路径 ⊄ `shareableRoots` → 拒绝（fail-closed，见安全不变量）。

## Security Invariants（合意条款，必须落入 security 测试文档）

去掉项目/git 边界后，**沙箱 + 全局天花板成为唯一约束**。以下两条为本 FR 的合意安全条款，须分别落入独立 security 测试文档并纳入验收：

### SI-1 天花板是子集约束，不是并集（`docs/security/authorization/02-file-sharing-ceiling.md`）

- workspace/profile 声明的每一条 readable/writable 路径，必须是 `shareableRoots` 中某条的子路径。
- 越界一律在 apply 时**拒绝**，绝不在运行时「尽力而为」或静默裁剪。
- 符号链接/`..`/绝对路径逃逸必须在规范化后再判定子集关系。
- 未配置 FileSharing 时，`kind: task` workspace 无任何可共享宿主路径（默认拒绝，而非默认放行）。

### SI-2 HOME 重定向必须真隔离（`docs/security/file-security/02-workspace-home-isolation.md`）

- `work_dir` 兼作 `~/` 时，沙箱内 `HOME`/`XDG_CONFIG_HOME` 等必须被强制重写为 `work_dir`，不得暴露真实用户 home 路径。
- 现有 spawn 的 `env_clear()` + allowlist（`runner/spawn.rs`）是执行点，须显式验证 `HOME`/`XDG_*` 不携带真实值。
- agent 不得通过 `$HOME` 展开、`~` 展开或环境继承逃逸到 `shareableRoots` 之外。
- 临时目录路径的清理：task 结束后分配的临时 HOME 须按 artifacts 生命周期回收，不残留敏感中间物。

## Risks And Mitigations

- 风险：路径子集判定在 symlink/挂载点上被绕过。
  - 缓解：规范化（realpath）后判定；拒绝跨越 `shareableRoots` 的 symlink；security 场景覆盖。
- 风险：`root_path` → `work_dir` 更名破坏既有 manifest。
  - 缓解：`root_path` 保留为 `work_dir` 的 deserialize 别名；round-trip 测试覆盖。
- 风险：非代码 item 无 QA finalize，收敛依赖 agent 自判，可能不终止。
  - 缓解：沿用 `max_cycles`、退化循环检测、budget cap；`mark_done` 缺失时按超时/终局事件收口。
- 风险：临时 HOME 分配引入磁盘泄漏或跨 task 串号。
  - 缓解：per-task 唯一目录（对齐 FR-116 的 per-run MCP config 修复）、0700 权限、artifacts 生命周期回收。
- 风险：GUI/CLI 对 `kind: task` workspace 仍按 QA 视角渲染。
  - 缓解：`kind` 显式驱动展示分支；Console 隐藏 QA-only 面板。

## Acceptance Criteria

- `cargo build --workspace` / `cargo test --workspace` 通过；既有 `code_repo` workspace 与 manifest 行为零变化，`root_path` 别名 round-trip 正确。
- Workspace 支持 `kind: task` 与可选 `work_dir`；`qa_targets`/`ticket_dir` 可省略。
- 缺省 `work_dir` 时 daemon 为 task 分配唯一临时目录作 cwd/HOME，权限 0700，task 结束回收。
- daemon 级 FileSharing 配置可声明 `globalSkills` 与 `shareableRoots`；全局 skills 以只读挂入每个 task 沙箱。
- 非代码 task 以单一隐式 item 运行，终态由 `mark_done`/终局事件决定，QA finalize 规则不参与。
- apply-time 门控拒绝全部非法组合（≥5 QA 场景：git_tag×task、self_ref×task、qa_targets×task、code_repo 无路径、路径越界天花板）。
- **SI-1** 落入 `docs/security/authorization/02-file-sharing-ceiling.md`，含子集/规范化/symlink/默认拒绝场景，且有可执行验证。
- **SI-2** 落入 `docs/security/file-security/02-workspace-home-isolation.md`，含 HOME/XDG 重写、`$HOME` 逃逸、临时目录清理场景，且有可执行验证。
- 仓库管理 Slack 场景 pilot：badge 触发 → 非代码 task → agent 读消息/查库存/给建议 → Attention 交人决策，端到端可复现（`test-non-code-workspace.sh`）。
