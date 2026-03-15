# Design Doc 64: Heartbeat-Aware Inflight Wait Timeout (FR-052)

**关联 FR**: FR-052
**前置**: FR-038 (Design Doc 50)
**状态**: Implemented
**日期**: 2026-03-15

---

## 1. 问题

FR-038 引入的 `wait_for_inflight_runs()` 使用 **硬编码 120 秒超时**，且 **不感知 heartbeat**。在大规模 QA 回归场景中（130 items, max_parallel=4），正常运行的 agent 子进程因超时被判定为 orphan，导致 task 过早失败。

### 复现路径

```
full-qa workflow, 130 items, max_parallel=4
  → item segment spawns items, JoinSet 等待完成
  → cycle 结束, post-loop 进入 wait_for_inflight_runs()
  → 4 个 items 仍有 exit_code=-1, PID 存活, heartbeat 活跃
  → 120s 后 inflight_wait_timeout 触发
  → items_compensated → task_failed
  → 实际仅完成 26/130 items
```

### 根因分析

`wait_for_inflight_runs()` 的三个退出条件：

| 条件 | 检查方式 | 问题 |
|------|---------|------|
| 所有 runs 完成 | `exit_code != -1` | 正确但慢 runs 无法受益 |
| 所有 PID 已死 | `libc::kill(pid, 0)` | 正确，但存活进程不会触发 |
| **超时** | **`elapsed >= 120s`** | **不区分 orphan 和正常 run** |

关键缺陷：heartbeat 事件表明子进程活跃且在正常工作，但超时逻辑完全忽略此信号。对于 FR-038 的设计意图（处理 daemon restart orphan），120s 足够。但对于正常执行路径中的 large item segment，120s 远不够（130 items × 2-5 min/item = 65-162 min）。

### 影响范围

- full-qa workflow: 130+ items, 必定触发
- self-bootstrap workflow: 通常 2-10 items, 不受影响
- self-evolution workflow: 2 items, 不受影响
- 任何 item count × avg_duration > 120s 的场景

---

## 2. 设计方案

### 2.1 方案概览

在 `wait_for_inflight_runs()` 中引入 **heartbeat-aware 活性检测**：若 in-flight run 的最近 heartbeat 在 `heartbeat_grace_period` 内，视为活跃，重置超时计时器。

### 2.2 可配置超时

在 `safety` 配置中新增字段：

```yaml
safety:
  inflight_wait_timeout_secs: 300   # 默认 300s (从 120 提升)
  inflight_heartbeat_grace_secs: 60 # heartbeat 宽限期, 默认 60s
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `inflight_wait_timeout_secs` | u64 | 300 | 无 heartbeat 活动时的最大等待秒数 |
| `inflight_heartbeat_grace_secs` | u64 | 60 | heartbeat 间隔超过此值视为 stale |

### 2.3 活性检测逻辑

```rust
async fn wait_for_inflight_runs(state, task_id, safety) -> Result<()> {
    let inflight = find_inflight_command_runs_for_task(state, task_id).await?;
    if inflight.is_empty() { return Ok(()); }

    let timeout = Duration::from_secs(safety.inflight_wait_timeout_secs);
    let grace = Duration::from_secs(safety.inflight_heartbeat_grace_secs);
    let poll_interval = Duration::from_secs(2);
    let mut last_activity = Instant::now();  // 新增：上次活动时间

    loop {
        // 超时判定：从上次活动算起，而非从开始算起
        if last_activity.elapsed() >= timeout {
            // ... emit inflight_wait_timeout
            break;
        }

        tokio::time::sleep(poll_interval).await;

        let remaining = find_inflight_command_runs_for_task(state, task_id).await?;
        if remaining.is_empty() { break; }

        // 检查 heartbeat 活性
        let has_active_heartbeat = check_recent_heartbeats(
            state, task_id, &remaining, grace
        ).await?;

        if has_active_heartbeat {
            last_activity = Instant::now();  // 重置超时计时器
        }

        // PID 死亡检查（保持不变）
        let all_dead = remaining.iter().all(|(_, _, _, pid)| {
            pid.map_or(true, |p| unsafe { libc::kill(p as i32, 0) } != 0)
        });
        if all_dead { break; }
    }

    Ok(())
}
```

### 2.4 Heartbeat 查询

新增查询函数：

```rust
/// 检查指定 in-flight runs 是否有近期 heartbeat 事件
async fn check_recent_heartbeats(
    state: &Arc<InnerState>,
    task_id: &str,
    inflight: &[(String, String, String, Option<i64>)],
    grace: Duration,
) -> Result<bool> {
    let cutoff = Utc::now() - chrono::Duration::seconds(grace.as_secs() as i64);
    let item_ids: Vec<&str> = inflight.iter().map(|(_, item_id, _, _)| item_id.as_str()).collect();

    // SELECT COUNT(*) FROM events
    // WHERE task_id = ? AND event_type = 'step_heartbeat'
    //   AND item_id IN (...)
    //   AND created_at >= ?
    let count = count_recent_heartbeats(state, task_id, &item_ids, &cutoff).await?;
    Ok(count > 0)
}
```

### 2.5 增强的超时事件

超时时发出更多诊断信息：

```rust
"inflight_wait_timeout",
json!({
    "elapsed_secs": total_elapsed.as_secs(),
    "since_last_activity_secs": last_activity.elapsed().as_secs(),
    "remaining_runs": remaining.len(),
    "remaining_items": remaining.iter().map(|(_, item_id, _, _)| item_id).collect::<Vec<_>>(),
    "pids": remaining.iter().filter_map(|(_, _, _, pid)| *pid).collect::<Vec<_>>(),
    "has_recent_heartbeat": has_active_heartbeat,
})
```

---

## 3. 关键变更

| 文件 | 变更 |
|------|------|
| `orchestrator-config/src/config/execution.rs` | `SafetyConfig` 新增 `inflight_wait_timeout_secs`, `inflight_heartbeat_grace_secs` 字段 |
| `orchestrator-config/src/config/yaml_types.rs` | YAML serde 映射 |
| `core/src/task_repository/queries.rs` | 新增 `count_recent_heartbeats()` 查询 |
| `core/src/task_repository/trait_def.rs` | 新增 trait 方法 |
| `core/src/task_repository/mod.rs` | trait impl |
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/mod.rs` | 重写 `wait_for_inflight_runs()` |

---

## 4. 行为矩阵

| 场景 | heartbeat? | PID 存活? | 行为 |
|------|-----------|----------|------|
| Daemon restart orphan, 子进程已死 | 无 | 否 | 立即退出（PID dead） |
| Daemon restart orphan, 子进程存活但无 heartbeat | 无 | 是 | `timeout` 后超时 |
| 正常执行, agent 活跃 | 有 | 是 | 持续等待（计时器持续重置） |
| Agent 挂起, 无 heartbeat 无输出 | 无 | 是 | `timeout` 后超时 |
| Agent 偶发 heartbeat | 有 (间歇) | 是 | 每次 heartbeat 重置，宽限期内等待 |

---

## 5. 向后兼容性

- `inflight_wait_timeout_secs` 默认 300（原 120 硬编码提升为 300）
- `inflight_heartbeat_grace_secs` 默认 60
- 未配置这两个字段的旧 workflow YAML 使用默认值，行为与旧版本接近但更宽容
- `serde(default)` 确保旧 YAML 无需修改

---

## 6. 与 stall detection 的区别

| 机制 | 目的 | 触发条件 | 行为 |
|------|------|---------|------|
| **Stall detection** (FR-045) | 杀死卡住的 agent | 30 个连续 stagnant heartbeats (~15 min) | kill 进程, exit=-7 |
| **Inflight wait** (FR-038) | post-loop orphan 清理 | 循环结束后仍有 exit_code=-1 的 runs | 等待 → 超时 → task_failed |
| **本 FR** (FR-052) | 使 inflight wait 感知 heartbeat | FR-038 + heartbeat 活性 | 活跃时持续等待，stale 时超时 |

三者互补：stall detection 在 step 执行期间保护单个进程；inflight wait 在 post-loop 保护 task 完成判定；本 FR 让 inflight wait 不误杀正常工作的进程。

---

## 7. 关联 Bug: Zombie Reaping

超时后 `wait_for_inflight_runs()` 仅 break，不对 orphan 子进程执行 kill 或 waitpid。
已退出的子进程变为 zombie（Z 状态），DB 中 exit_code 永久滞留 -1。

详见 ticket: `docs/ticket/20260315-zombie-reaping-after-task-failed.md`

本 FR 实现时应同时解决：超时后对 remaining runs 执行 SIGTERM → grace → SIGKILL，
并更新 DB 中 exit_code 和 ended_at。

---

## 8. 测试策略

### 7.1 单元测试

- `check_recent_heartbeats` 返回 true/false 的边界测试
- `SafetyConfig` serde 默认值 round-trip
- `inflight_wait_timeout_secs=0` 边界：立即超时

### 7.2 集成测试

- 模拟 4 个 in-flight runs + 活跃 heartbeat → 不超时
- 模拟 2 个 in-flight runs + 无 heartbeat → `timeout` 后超时
- 模拟混合：1 个有 heartbeat + 1 个无 → heartbeat 重置计时器

### 7.3 QA 验证

- 复现原始失败场景：130 items, max_parallel=4, 确认 task 不再过早 fail
- 配置 `inflight_wait_timeout_secs: 10` 验证小值时快速超时
