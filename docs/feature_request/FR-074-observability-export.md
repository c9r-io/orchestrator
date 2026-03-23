# FR-074: 可观测性导出 — Prometheus Metrics 端点

## 优先级: P2

## 状态: Proposed

## 背景

当前 agent metrics、task 执行指标、事件统计等数据存储在 SQLite 中，仅通过 CLI 命令查询。无法接入 Prometheus/Grafana 等现有监控体系，限制了生产环境的运维能力。

## 需求

### 1. Prometheus Metrics HTTP 端点
- 在 `orchestratord` 上暴露 `/metrics` HTTP 端点（独立于 gRPC 端口）
- 可通过 `--metrics-bind` 参数配置监听地址（默认 `127.0.0.1:9090`）
- 输出格式: Prometheus text exposition format

### 2. 导出指标

**Daemon 级**:
- `orchestratord_uptime_seconds` — daemon 运行时长
- `orchestratord_incarnation` — 重启次数
- `orchestratord_workers_configured` / `_active` — worker 配置/活跃数

**Task 级**:
- `orchestratord_tasks_total{status}` — 各状态任务数
- `orchestratord_task_items_total{status}` — 各状态 item 数
- `orchestratord_task_duration_seconds` — 任务执行耗时 histogram

**Agent 级**:
- `orchestratord_agent_health_score{agent}` — 健康分数
- `orchestratord_agent_success_rate{agent}` — 成功率
- `orchestratord_agent_inflight{agent}` — 在途任务数

**事件级**:
- `orchestratord_events_total{type}` — 事件计数
- `orchestratord_event_cleanup_last_run` — 上次清理时间戳

### 3. 可选: OpenTelemetry 支持
- 作为 follow-up，支持 OTLP exporter
- 可复用 `tracing-opentelemetry` crate

## 验收标准

- [ ] `curl http://127.0.0.1:9090/metrics` 返回合法的 Prometheus 格式
- [ ] Prometheus server 能 scrape 并显示指标
- [ ] `--metrics-bind` 参数可自定义端口
- [ ] 指标值与 `orchestrator task list` / `orchestrator agent list` 一致
