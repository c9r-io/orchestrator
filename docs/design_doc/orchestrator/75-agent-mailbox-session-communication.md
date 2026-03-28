# Design Doc 75: Agent 间通信接口草案 — Mailbox + Session Control Plane

> 来源：FR-065 | 状态：接口草案（实施前须再次与用户对齐）

## 1. 背景

orchestrator 当前运行时已具备：

- daemon 持有全局状态，agent 作为 shell 子进程执行
- `task` / `task_item` / `command_run` / `events` 形成完整执行审计链
- step 启动时记录 `pid`，可通过运行态映射查询当前任务上下文
- TTY 路径已生成 `session_id`，持久化 `agent_sessions`、`input_fifo`、`transcript.log`、`output.json`
- 协作层已有 shared context、DAG、artifact 等类型（`orchestrator-collab` crate）；早期的 `MessageBus` 抽象因从未被消费端使用已被移除

**缺口**：agent 可被 orchestrator 调度，但缺少正式、可观测、可审计的 agent 间交互控制面。

## 2. 两层抽象

| 层 | 目的 | 推荐语义 |
|----|------|----------|
| **Mailbox** | 异步请求、通知、回执、可审计消息传递 | store-and-forward |
| **Session** | 实时输入、流式输出、attach/detach、多人旁观 | interactive stream |

- "系统邮件"适合作为 Mailbox
- "实时通话"通过显式 Session API 实现
- `pid` 不作主标识，仅为运行态观测辅助

## 3. 标识模型

| 标识 | 用途 | 稳定性 |
|------|------|--------|
| `session_id` | 活会话主标识；attach/read/write/close 核心入口 | 高 |
| `run_id` | 单次执行实例标识；审计、回放、结果关联 | 高 |
| `task_id` | 顶层任务聚合 | 高 |
| `task_item_id` | item 级执行上下文 | 高 |
| `step_id` | 步骤级上下文 | 高 |
| `agent_id` | agent 逻辑身份 | 中 |
| `pid` | 运行态观测、诊断、kill/attach 辅助 | 低 |

**约束**：

- 禁止以 `pid` 作为持久化通信对象主键
- 允许通过 `pid` 反查 `session_id/task_id/task_item_id/step_id/agent_id`
- CLI 和 gRPC 查询结果可返回 `pid`，但所有写操作应优先接受 `session_id`

## 4. Mailbox 抽象

### 4.1 功能定位

异步协作，不承担实时终端交互。典型场景：

- agent A 向 agent B 发起请求
- agent B 回复阶段结果或错误
- 调度者查看某 agent 的收件箱/发件箱/未读消息
- 审计系统回放通信历史

### 4.2 建议消息模型

```yaml
apiVersion: orchestrator.dev/v2
kind: AgentMessage
metadata:
  name: msg-20260321-001
spec:
  sender:
    agent_id: planner
    task_id: task-123
    task_item_id: item-01
    step_id: plan
    session_id: session-aaa
  receiver:
    agent_id: coder
    task_id: task-123
    task_item_id: item-01
  message_type: request          # request | response | ack | publish | control
  subject: "请实现登录 API"
  reply_to: null
  correlation_id: corr-123
  ttl_secs: 300
  require_ack: true
  payload:
    kind: execution_request
    data:
      summary: "实现 auth/login"
      constraints:
        - "不得修改 proto"
        - "需保留现有 handler 接口"
```

### 4.3 消息状态

生命周期：`queued -> delivered -> acknowledged -> responded -> expired`

失败态：`delivery_failed`, `receiver_unavailable`, `discarded`

### 4.4 AgentMailbox CRD 草案

```yaml
apiVersion: orchestrator.dev/v2
kind: AgentMailbox
metadata:
  name: coder
spec:
  agent_id: coder
  retention_days: 7
  max_pending_messages: 1000
  delivery_mode: at_least_once    # fire_and_forget | at_least_once
status:
  unread_count: 3
  in_flight_count: 1
  last_message_at: "2026-03-21T12:03:00Z"
```

## 5. Session 抽象

### 5.1 功能定位

shell 交互的正式控制面：

- attach 到正在运行的 agent 会话
- 向会话写入输入
- 从会话读取 transcript 或结构化输出
- 管理 writer / reader lease
- 查询 agent 当前是否在线、正在执行什么任务

### 5.2 与现有运行时的关系

现有基础（建议在此基础上补齐，不另起炉灶）：

- TTY step 创建 `session_id`
- 会话有 `input_fifo_path`
- 有 `transcript_path` 和 `output_json_path`
- 有 writer / reader attachment 数据模型（`session_attachments` 表）

### 5.3 Session 状态

`opening -> active -> detached -> draining -> closed -> failed`

- `active`: 至少一个 writer 或 shell 进程仍在线
- `detached`: agent 进程仍运行，但无交互方附着
- `draining`: 收到关闭请求，拒绝新 writer
- `closed`: 正常结束
- `failed`: 非正常退出、FIFO/PTY 错误、进程异常终止

### 5.4 AgentSession CRD 草案

```yaml
apiVersion: orchestrator.dev/v2
kind: AgentSession
metadata:
  name: session-aaa
spec:
  agent_id: coder
  task_id: task-123
  task_item_id: item-01
  step_id: implement
  mode: tty                       # tty | noninteractive
  access:
    max_readers: 8
    max_writers: 1
    allow_human_attach: true
    allow_agent_attach: true
  retention:
    transcript_ttl_hours: 168
    output_ttl_hours: 168
status:
  session_id: session-aaa
  state: active
  pid: 12345
  run_id: run-xyz
  attached_readers: 2
  attached_writer: "client/operator-1"
  created_at: "2026-03-21T12:00:00Z"
  updated_at: "2026-03-21T12:03:00Z"
```

## 6. 建议 CLI 形状

### 6.1 Mailbox CLI

```bash
orchestrator agent mailbox list [--agent <agent_id>] [--task <task_id>] [--state unread]
orchestrator agent mailbox send \
  --from-agent planner --to-agent coder \
  --task task-123 --item item-01 \
  --subject "实现登录 API" --payload-file payload.json
orchestrator agent mailbox ack <message_id>
orchestrator agent mailbox reply <message_id> --payload-file reply.json
orchestrator agent mailbox wait --agent coder --timeout 30
orchestrator agent mailbox get <message_id> -o json
```

查询过滤支持：`--agent`, `--task`, `--item`, `--step`, `--session`, `--pid`

约束：`--pid` 仅作查询过滤；所有写操作不得只接受 `--pid`，必须能解析到具体 `session_id` 或目标 agent。

### 6.2 Session CLI

```bash
orchestrator agent session list [--task <task_id>] [--agent <agent_id>]
orchestrator agent session open --task task-123 --item item-01 --step implement
orchestrator agent session attach <session_id> [--mode reader]
orchestrator agent session send-input <session_id> --text "继续，先补测试"
orchestrator agent session read <session_id> [--follow]
orchestrator agent session detach <session_id>
orchestrator agent session close <session_id>
orchestrator agent session inspect <session_id> -o json
orchestrator agent session resolve --pid 12345
```

约束：`attach` 默认 `reader`；`writer` 需显式申请 lease；`resolve --pid` 映射 pid 到 `session_id/task_id/task_item_id/step_id/agent_id`。

## 7. 建议 gRPC API 形状

### 7.1 Mailbox RPC

```proto
rpc AgentMessageSend(AgentMessageSendRequest) returns (AgentMessageSendResponse);
rpc AgentMessageList(AgentMessageListRequest) returns (AgentMessageListResponse);
rpc AgentMessageGet(AgentMessageGetRequest) returns (AgentMessageGetResponse);
rpc AgentMessageAck(AgentMessageAckRequest) returns (AgentMessageAckResponse);
rpc AgentMessageReply(AgentMessageReplyRequest) returns (AgentMessageReplyResponse);
rpc AgentMessageWait(AgentMessageWaitRequest) returns (stream AgentMessageEnvelope);
```

### 7.2 Session RPC

```proto
rpc AgentSessionList(AgentSessionListRequest) returns (AgentSessionListResponse);
rpc AgentSessionGet(AgentSessionGetRequest) returns (AgentSessionGetResponse);
rpc AgentSessionAttach(AgentSessionAttachRequest) returns (AgentSessionAttachResponse);
rpc AgentSessionDetach(AgentSessionDetachRequest) returns (AgentSessionDetachResponse);
rpc AgentSessionSendInput(AgentSessionSendInputRequest) returns (AgentSessionSendInputResponse);
rpc AgentSessionRead(AgentSessionReadRequest) returns (stream AgentSessionOutputChunk);
rpc AgentSessionClose(AgentSessionCloseRequest) returns (AgentSessionCloseResponse);
rpc AgentSessionResolvePid(AgentSessionResolvePidRequest) returns (AgentSessionResolvePidResponse);
```

关键返回字段：

- `AgentSessionAttach`: `session_id`, `lease_mode`, `client_id`, `writer_granted`
- `AgentSessionRead`: `timestamp`, `stream` (stdout/stderr/transcript/output_json), `chunk`, `offset`, `eof`
- `AgentSessionResolvePid`: `pid`, `session_id`, `run_id`, `task_id`, `task_item_id`, `step_id`, `agent_id`, `state`

## 8. 推荐实施顺序

| 阶段 | 范围 | 说明 |
|------|------|------|
| Phase 1 | 观测与解析 | `pid -> session/task/item/step/agent` 查询；`session list/get/inspect`；不开放 agent-to-agent 写入 |
| Phase 2 | Mailbox | 持久化消息表；`send/list/get/ack/reply`；CLI 与 gRPC 基础能力 |
| Phase 3 | Session 控制 | 对 `agent_sessions` 补齐 attach/send-input/read/close；writer/reader lease 治理；transcript follow |
| Phase 4 | 高阶能力 | agent 直接 attach 到其他 agent session；权限治理、配额、限流；GUI 观测 |

## 9. 风险与缓解

| 风险 | 缓解策略 |
|------|----------|
| `pid` 复用导致错误映射 | 强制 `session_id` 为写入口主键 |
| shell 子进程树复杂 | `pid` 仅做短生命周期查询索引 |
| 会话写入并发冲突 | writer lease 只允许单 writer |
| interactive session 安全边界 | 所有 session I/O 进入审计链 |
| FIFO/PTY 平台差异 | 先支持已验证的 shell/FIFO 模式，再评估 PTY multiplexing |

## 10. 非目标

- 不要求立即支持跨主机/分布式 agent 通信
- 不要求立即支持音视频式"通话"
- 不要求在第一阶段实现 GUI


## 11. 实施前约束

**后续实现前，必须再次与用户进行需求对齐**，确认本草案的抽象、命名、优先级和范围仍然有效，避免因用户遗忘或上下文变化导致误实现。

## 12. 现有运行时基础

已存在的代码/数据结构（实施时应在此基础上扩展）：

| 组件 | 位置 | 当前状态 |
|------|------|----------|
| `agent_sessions` 表 | `core/src/persistence/migration_steps.rs` | 持久化会话记录，含 session_id/pid/fifo/transcript 路径 |
| `session_attachments` 表 | `core/src/persistence/migration_steps.rs` | reader/writer 附着记录 |
| `SessionRow` / store ops | `core/src/session_store.rs` | insert/update/query 操作 |
