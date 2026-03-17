# FR-058: QA 自引用测试覆盖率恢复 — 场景级安全分级治理

## 状态

- **优先级**: P1
- **状态**: In Progress
- **提出日期**: 2026-03-17

## 背景

Full-QA 回归测试以自引用方式运行：orchestrator 的 QA agent 在自身 daemon 上执行 QA 文档中的测试场景。
为防止 QA agent 执行到 `kill daemon`、`cargo build --release`、`orchestrator task create` 等危险操作
导致 daemon 崩溃或 restart 级联，我们将 138 个 QA 文档中的 **118 个**标记为 `self_referential_safe: false`，
由 prehook 自动跳过。

**问题**：当前仅 **20 个文档（约 14.5%）** 参与 full-QA 执行，测试覆盖率严重不足，
full-qa workflow 已失去"全量回归"的意义。

**机会**：大量被标记为不安全的文档中，只有部分场景涉及危险操作，其余场景为纯只读验证
（如检查 `cargo test` 输出、验证 sqlite3 查询结果、校验配置文件格式）。
通过场景级安全分级，可以恢复大量可测试面。

## 现状分析

### 118 个不安全文档的操作类型分布

| 操作类型 | 文档数 | 恢复难度 | 恢复策略 |
|---------|--------|---------|---------|
| 无实际危险操作（误标） | 6 | 极低 | 直接移除 `self_referential_safe: false` |
| 仅 `cargo test` | 30 | 低 | 拆分场景：`cargo test` 步骤标记不安全，验证步骤保留 |
| 仅 `orchestrator apply` | 8 | 中 | 隔离 apply 场景，保留 get/check 只读场景 |
| `apply + delete` | 5 | 中 | 同上 |
| `task create + apply + delete` | 18 | 高 | 需逐场景分析，提取纯验证场景 |
| `cargo build + kill + ...` | 15 | 极高 | 仅能恢复不涉及进程操作的场景（如配置验证） |
| 其他组合 | 36 | 中-高 | 逐文档分析 |

### 恢复潜力估算

**第一批（直接恢复）— 6 个文档，29 个场景**：

无实际危险操作的文档，可直接移除安全标记：

- `108-incremental-item-progress.md` (6 scenarios)
- `109-parallel-spawn-stagger-delay.md` (5 scenarios)
- `47-task-spawning.md` (5 scenarios)
- `48-dynamic-items-selection.md` (5 scenarios)
- `91-daemon-crash-resilience.md` (5 scenarios)
- `95-prehook-self-referential-safe-filter.md` (3 scenarios)

**第二批（场景拆分恢复）— 30 个文档，94 个场景**：

仅含 `cargo test` 的文档。每份文档的场景通常分为：
1. **前置步骤**：`cargo test --lib -- some_test` 运行单测（不安全 — 编译可能干扰）
2. **验证步骤**：检查输出、grep 代码、sqlite3 查询（安全）

可通过场景级 `self_referential_safe` 标注或文档拆分来恢复验证步骤。

**总计可恢复**：约 123 个场景（占当前被跳过场景的显著比例）。

## 目标

1. 将 full-QA 可执行文档从 20 个恢复到 **≥56 个（≥40%）**
2. 建立场景级安全分级机制，支持同一文档中部分场景执行、部分跳过
3. 小步迭代，每批治理 5-10 个文档，逐步扩大测试面

## 实现方案

### Phase 1: 直接恢复误标文档 ✅ 完成

审计 6 个候选文档，确认 4 个安全、2 个需保留标记：

- ✅ `108-incremental-item-progress.md` — 纯只读 task info 查询，已恢复
- ✅ `109-parallel-spawn-stagger-delay.md` — 配置观察，无写操作，已恢复
- ✅ `91-daemon-crash-resilience.md` — 纯代码审查，不执行危险操作，已恢复
- ✅ `95-prehook-self-referential-safe-filter.md` — 过滤行为验证，已恢复
- ❌ `47-task-spawning.md` — spawn_task 创建子任务，保留标记
- ❌ `48-dynamic-items-selection.md` — item 删除/替换写操作，保留标记

**结果**：可执行文档数从 20 → 24

### Phase 2: 场景级安全标注机制

设计并实现场景级 `self_referential_safe` 支持：

**方案 A — frontmatter 场景白名单**（推荐）：
```yaml
---
self_referential_safe: false
self_referential_safe_scenarios: [S2, S3, S5]  # 仅这些场景在自引用执行时运行
---
```

**方案 B — 场景内联标注**：
```markdown
### 场景 1: 编译验证
<!-- self_referential_safe: false -->
```

**方案 C — 文档拆分**：
将安全场景提取到 `*-safe.md` 副本中。

方案 A 改动最小，只需修改 prehook 评估逻辑和 QA agent 的场景调度。

### Phase 3: 逐批治理仅 `cargo test` 文档（30 个文档，分 3-4 批）

对每批 8-10 个文档：
1. 逐场景分析：标注哪些场景仅含只读验证
2. 添加 `self_referential_safe_scenarios` 白名单
3. 在 full-qa 中验证恢复的场景正常执行
4. 更新 `full-qa-execution.md` 计数

### Phase 4: 治理 `orchestrator apply/delete` 文档（13 个文档）

分析 `apply`/`delete` 场景中哪些使用了项目隔离（`--project` 参数），
隔离良好的场景可标记为安全。

### Phase 5: 评估 `task create` 文档（18 个文档）

最复杂的一批。需要逐场景确认是否会创建实际竞争任务，
或仅在隔离项目中操作。

## 迭代节奏

- 每批治理 5-10 个文档
- 每批完成后运行一次 full-qa 验证
- 预计 5-8 个迭代完成全部治理
- 每个迭代通过 `qa-doc-governance` skill 进行文档一致性检查

## 成功判定

1. full-QA 可执行文档 ≥ 56 个（覆盖率 ≥ 40%）
2. 恢复的场景不引发 daemon restart 或进程干扰
3. 建立可持续的场景级安全分级机制，新增 QA 文档时自然适用
4. `full-qa-execution.md` 中的计数与实际一致

## 深层目标：不安全场景 = 系统能力缺口

完成前 5 个 Phase 的场景拆分恢复后，剩余的**真正不安全场景**需要从用户视角重新审视。

一个 QA 场景需要 `kill daemon` 或 `cargo build --release` 才能验证，本质上说明
**系统缺乏对应的安全 API 或可观测性接口**。典型例子：

| 当前不安全操作 | 用户真正需要的能力 | 缺口类型 |
|--------------|-------------------|---------|
| `kill -9 $DAEMON_PID` 测试崩溃恢复 | 故障注入 API（`orchestrator daemon inject-fault crash`） | 功能缺失 |
| `cargo build --release` 测试二进制更新 | 版本/构建状态查询接口 | 可观测性缺口 |
| 手动 `kill` + 检查 PID 文件 | 进程健康检查 API（`orchestrator daemon health`） | 功能缺失 |
| `cargo test` 验证内部行为 | 内建自检能力（`orchestrator self-check`） | 架构缺口 |
| 直接操作 sqlite3 验证状态 | 结构化状态查询 API | 可观测性缺口 |

**长期方向**：每个不安全场景都应转化为一个功能需求 —— 要么提供安全的等价 API，
要么将验证逻辑内化为系统自检能力。当所有场景都能通过安全 API 完成时，
`self_referential_safe: false` 将自然归零，full-QA 将真正实现全量覆盖。

这个方向可在 Phase 1-5 完成后，作为后续 FR 逐步推进。

## 风险

- 场景级标注增加 frontmatter 复杂度，需要工具支持校验
- 错误标注安全场景可能导致 daemon 被杀 — 需严格审查
- `cargo test` 场景中的编译步骤可能产生二进制文件覆盖 — 需评估 sandbox 是否足够隔离

## 相关资源

- 当前不安全文档标记：commit `a489ede` + `6de7edd`
- Prehook 安全过滤机制：`docs/design_doc/orchestrator/52-prehook-self-referential-safe-filter.md`
- Full-QA 执行计划：`docs/showcases/full-qa-execution.md`
- 全局安全检查实现：commit `8de6621`
