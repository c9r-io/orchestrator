# QA-118: GUI 实时状态推送与许愿池数据隔离

**关联设计**: `docs/design_doc/orchestrator/76-gui-realtime-wish-isolation.md`
**关联 FR**: FR-066（已闭环）

## 前置条件

> **重要**: 许愿池功能依赖 `wish-pool` 项目。GUI 中 `WishPool.tsx` 硬编码了
> `project_id: "wish-pool"`，若该项目不存在，`task create` 会报错
> `project not found: wish-pool`。
>
> 在执行 S3/S4 前，需要先初始化该项目：
> ```bash
> orchestrator init
> # 使用 full-qa workflow 为 wish-pool 项目创建 workspace + agent + workflow
> orchestrator apply --project wish-pool -f docs/workflow/full-qa.yaml
> # 同时加载所需的 secret 和 execution profile
> orchestrator apply --project wish-pool -f docs/workflow/claude-secret.yaml
> orchestrator apply --project wish-pool -f docs/workflow/execution-profiles.yaml
> ```
> 验证项目已创建：`orchestrator task list --project wish-pool`（应返回空列表而非报错）。

## 验证场景

## Scenario 0: 入口可见性

**入口**: 启动 GUI 后，侧边栏「进度观察」Tab

| 步骤 | 操作 | 预期结果 |
|------|------|---------|
| 1 | 启动 GUI | 默认进入「进度观察」Tab |
| 2 | 确认侧边栏有「进度观察」Tab | Tab 可见且可点击 |
| 3 | 确认活跃 running 任务右上角有「● 实时」标记 | 实时状态指示器可见 |

**预期**: 进度观察 Tab 可见，实时状态指示器正常显示。

---

### 场景 1: ProgressList 实时更新

**目标**: running 任务的状态/进度自动更新，无需手动刷新。

**步骤**:
1. 有至少一个 running 任务
2. 打开「进度观察」Tab
3. 观察任务卡片的进度条和状态是否自动更新
4. 确认活跃任务卡片右上角显示「● 实时」标记

**期望**: 进度每 3 秒自动更新，延迟 < 2s。

---

### 场景 2: TaskDetail 实时更新

**目标**: 任务详情页的步骤状态和进度实时更新。

**步骤**:
1. 进入一个 running 任务的详情页
2. 观察步骤列表中各 item 的状态图标是否实时变化
3. 观察进度条是否自动递增
4. 确认日志流（TaskFollow）与状态更新（TaskWatch）并行运行

**期望**: 步骤完成时 item 状态实时从 running → completed。

---

### 场景 3: 许愿池数据隔离

**目标**: 许愿池仅显示 project_id="wish-pool" 的任务。

**步骤**:
1. 通过许愿池创建一个许愿
2. 确认许愿任务出现在「许愿池」Tab
3. 切换到「进度观察」Tab，确认全量任务列表显示
4. 通过 CLI 创建一个普通任务（无 project_id）
5. 确认普通任务不出现在「许愿池」Tab

**期望**: 两个 Tab 数据完全隔离。

---

### 场景 4: 许愿创建传递 project_id

**步骤**:
1. 在许愿池输入需求并提交
2. 通过 CLI `orchestrator task list` 查看新任务
3. 确认任务的 project_id 为 "wish-pool"

**期望**: 许愿创建时自动携带 project_id。

---

### 场景 5: FR 草稿完整渲染

**目标**: WishDetail 显示完整 FR 草稿内容。

**步骤**:
1. 创建一个许愿，等待草稿完成（status=completed）
2. 点击进入许愿详情
3. 确认显示的是 TaskLogs 返回的完整内容（非拼接的日志行）

**期望**: FR 草稿以完整文本展示，格式清晰。

---

### 场景 6: 分阶段等待提示

**目标**: 许愿提交后显示分阶段进度文字。

**步骤**:
1. 提交一个许愿
2. 进入许愿详情页
3. 观察进度文字变化：
   - 0-3s: "正在理解你的需求..."
   - 3-8s: "正在设计功能方案..."
   - 8s+: "正在撰写 FR 文档..."
4. 日志到达后自动切换为实时日志显示

**期望**: 进度提示平滑过渡，用户始终知道系统在做什么。

---

### 场景 7: TaskWatch 资源清理

**步骤**:
1. 打开进度观察页（启动多个 TaskWatch）
2. 切换到许愿池 Tab
3. 再切回进度观察 Tab
4. 确认无重复订阅（通过日志或网络观察）

**期望**: 页面切换时正确清理和重建 watch 订阅。

---

## Checklist

- [ ] S1: ProgressList running 任务自动更新（无需手动刷新）— **GUI REQUIRED**
- [ ] S2: TaskDetail 步骤状态和进度实时刷新 — **GUI REQUIRED**
- [x] S3: 许愿池仅显示 wish-pool 项目任务 — **PARTIAL** (CLI验证通过 project_filter 逻辑，但 "wish-pool" 项目未配置)
- [ ] S4: 许愿创建时 project_id 正确传递 — **FAILED** (项目 "wish-pool" 不存在，task_create 报错)
- [ ] S5: 完成状态许愿显示 TaskLogs 完整草稿 — **GUI REQUIRED**
- [ ] S6: 草稿中状态显示分阶段等待提示 — **GUI REQUIRED**
- [ ] S7: 页面切换时 watch 订阅正确清理 — **GUI REQUIRED**

## 测试结果摘要 (2026-03-21)

### CLI 可验证项

**S3 — PARTIAL PASS**
- `orchestrator task list --project {project_id}` 过滤功能正常工作
- 验证：使用 `self-bootstrap` 项目创建任务，project filter 正确返回该项目的任务
- 问题：`wish-pool` 项目未在配置中定义，导致基于该项目的过滤/创建都会失败

**S4 — FAILED**
- 错误：`task.create: project not found: wish-pool`
- 根因：`crates/orchestrator-scheduler/src/task_ops.rs:117-124` 要求 project_id 必须在 `active.config.projects` 中存在
- 影响：GUI WishPool 创建许愿时会失败（`WishPool.tsx:68` 传递 `project_id: "wish-pool"`）

### GUI 专用项 (需要 Tauri GUI 运行)

S1, S2, S5, S6, S7 均需要：
1. Tauri GUI 构建并运行
2. 可通过 `start_task_watch` / `stop_task_watch` RPC 命令验证实时功能

### 关联 Ticket

- `docs/ticket/qa118_gui_wishpool_projectid_20260321_180500.md` — S3/S4 项目隔离问题
