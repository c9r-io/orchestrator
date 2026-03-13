# FR-039: Trigger 资源 — Cron 与事件驱动的任务自动创建

## 优先级

P1 — 填补 orchestrator 被动触发能力的空白，是生产可用性的关键缺失原语

## 背景与问题

### 现状

orchestrator 当前的 **全部 10 个资源类型**（Project, Workspace, Agent, Workflow, StepTemplate, ExecutionProfile, RuntimePolicy, EnvStore, SecretStore, CRD）均围绕声明式配置和命令式执行展开。任务的启动只有一条路径：

```
用户手动执行 → orchestrator task create → daemon worker claim → run_task_loop
```

**不存在任何被动触发机制** — 没有时间驱动，没有事件驱动。

### 痛点

1. **定期任务需外部 crontab**：夜间回归 QA、周期性文档治理、定期安全扫描等场景，用户必须依赖系统 crontab 或外部调度器调用 `orchestrator task create`，破坏了 orchestrator 的自治管理边界
2. **任务间无法编排因果链**：一个 workflow 完成后触发下游 workflow（如 QA 通过后自动部署）需要手动轮询或外部胶水脚本
3. **Webhook 集成不可能**：GitHub push event、CI 回调等外部事件无法直接触发 workflow
4. **与 K8s 生态的心智模型断裂**：K8s 有 CronJob、Argo 有 EventSource/Sensor，orchestrator 作为类 K8s 编排器缺少这一层

### 设计原则

- **统一为单一 Trigger 资源**：Cron 和 Event 的 action 端完全相同（"创建一个 task"），仅触发条件不同，不应拆成两个 CRD
- **Trigger 不参与执行**：Trigger 只负责"什么时候创建 task"，不干预 task 内部的步骤执行
- **可观测、可暂停、可审计**：每次触发都有事件记录，支持 suspend 开关

## 提议的资源设计

### Trigger 资源 Manifest

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: nightly-qa
  project: my-project
spec:
  # ── 触发条件（二选一） ──────────────────────────────
  # 方式 A: Cron 定时触发
  cron:
    schedule: "0 2 * * *"           # 标准 5 段 cron 表达式
    timezone: "Asia/Shanghai"       # 可选，默认 UTC

  # 方式 B: 事件触发
  # event:
  #   source: task_completed         # 事件源类型
  #   filter:                        # CEL 过滤条件
  #     workflow: "full-qa"
  #     condition: "status == 'completed' && unresolved_items == 0"

  # ── 触发动作 ──────────────────────────────────────
  action:
    workflow: deploy-staging         # 目标 Workflow 名称
    workspace: main-workspace        # 目标 Workspace 名称
    args:                            # 可选: 传递给 task create 的参数
      target-file:
        - "docs/qa/"
    start: true                      # 创建后是否立即启动（默认 true）

  # ── 策略控制 ──────────────────────────────────────
  concurrencyPolicy: Forbid          # Allow | Forbid | Replace
  suspend: false                     # 暂停开关
  historyLimit:
    successful: 3                    # 保留最近 N 个成功 task
    failed: 3                        # 保留最近 N 个失败 task
  throttle:
    minInterval: 300                 # 事件触发专用：最小触发间隔（秒），防抖
```

### Spec 字段详述

#### 触发条件（互斥，必须且只能设置一个）

| 字段 | 类型 | 说明 |
|------|------|------|
| `cron.schedule` | string | 标准 5 段 cron 表达式（分 时 日 月 周） |
| `cron.timezone` | string | IANA 时区名，默认 `UTC` |
| `event.source` | enum | 事件源类型，见下方分层 |
| `event.filter` | object | CEL 条件过滤，event source 不同则上下文变量不同 |

#### 事件源分层（`event.source`）

| Source | 描述 | CEL 上下文 | 实现阶段 |
|--------|------|-----------|---------|
| `task_completed` | 任务成功完成 | `workflow`, `workspace`, `task_id`, `status`, `cycles`, `unresolved_items` | Phase 1 |
| `task_failed` | 任务执行失败 | 同上 | Phase 1 |
| `webhook` | HTTP POST 到 daemon endpoint | `headers`, `body`, `method`, `path` | Phase 2 |
| `fs_watch` | 文件系统变更通知 | `path`, `event_type` (create/modify/delete) | Phase 3 |

#### 动作

| 字段 | 类型 | 必须 | 说明 |
|------|------|------|------|
| `action.workflow` | string | 是 | 目标 Workflow 资源名称 |
| `action.workspace` | string | 是 | 目标 Workspace 资源名称 |
| `action.args` | map | 否 | 传递给 task create 的参数（target-file 等） |
| `action.start` | bool | 否 | 创建后是否自动启动，默认 `true` |

#### 策略控制

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `concurrencyPolicy` | enum | `Forbid` | `Allow`: 允许并发；`Forbid`: 有运行中 task 时跳过本次触发；`Replace`: 终止旧 task 再创建新的 |
| `suspend` | bool | `false` | 暂停触发器，不删除资源 |
| `historyLimit.successful` | int | 3 | 自动清理旧的成功 task（0 = 不保留） |
| `historyLimit.failed` | int | 3 | 自动清理旧的失败 task（0 = 不保留） |
| `throttle.minInterval` | int | 0 | 事件触发专用：两次触发间最小间隔秒数 |

### 数据库 Schema 扩展

```sql
-- 触发器状态跟踪
CREATE TABLE trigger_state (
    trigger_name    TEXT NOT NULL,
    project         TEXT NOT NULL,
    -- Cron 状态
    last_fired_at   TEXT,              -- ISO8601 上次触发时间
    next_fire_at    TEXT,              -- ISO8601 下次触发时间（cron 专用）
    -- 统计
    fire_count      INTEGER DEFAULT 0, -- 累计触发次数
    last_task_id    TEXT,              -- 最近创建的 task ID
    last_status     TEXT,              -- skip_concurrent | created | failed_to_create
    -- 元数据
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (trigger_name, project)
);
```

### 事件类型扩展

新增事件 kind：

| Event Kind | 数据 | 说明 |
|------------|------|------|
| `trigger_fired` | `{trigger, source, task_id}` | 触发器成功创建 task |
| `trigger_skipped` | `{trigger, reason}` | 触发被跳过（concurrency/suspend/throttle） |
| `trigger_error` | `{trigger, error}` | 触发失败（workflow 不存在、参数错误等） |

## 架构设计

### 组件拆分

```
┌─────────────────────────────────────────────────────┐
│                   orchestratord                      │
│                                                      │
│  ┌────────────────┐   ┌──────────────────────────┐  │
│  │  TriggerEngine │   │     Worker Pool           │  │
│  │                │   │                           │  │
│  │  ┌──────────┐  │   │  ┌───────┐ ┌───────┐     │  │
│  │  │CronTicker│──┼───┼─▶│Worker │ │Worker │ ... │  │
│  │  └──────────┘  │   │  └───────┘ └───────┘     │  │
│  │  ┌──────────┐  │   └──────────────────────────┘  │
│  │  │EventBus  │──┘                                  │
│  │  │Listener  │     ┌──────────────────────────┐   │
│  │  └──────────┘     │  gRPC Service             │  │
│  │  ┌──────────┐     │  ┌────────────────────┐   │  │
│  │  │Webhook   │◀────┤  │POST /webhook/:name │   │  │
│  │  │Receiver  │     │  └────────────────────┘   │  │
│  │  └──────────┘     └──────────────────────────┘   │
│  └────────────────┘                                  │
└─────────────────────────────────────────────────────┘
```

### TriggerEngine 核心职责

1. **Cron Ticker**：daemon 启动时读取所有 `spec.cron` 的 Trigger，计算 `next_fire_at` 并注册定时任务。配置变更（apply/delete）时动态增删
2. **EventBus Listener**：订阅 `InnerState.event_sink` 的 `task_completed`/`task_failed` 事件，匹配 `spec.event` 的 filter 条件
3. **Webhook Receiver**（Phase 2）：在 daemon 的 gRPC/HTTP 端口上挂载 `/webhook/:trigger_name` endpoint
4. **触发执行**：条件满足后调用 `create_task()` (core/src/service/task.rs:16)，复用现有 task create 链路

### 关键设计决策

#### 1. TriggerEngine 生命周期绑定 daemon

TriggerEngine 作为 `orchestratord` 的一个 long-running tokio task 启动，与 daemon 同生命周期：
- daemon 启动 → 从 DB/config 加载所有 Trigger → 初始化 cron scheduler + event subscriptions
- `apply -f trigger.yaml` → 热更新：TriggerEngine 收到通知，增删对应的 cron job / event subscription
- daemon 停止 → TriggerEngine 优雅退出

#### 2. Cron 持久化与重启恢复

- `trigger_state.next_fire_at` 持久化到 SQLite
- daemon 重启后，对比 `next_fire_at` 与当前时间：
  - 若已过期：根据 `concurrencyPolicy` 决定是否补触发（Forbid 策略下跳过）
  - 若未过期：重新注册定时任务

#### 3. ConcurrencyPolicy 实现

在 `fire_trigger()` 执行前：

```rust
match trigger.spec.concurrency_policy {
    Allow => { /* 直接创建 */ }
    Forbid => {
        // 查询该 trigger 创建的 task 中是否有 running/pending 状态的
        if has_active_task(trigger_name, project) {
            emit_event("trigger_skipped", reason: "concurrent_task_active");
            return;
        }
    }
    Replace => {
        // 终止该 trigger 创建的旧 running task
        cancel_active_tasks(trigger_name, project);
        // 然后创建新 task
    }
}
```

#### 4. Event 触发的 CEL 过滤

复用现有 CEL 引擎（`core/src/prehook.rs`），扩展上下文变量：

```rust
// 事件触发的 CEL 上下文
struct TriggerEventContext {
    workflow: String,     // 源 task 的 workflow name
    workspace: String,    // 源 task 的 workspace name
    task_id: String,      // 源 task ID
    status: String,       // completed | failed
    cycles: i64,          // 执行了多少个 cycle
    unresolved_items: i64,// 剩余未解决 items
}
```

#### 5. historyLimit 自动清理

TriggerEngine 在每次 `trigger_fired` 后，异步执行：
1. 查询该 trigger 创建的所有 completed/failed task，按 `created_at` 降序
2. 超出 `historyLimit` 的旧 task 调用 `delete_task()`
3. 这是 best-effort 操作，失败只记日志不影响触发

## 实现路线图

### Phase 1: Cron + TaskEvent（核心）

**涉及的代码变更：**

| 文件/模块 | 变更 |
|-----------|------|
| `core/src/cli_types.rs:117` | ResourceKind 枚举追加 `Trigger` |
| `core/src/cli_types.rs` (新增) | `TriggerSpec`, `TriggerCronSpec`, `TriggerEventSpec`, `TriggerActionSpec`, `ConcurrencyPolicy` 类型定义 |
| `core/src/resource/trigger.rs` (新增) | Trigger 资源的 parse/validate/apply 实现 |
| `core/src/resource/registry.rs` | 注册 Trigger 资源 |
| `core/src/resource/parse.rs` | 添加 Trigger 的解析分支 |
| `core/src/trigger_engine.rs` (新增) | TriggerEngine 核心：cron ticker + event listener |
| `core/src/state.rs:60` | InnerState 追加 `trigger_engine: Arc<TriggerEngine>` |
| `core/src/db.rs` | 追加 `trigger_state` 表 migration |
| `core/src/events.rs` | 追加 `trigger_fired`, `trigger_skipped`, `trigger_error` 事件类型 |
| `crates/daemon/src/` | daemon 启动时初始化 TriggerEngine |
| `crates/cli/src/cli.rs` | 添加 `trigger list`, `trigger suspend`, `trigger resume` 子命令 |

**Cron 依赖**：`cron` crate（纯 Rust cron 表达式解析）+ `chrono-tz`（时区支持）

**预计复杂度**：中等。核心只需在 daemon 中增加一个 tokio task 做 cron 调度 + event 订阅，触发端完全复用 `create_task()`。

### Phase 2: Webhook 事件源

**新增变更：**

| 文件/模块 | 变更 |
|-----------|------|
| `core/src/trigger_engine.rs` | 追加 WebhookReceiver |
| `crates/daemon/src/` | 在 daemon 的 HTTP/gRPC 服务上挂载 webhook endpoint |
| `core/src/cli_types.rs` | TriggerEventSpec 追加 webhook source |

**设计要点**：
- Webhook endpoint: `POST /api/v1/webhook/:trigger_name`
- 请求 body 作为 CEL 上下文的 `body` 变量
- 可选 HMAC 签名验证（`spec.event.webhook.secret` 引用 SecretStore）

### Phase 3: 文件系统监听（可选）

**新增变更：**

| 文件/模块 | 变更 |
|-----------|------|
| `core/src/trigger_engine.rs` | 追加 FsWatcher（基于 `notify` crate） |
| `core/src/cli_types.rs` | TriggerEventSpec 追加 fs_watch source |

**设计要点**：
- 监听路径从 `spec.event.filter.paths[]` 获取
- 支持 glob 过滤（`**/*.rs`）
- 内建 debounce（文件系统事件通常成簇到达）

## CLI 交互

### 新增命令

```bash
# 查看所有 trigger
orchestrator get trigger [-p my-project]

# 查看 trigger 详情（含下次触发时间、最近触发历史）
orchestrator describe trigger nightly-qa

# 暂停/恢复 trigger
orchestrator trigger suspend nightly-qa
orchestrator trigger resume nightly-qa

# 手动触发一次（调试用）
orchestrator trigger fire nightly-qa

# 标准 CRUD
orchestrator apply -f trigger.yaml
orchestrator delete trigger nightly-qa
```

### 输出示例

```
$ orchestrator get trigger
NAME          SOURCE   SCHEDULE      LAST FIRED           NEXT FIRE            STATUS
nightly-qa    cron     0 2 * * *     2026-03-14 02:00:08  2026-03-15 02:00:00  Active
on-qa-done    event    task_completed  2026-03-14 02:35:12  -                    Active
deploy-gate   webhook  -             2026-03-13 18:22:00  -                    Suspended
```

## 验证标准

1. **Cron 基本功能**：apply 一个 `cron.schedule: "* * * * *"` 的 Trigger，验证每分钟自动创建 task
2. **ConcurrencyPolicy Forbid**：task 运行期间到达触发时间，验证不会创建新 task 且有 `trigger_skipped` 事件
3. **ConcurrencyPolicy Replace**：task 运行期间到达触发时间，验证旧 task 被 cancel 且新 task 被创建
4. **Daemon 重启恢复**：daemon 重启后 cron trigger 继续按预期触发
5. **Event 触发**：Workflow A 完成后，配置了 `event.source: task_completed` + `filter.workflow: A` 的 Trigger 自动创建 Workflow B 的 task
6. **Suspend/Resume**：`trigger suspend` 后不再触发，`trigger resume` 后恢复
7. **historyLimit**：触发次数超过 limit 后旧 task 被自动清理
8. **Throttle 防抖**：事件短时间内密集到达，验证 `minInterval` 内只触发一次
9. **Preflight Check**：`orchestrator check` 覆盖 Trigger 的引用完整性（workflow/workspace 存在性）
10. **事件审计**：所有触发/跳过/错误都有对应事件记录

## 用例示例

### 用例 1: 夜间 QA 回归

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: nightly-qa
  project: my-project
spec:
  cron:
    schedule: "0 2 * * *"
    timezone: "Asia/Shanghai"
  action:
    workflow: full-qa
    workspace: main-workspace
  concurrencyPolicy: Forbid
```

### 用例 2: QA 通过后自动部署

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: auto-deploy-on-qa-pass
  project: my-project
spec:
  event:
    source: task_completed
    filter:
      workflow: "full-qa"
      condition: "status == 'completed' && unresolved_items == 0"
  action:
    workflow: deploy-staging
    workspace: main-workspace
  concurrencyPolicy: Replace
```

### 用例 3: GitHub Webhook 触发构建（Phase 2）

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: github-push
  project: my-project
spec:
  event:
    source: webhook
    filter:
      condition: "body.ref == 'refs/heads/main'"
    webhook:
      secret: github-webhook-secret   # 引用 SecretStore
  action:
    workflow: build-and-test
    workspace: main-workspace
  throttle:
    minInterval: 60
```

## 相关

- `core/src/cli_types.rs:117` — ResourceKind 枚举（需扩展）
- `core/src/service/task.rs:16` — `create_task()` 入口（Trigger 复用此链路）
- `core/src/state.rs:60` — InnerState（需追加 TriggerEngine）
- `core/src/events.rs` — 事件系统（需追加 trigger 事件类型）
- `core/src/prehook.rs` — CEL 引擎（event filter 复用）
- FR-027: Worker Notify 唤醒机制（TriggerEngine 创建 task 后同样需要 notify worker）
- FR-005: Daemon 生命周期治理（TriggerEngine 作为 daemon 子组件需纳入生命周期管理）
