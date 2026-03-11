# FR-013 gRPC 控制面速率限制与 DoS 防护

- ID: `FR-013`
- Priority: `P0`
- Status: `In Progress`
- Owner: `orchestrator core`
- Related baseline:
  - `FR-002`
  - `FR-010`

## 背景

当前 daemon 控制面已经具备 gRPC 接口、认证鉴权与审计能力，但尚未形成系统级请求速率限制和资源保护机制。对于本地单机工具，这一缺口短期内不总是显性问题；但一旦启用 TCP 控制面、并发 worker、长连接 watch/log 流，异常客户端或恶意流量就可能挤占服务资源。

gRPC 控制面目前更像“可信客户端使用前提下的功能面”，还不是“对不可信请求具备韧性的资源面”。

## 当前实现状态（2026-03-12）

### 已落地

- daemon 启动时会加载或生成 `data/control-plane/protection.yaml`，提供默认配额与 per-RPC override 入口。
- 所有 gRPC RPC 已接入统一保护模块，按 `read` / `write` / `stream` / `admin` 四类应用默认预算。
- TCP 模式优先按 mTLS `subject_id` 隔离；识别失败时退化为 `remote_addr`；UDS 退化为 `local-process`，不会因为主体缺失而放行。
- unary RPC 具备速率限制与并发限制；`TaskFollow` / `TaskWatch` 具备建连速率限制与活跃流数量限制。
- 超额请求统一返回稳定的 gRPC 拒绝语义：
  - `RESOURCE_EXHAUSTED`: `rate_limited` / `concurrency_limited` / `stream_limit_exceeded`
  - `UNAVAILABLE`: `load_shed`
- 限流拒绝会写入 `control_plane_audit`，补充 `traffic_class`、`limit_scope`、`decision`、`reason_code` 字段，并输出结构化 tracing。

### 默认配额

- `read`
  - subject: `20 rps`, `burst=40`, `max_in_flight=32`
  - global: `100 rps`, `burst=200`, `max_in_flight=128`
- `write`
  - subject: `5 rps`, `burst=10`, `max_in_flight=8`
  - global: `25 rps`, `burst=50`, `max_in_flight=32`
- `stream`
  - subject: `1 open/s`, `burst=2`, `max_active_streams=2`
  - global: `8 open/s`, `burst=16`, `max_active_streams=32`
- `admin`
  - subject: `1 rps`, `burst=2`, `max_in_flight=1`
  - global: `5 rps`, `burst=10`, `max_in_flight=4`

### 尚未闭环

- 当前实现是 `crates/daemon` 内的统一保护模块与 RPC 入口收口，不是严格意义上的 `tower` middleware / layer 形态。
- 设计文档与 QA 文档仍未补齐，当前 FR 还不能删除。
- 尚未补充高并发压测与场景化 QA 记录，因此“稳定拒绝而不退化崩溃”的验证仍停留在单元测试与编译回归层。

## 问题陈述

- 高频请求、并发流式订阅、恶意重试可导致 CPU、内存、文件句柄和 SQLite 连接竞争放大。
- 认证、授权、业务处理目前缺少统一前置限流层。
- 无论是误配置客户端还是有意攻击，都可能将 daemon 推入拒绝服务状态。

## 目标

- 为 gRPC 控制面引入统一速率限制与基础 DoS 防护。
- 在尽量不破坏本地正常开发体验的前提下，为高风险 RPC 设置更严格的配额和并发约束。
- 优先使用 `tower` 中间件能力构建可组合的资源保护层。

## 非目标

- 不在本 FR 中引入完整的 API 网关或外部 WAF。
- 不实现跨节点分布式限流。
- 不试图解决网络层 DDoS；本 FR 只覆盖应用层和进程内资源保护。

## 范围

### In

- gRPC unary / streaming RPC 的速率限制
- 高成本 RPC 的并发上限
- 拒绝结果的标准化错误语义
- 限流命中观测与审计

### Out

- 传输层 SYN flood 等网络设备问题
- 外部网关层面的通用流量清洗

## 需求

### 1. 统一中间件接入

- 控制面必须通过统一中间件层接入速率限制，而不是在各个 RPC handler 中零散实现。
- 实现应优先基于 `tower` 中间件能力，以便后续叠加超时、并发上限、负载保护和观测。

实现对齐说明：

- 已满足“统一收口，不在业务 service 层散落实现”。
- 当前尚未满足“基于 `tower` layer 装配”的目标，后续应把现有保护模块从 RPC 入口调用迁移到真正的 transport/middleware 栈。

### 2. 分级限流

- 至少区分以下 RPC 类别：
  - 低成本读请求
  - 高成本写请求
  - 流式订阅请求
  - 管理员高风险请求
- 不同类别必须允许配置独立配额、突发窗口和并发上限。

### 3. 主体与来源维度

- 在安全 TCP 模式下，限流应优先支持按主体或证书身份维度隔离。
- 在 UDS / 本地模式下，至少支持全局进程内限流。
- 无法识别主体时，必须退化到来源级或全局级保护，而不是放行。

### 4. 可观测性与审计

- 限流命中必须暴露结构化事件或指标。
- 至少记录：
  - RPC 名称
  - 主体或来源
  - 限流类别
  - 拒绝原因
  - 命中时间

### 5. 行为约束

- 被限流请求必须返回稳定、可识别的 gRPC 错误语义。
- streaming RPC 必须防止通过无限订阅数量耗尽资源。
- 限流不能破坏正常单用户本地开发的基本操作路径。

## 验收标准

- [x] gRPC 控制面接入统一保护入口。
- [x] 读、写、流式 RPC 至少具备一套默认限流配置。
- [x] 限流命中可在日志或审计中观察。
- [ ] 高并发请求压测下，daemon 能稳定拒绝超额请求而非退化崩溃。
- [x] 本文档已说明默认配额、调优入口和本地开发退化策略（UDS 使用 `local-process` + 全局预算）。
- [ ] 控制面保护收口到 `tower` middleware / layer。
- [ ] 设计文档与 QA 文档补齐。

## 风险与缓解

- 风险：配额过紧影响正常 CLI 使用。
  - 缓解：为本地默认场景保留保守但宽松的基线，并允许显式配置。
- 风险：流式请求限流策略不当，影响 `watch/logs --follow` 体验。
  - 缓解：对流式 RPC 使用独立预算与连接数控制。
- 风险：限流逻辑散落到业务层。
  - 缓解：要求统一通过 `tower` 中间件收口。

## 后续产物

- 设计文档：`docs/design_doc/orchestrator/27-grpc-control-plane-protection.md`
- QA 文档：`docs/qa/orchestrator/65-grpc-control-plane-protection.md`
