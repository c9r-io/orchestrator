# FR-067: GUI CLI 功能对齐 — 补全缺失 RPC 覆盖

**状态**: Proposed

## 背景

orchestrator CLI 拥有 51+ RPC 方法的完整覆盖，但 GUI 当前仅封装了约 20 个核心命令。专家模式承诺"覆盖 80% 以上 CLI 常用操作"，但以下常用功能尚未在 GUI 中可用。

## 目标

补全 GUI 的 Tauri 命令封装和专家模式 UI，使 CLI 的常用操作在 GUI 中均可执行。

## 缺失 RPC 清单

### 第一批：任务管理补全

| RPC | CLI 命令 | 用途 | RBAC |
|-----|---------|------|------|
| `TaskLogs` | `task logs` | 查看已完成任务的历史日志 | read_only |
| `TaskTrace` | `task trace` | 查看任务执行跟踪链 | read_only |
| `TaskRecover` | `task recover` | 从错误状态恢复任务 | operator |
| `TaskDeleteBulk` | `task delete --bulk` | 批量删除任务 | admin |

### 第二批：系统运维

| RPC | CLI 命令 | 用途 | RBAC |
|-----|---------|------|------|
| `Check` | `check` | 运行预检查（daemon 健康/配置完整性） | read_only |
| `WorkerStatus` | `debug worker-status` | 查看 worker 线程状态（active/idle） | read_only |
| `DbStatus` | `db status` | 查看数据库状态 | read_only |
| `Shutdown` | `daemon stop` | 优雅关闭 daemon | admin |
| `MaintenanceMode` | `daemon maintenance` | 进入/退出维护模式 | admin |

### 第三批：资源高级操作

| RPC | CLI 命令 | 用途 | RBAC |
|-----|---------|------|------|
| `ManifestValidate` | `manifest validate` | 验证 YAML 清单语法 | operator |
| `ManifestExport` | `manifest export` | 导出全量资源清单 | read_only |
| `SecretKeyList` | `secret list` | 列出加密密钥 | admin |
| `SecretKeyRotate` | `secret rotate` | 轮转密钥 | admin |

### 第四批：事件与触发器

| RPC | CLI 命令 | 用途 | RBAC |
|-----|---------|------|------|
| `EventCleanup` | `event cleanup` | 清理过期事件 | admin |
| `EventStats` | `event stats` | 事件统计 | read_only |
| `TriggerSuspend/Resume/Fire` | `trigger suspend/resume/fire` | 触发器管理 | operator |

## 设计方案

### 后端

每个 RPC 添加对应的 `#[tauri::command]` 函数，模式与现有命令一致。

### 前端

**专家模式新增 Tab**：

- **Store Tab**：已有后端命令（`store_list/get/put/delete`），新增 UI 面板（KV 浏览器 + 编辑器）
- **System Tab**：Check / WorkerStatus / DbStatus / Shutdown / MaintenanceMode
- **Trigger Tab**：触发器列表 + suspend/resume/fire 操作
- **Secret Tab**：密钥列表 + 轮转操作（admin only）

**TaskDetail 增强**：
- 「历史日志」按钮：调用 `TaskLogs` 查看已完成任务的全量日志
- 「执行跟踪」按钮：调用 `TaskTrace` 查看执行链
- 「恢复」按钮：TaskRecover（仅 error 状态显示）
- 批量选择 + 批量删除

## 成功标准

1. 专家模式覆盖 ≥ 80% CLI 常用操作（当前约 40%，目标补全至 85%+）
2. 所有新增命令的 RBAC 权限控制正确
3. 每批 RPC 独立可交付，不阻塞其他批次

## 约束

1. 不修改 proto 定义或 daemon 代码
2. 分批实现，每批可独立编译和验证
3. 每批新增命令需在 `crates/gui/src/lib.rs` 中注册
