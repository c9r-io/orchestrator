# DD-076: GUI 实时状态推送与许愿池数据隔离

## 概述

解决 GUI 三个 P0 级体验问题：进度页手动刷新、许愿池数据混杂、FR 草稿渲染粗糙。本文档是 FR-066 的设计决策记录。

## 设计决策

### 1. TaskWatch 实时推送

**问题**：ProgressList 和 TaskDetail 依赖手动点击「刷新」，无法实时反映任务进度。

**方案**：复用 FR-063 已实现的 `start_task_watch` / `stop_task_watch` Tauri 命令，将 TaskWatch streaming 接入前端组件。

**ProgressList 实现**：
- 页面挂载时，对所有 `running`/`paused` 任务调用 `start_task_watch(task_id, interval_secs=3)`
- 通过 `listen("task-watch-{task_id}", ...)` 接收 `WatchSnapshot` 事件
- 每个 snapshot 包含完整 `TaskSummary` + `items`，直接替换对应卡片数据
- 页面卸载时调用 `stop_task_watch` 清理所有订阅
- 使用 `useRef` 跟踪已订阅任务，避免重复订阅

**TaskDetail 实现**：
- 进入详情页时启动 `start_task_watch`
- 维护 `liveData` state，与初始 `data` 合并为 `displayData`
- TaskWatch 和 TaskFollow（日志流）并行运行，互不干扰
- 退出详情页时自动清理 watch 订阅

### 2. 许愿池数据隔离

**问题**：WishPool 调用 `task_list({})` 获取全量任务，许愿和开发任务混在一起。

**方案**：利用 `project_id` 字段隔离。

- 许愿创建时设置 `project_id = "wish-pool"`
- WishPool 页调用 `task_list({ project_filter: "wish-pool" })` 仅显示许愿任务
- ProgressList 页不加过滤，显示所有任务（包括确认后的开发任务）
- `TaskSummary` 和 `TaskDetail` 结构体增加 `project_id`、`workflow_id`、`goal` 字段

**数据流**：
```
WishPool → task_create(project_id="wish-pool") → 许愿任务
WishDetail → 确认开发 → task_create(无 project_id) → 开发任务
ProgressList → task_list({}) → 全量任务
```

### 3. FR 草稿渲染

**问题**：WishDetail 拼接 TaskFollow 日志行作为草稿，不是实际 FR 内容。

**方案**：

- 新增 `task_logs` Tauri 命令：调用 `TaskLogs` streaming RPC，收集所有 `TaskLogChunk` 后一次性返回
- 任务完成后，调用 `task_logs(task_id)` 获取完整输出
- 提取 `content` 字段拼接为 FR 草稿文本
- 任务进行中时，显示分阶段等待提示（0-3s / 3-8s / 8s+），TaskFollow 日志到达后切换为实时显示

## 关键文件

| 文件 | 变更 |
|------|------|
| `crates/gui/src/commands/task.rs` | TaskSummary/TaskDetail 增加 project_id/workflow_id/goal |
| `crates/gui/src/commands/stream.rs` | 新增 task_logs 命令 + WatchSnapshot 字段更新 |
| `gui/src/pages/ProgressList.tsx` | TaskWatch 订阅管理 |
| `gui/src/pages/TaskDetail.tsx` | TaskWatch 实时数据合并 |
| `gui/src/pages/WishPool.tsx` | project_filter 过滤 + project_id 传参 |
| `gui/src/pages/WishDetail.tsx` | task_logs 获取草稿 + 分阶段进度提示 |
