# QA Doc 106: Inflight Wait Heartbeat-Aware Timeout (FR-052)

**关联**: Design Doc 64, FR-038 (Design Doc 50)

---

## 验证场景

### Scenario 1: Heartbeat 重置超时计时器

**前提**: workflow 配置 `inflight_wait_timeout_secs: 10`, `inflight_heartbeat_grace_secs: 5`

**步骤**:
1. 启动 task，使 agent 子进程每 3 秒发出 step_heartbeat 事件
2. 等待 post-loop 进入 `wait_for_inflight_runs()`
3. 观察日志：每次 heartbeat 应重置 `last_activity`

**预期**: 在 agent 持续活跃期间，不触发 `inflight_wait_timeout`

### Scenario 2: 无 heartbeat 时正常超时

**前提**: workflow 配置 `inflight_wait_timeout_secs: 10`

**步骤**:
1. 构造 in-flight run（exit_code=-1, PID 存活）但不发送任何 heartbeat
2. 等待 `wait_for_inflight_runs()` 超时

**预期**: 约 10 秒后触发 `inflight_wait_timeout` 事件，随后 `reap_inflight_runs` 清理子进程

### Scenario 3: 可配置超时参数 serde 默认值

**步骤**:
1. 用空 `safety: {}` 配置反序列化 SafetyConfig
2. 检查字段值

**预期**: `inflight_wait_timeout_secs=300`, `inflight_heartbeat_grace_secs=60`

### Scenario 4: 增强的超时诊断事件

**前提**: 触发 `inflight_wait_timeout`

**步骤**:
1. 查询 events 表中 `inflight_wait_timeout` 事件的 payload

**预期**: 包含 `elapsed_secs`, `since_last_activity_secs`, `remaining_runs`, `remaining_items`, `pids`, `timeout_secs`, `grace_secs`

### Scenario 5: 向后兼容 — 旧 YAML 无需修改

**步骤**:
1. 使用不包含 `inflight_wait_timeout_secs` / `inflight_heartbeat_grace_secs` 的旧 workflow YAML
2. 启动 daemon 并运行 task

**预期**: 编译通过，运行正常，使用默认值 300s/60s
