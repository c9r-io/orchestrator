# DD-077: GUI CLI 功能对齐 — RPC 覆盖补全

## 概述

将 GUI 的 RPC 覆盖率从约 40% 提升至 82%+，新增 17 个 Tauri 命令覆盖 4 个功能域（系统运维、资源高级操作、密钥管理、事件/触发器），并为专家模式新增 4 个 Tab。

## 新增 Tauri 命令

| 模块 | 命令 | RPC | RBAC |
|------|------|-----|------|
| system | `check` | Check | read_only |
| system | `worker_status` | WorkerStatus | read_only |
| system | `db_status` | DbStatus | read_only |
| system | `shutdown` | Shutdown | admin |
| system | `maintenance_mode` | MaintenanceMode | admin |
| task | `task_trace` | TaskTrace | read_only |
| task | `task_recover` | TaskRecover | operator |
| task | `task_delete_bulk` | TaskDeleteBulk | admin |
| manifest | `manifest_validate` | ManifestValidate | operator |
| manifest | `manifest_export` | ManifestExport | read_only |
| secret | `secret_key_list` | SecretKeyList | admin |
| secret | `secret_key_status` | SecretKeyStatus | admin |
| secret | `secret_key_rotate` | SecretKeyRotate | admin |
| secret | `secret_key_revoke` | SecretKeyRevoke | admin |
| event | `event_cleanup` | EventCleanup | admin |
| event | `event_stats` | EventStats | read_only |
| trigger | `trigger_suspend/resume/fire` | TriggerSuspend/Resume/Fire | operator |

## 专家模式新增 Tab

| Tab | 功能 | 数据源 |
|-----|------|--------|
| Store | KV 浏览器 + 编辑器 | store_list/get/put/delete |
| 系统 | Worker 状态 + DB 状态 + 预检查 + 维护模式 + 关闭 | check/worker_status/db_status/shutdown/maintenance_mode |
| 触发器 | 触发器列表 + suspend/resume/fire | resource_get("triggers") + trigger_* |
| 密钥 | 密钥表 + 轮转/撤销 | secret_key_status/rotate/revoke |

## TaskDetail 增强

- **跟踪**按钮：调用 TaskTrace，显示 trace_json
- **恢复任务**按钮：调用 TaskRecover（仅 error/failed 状态）
- 专家模式 Tab 从 4 个扩展到 8 个

## 覆盖统计

总 Tauri 命令数：47（含 GUI 专用命令如 connect/probe_role）
覆盖 RPC 数：~42/51 = **82%**
