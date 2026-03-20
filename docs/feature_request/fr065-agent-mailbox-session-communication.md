# FR-065: Agent 间通信接口草案 — Mailbox + Session Control Plane

**状态**: Proposed

## 背景

orchestrator 当前已经具备以下运行时基础：

- daemon 持有全局状态，agent 作为 shell 子进程执行
- `task` / `task_item` / `command_run` / `events` 已形成完整执行审计链
- step 启动时会记录 `pid`，可通过运行态映射查询当前任务上下文
- TTY 路径已经生成 `session_id`，并持久化 `agent_sessions`、`input_fifo`、`transcript.log`、`output.json`
- 协作层已有 `collab::AgentMessage` 与 `MessageBus` 抽象，但目前主要用于进程内 publish，不是对外可治理的持久化消息接口

当前缺口在于：agent 可以被 orchestrator 调度，但缺少一个正式、可观测、可审计的“agent 与 agent 之间交互”的控制面。现阶段如果直接把该能力压在 shell 进程与 ad-hoc 文件/管道约定上，接口语义会不稳定，也不利于调试和治理。

本 FR 的目标不是立即实现完整交互能力，而是先沉淀一份明确的接口草案，指导后续实现。

## 目标

1. 定义 agent 间通信的两层抽象：异步 `Mailbox` 与实时 `Session`
2. 明确实体标识优先级：`session_id/run_id` 为主，`pid` 为观测辅助字段
3. 定义建议的 CRD / CLI / gRPC API 形状，便于后续实现分阶段落地
4. 明确 shell 交互模型下的约束、风险与非目标
5. 在文档中显式要求后续实施前必须再次与用户对齐，避免需求记忆漂移

## 设计方案

### 1. 总体原则

agent 间“通话”不应被简化为单一“系统邮件”能力，而应拆分为两层：

| 层 | 目的 | 推荐语义 |
|----|------|----------|
| Mailbox | 异步请求、通知、回执、可审计消息传递 | store-and-forward |
| Session | 实时输入、流式输出、attach/detach、多人旁观 | interactive stream |

结论：

- “系统邮件”适合作为 `Mailbox`
- “实时通话”应通过显式 `Session API` 实现
- `pid` 不应作为主标识，只能作为运行态观测和诊断辅助信息

### 2. 标识模型

后续实现时建议采用以下标识优先级：

| 标识 | 用途 | 稳定性 |
|------|------|--------|
| `session_id` | 活会话主标识；attach/read/write/close 的核心入口 | 高 |
| `run_id` | 单次执行实例标识；审计、回放、结果关联 | 高 |
| `task_id` | 顶层任务聚合 | 高 |
| `task_item_id` | item 级执行上下文 | 高 |
| `step_id` | 步骤级上下文 | 高 |
| `agent_id` | agent 逻辑身份 | 中 |
| `pid` | 运行态观测、诊断、kill/attach 辅助 | 低 |

约束：

- 禁止以 `pid` 作为持久化通信对象主键
- 允许通过 `pid` 反查 `session_id/task_id/task_item_id/step_id/agent_id`
- CLI 和 gRPC 的查询结果中可以返回 `pid`，但所有写操作应优先接受 `session_id`

### 3. Mailbox 抽象

#### 3.1 功能定位

Mailbox 用于异步协作，不承担实时终端交互职责。典型场景：

- agent A 向 agent B 发起请求
- agent B 回复阶段结果或错误
- 调度者查看某个 agent 的收件箱、发件箱和未读消息
- 审计系统回放通信历史

#### 3.2 建议消息模型

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
  message_type: request
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

#### 3.3 建议状态

Mailbox message 生命周期：

`queued -> delivered -> acknowledged -> responded -> expired`

补充失败态：

`delivery_failed`, `receiver_unavailable`, `discarded`

### 4. Session 抽象

#### 4.1 功能定位

Session 是 shell 交互的正式控制面，用于：

- 让 agent 或用户 attach 到一个正在运行的 agent 会话
- 向会话写入输入
- 从会话读取 transcript 或结构化输出
- 管理 writer / reader lease
- 查询某个 agent 当前是否在线、正在执行什么任务、对应哪个 `session_id`

#### 4.2 Session 与现有运行时的关系

现有实现已经部分具备以下基础：

- TTY step 会创建 `session_id`
- 会话有 `input_fifo_path`
- 有 `transcript_path` 和 `output_json_path`
- 有 writer / reader attachment 的数据模型

因此后续实现建议不是另起炉灶，而是在现有 `agent_sessions` 基础上补齐外部接口和运行态治理。

#### 4.3 Session 建议状态

`opening -> active -> detached -> draining -> closed -> failed`

说明：

- `active`: 至少一个 writer 或 shell 进程仍在线
- `detached`: agent 进程仍在运行，但当前无交互方附着
- `draining`: 会话收到关闭请求，拒绝新 writer
- `closed`: 正常结束
- `failed`: 非正常退出、FIFO/PTY 错误、进程异常终止

### 5. 建议 CRD 草案

本节是接口草案，不要求后续必须完全按 CRD 落地；也允许部分能力只通过 gRPC + CLI 暴露。

#### 5.1 AgentSession CRD

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
  mode: tty
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

建议字段：

- `spec.agent_id`
- `spec.task_id`
- `spec.task_item_id`
- `spec.step_id`
- `spec.mode`: `tty | noninteractive`
- `spec.access.max_readers`
- `spec.access.max_writers`
- `spec.access.allow_human_attach`
- `spec.access.allow_agent_attach`
- `spec.retention.transcript_ttl_hours`
- `spec.retention.output_ttl_hours`
- `status.session_id`
- `status.state`
- `status.pid`
- `status.run_id`
- `status.attached_readers`
- `status.attached_writer`

#### 5.2 AgentMailbox CRD

```yaml
apiVersion: orchestrator.dev/v2
kind: AgentMailbox
metadata:
  name: coder
spec:
  agent_id: coder
  retention_days: 7
  max_pending_messages: 1000
  delivery_mode: at_least_once
status:
  unread_count: 3
  in_flight_count: 1
  last_message_at: "2026-03-21T12:03:00Z"
```

建议字段：

- `spec.agent_id`
- `spec.retention_days`
- `spec.max_pending_messages`
- `spec.delivery_mode`: `fire_and_forget | at_least_once`
- `status.unread_count`
- `status.in_flight_count`
- `status.last_message_at`

#### 5.3 AgentMessage CRD

建议作为审计/调试导出对象，未必要求用户手工 apply。更可能由系统自动生成并通过 `get/describe` 查看。

建议字段：

- `spec.sender`
- `spec.receiver`
- `spec.message_type`: `request | response | ack | publish | control`
- `spec.subject`
- `spec.reply_to`
- `spec.correlation_id`
- `spec.ttl_secs`
- `spec.require_ack`
- `spec.payload.kind`
- `spec.payload.data`
- `status.state`
- `status.delivered_at`
- `status.acknowledged_at`
- `status.responded_at`
- `status.error`

### 6. 建议 CLI 形状

#### 6.1 Mailbox CLI

```bash
orchestrator agent mailbox list [--agent <agent_id>] [--task <task_id>] [--state unread]
orchestrator agent mailbox send \
  --from-agent planner \
  --to-agent coder \
  --task task-123 \
  --item item-01 \
  --subject "实现登录 API" \
  --payload-file payload.json
orchestrator agent mailbox ack <message_id>
orchestrator agent mailbox reply <message_id> --payload-file reply.json
orchestrator agent mailbox wait --agent coder --timeout 30
orchestrator agent mailbox get <message_id> -o json
```

查询和展示建议支持：

- `--agent`
- `--task`
- `--item`
- `--step`
- `--session`
- `--pid`

其中：

- `--pid` 仅作为查询过滤条件
- 所有写操作不得只接受 `--pid`，必须能解析到具体 `session_id` 或显式目标 agent 上下文

#### 6.2 Session CLI

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

补充要求：

- `attach` 默认 `reader`
- `writer` 需要显式申请 lease
- `resolve --pid` 用于把 `pid` 映射到 `session_id/task_id/task_item_id/step_id/agent_id`

### 7. 建议 gRPC API 形状

以下为建议的 proto 轮廓，字段名允许后续调整。

#### 7.1 Mailbox RPC

```proto
rpc AgentMessageSend(AgentMessageSendRequest) returns (AgentMessageSendResponse);
rpc AgentMessageList(AgentMessageListRequest) returns (AgentMessageListResponse);
rpc AgentMessageGet(AgentMessageGetRequest) returns (AgentMessageGetResponse);
rpc AgentMessageAck(AgentMessageAckRequest) returns (AgentMessageAckResponse);
rpc AgentMessageReply(AgentMessageReplyRequest) returns (AgentMessageReplyResponse);
rpc AgentMessageWait(AgentMessageWaitRequest) returns (stream AgentMessageEnvelope);
```

建议请求字段：

- `sender`
- `receiver`
- `subject`
- `payload_json`
- `correlation_id`
- `ttl_secs`
- `require_ack`

建议查询字段：

- `agent_id`
- `task_id`
- `task_item_id`
- `step_id`
- `session_id`
- `pid`
- `state_filter`

#### 7.2 Session RPC

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

`AgentSessionAttach` 建议返回：

- `session_id`
- `lease_mode`: `reader | writer`
- `client_id`
- `writer_granted`

`AgentSessionRead` 建议输出：

- `timestamp`
- `stream`: `stdout | stderr | transcript | output_json`
- `chunk`
- `offset`
- `eof`

`AgentSessionResolvePid` 建议输出：

- `pid`
- `session_id`
- `run_id`
- `task_id`
- `task_item_id`
- `step_id`
- `agent_id`
- `state`

### 8. 推荐实施顺序

#### Phase 1: 只做观测与解析

- 提供 `pid -> session/task/item/step/agent` 查询接口
- 提供 `session list/get/inspect`
- 不开放 agent-to-agent 写入

#### Phase 2: 做 Mailbox

- 持久化消息表
- `send/list/get/ack/reply`
- CLI 与 gRPC 基础能力

#### Phase 3: 做 Session 控制

- 对现有 `agent_sessions` 补齐 attach/send-input/read/close
- writer/reader lease 治理
- transcript/read follow

#### Phase 4: 高阶能力

- agent 直接 attach 到其他 agent session
- 权限治理、配额、限流
- GUI/可视化观测

### 9. 风险与边界

#### 9.1 主要风险

- `pid` 复用导致错误映射
- shell 子进程树复杂，顶层 pid 不一定代表真实工作进程
- 会话写入可能产生并发冲突
- interactive session 的安全边界比普通 task command 更敏感
- FIFO/PTY 在不同平台上的行为差异可能扩大实现复杂度

#### 9.2 缓解策略

- 强制以 `session_id` 为写入口主键
- `pid` 仅做短生命周期查询索引
- writer lease 只允许单 writer
- 所有 session 输入输出都进入审计链
- 先支持当前已验证的 shell/FIFO 模式，再评估完整 PTY multiplexing

### 10. 非目标

- 本 FR 不要求立即支持跨主机/分布式 agent 通信
- 本 FR 不要求立即支持音视频式“通话”
- 本 FR 不要求在第一阶段实现 GUI
- 本 FR 不要求废弃现有 `collab::MessageBus`

## 成功标准

1. 形成一份明确区分 `Mailbox` 与 `Session` 的正式接口草案
2. 后续实施时，团队能够依据本 FR 直接落设计文档与 proto/CLI 方案，而无需重新发散
3. `pid` 被明确降级为观测辅助字段，`session_id/run_id` 被明确为主标识
4. 文档明确约束 shell 交互面的能力边界与实施阶段
5. 文档显式记录“实施前需再次与用户对齐”的要求

## 约束

1. 后续实现前，**必须再次与用户进行需求对齐**，确认本 FR 的抽象、命名、优先级和范围仍然有效，避免因用户遗忘或上下文变化导致误实现
2. 若未来实现发现现有 `agent_sessions` 数据模型不足，可以调整字段，但不得违背“`session_id` 为主标识、`pid` 为辅助标识”的原则
3. Mailbox 与 Session 可以分阶段交付，不要求一次性全部实现
4. 若实现复杂度过高，优先保证 `Session inspect/resolve/list` 和 `Mailbox send/list/get`，再考虑更高级的 attach/write/follow

## 关联文档

- 架构总览：`docs/architecture.md`
- Agent Orchestrator 指南：`/Users/chenhan/c9r-io/orchestrator/.claude/skills/orchestrator-guide/SKILL.md`
- 现有 daemon 生命周期设计：`docs/design_doc/orchestrator/24-daemon-lifecycle-runtime-metrics.md`
