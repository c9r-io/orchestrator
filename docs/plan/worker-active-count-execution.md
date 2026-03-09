# self-bootstrap 课题执行计划：Worker 活跃计数真实化

本文档基于 [`self-bootstrap-execution-template.md`](self-bootstrap-execution-template.md) 生成，用于验证 orchestrator 的自举能力。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`Worker 活跃计数真实化`
>
> 背景：
> `WorkerStatusResponse.active_workers` 在 `core/src/service/system.rs:39` 硬编码为 `0`（`// TODO: track active worker count`）。
> daemon 启动多个 worker 后，`orchestrator task worker status` 始终报告 `active_workers=0`，
> 导致运维无法判断 daemon 当前负载、worker 是否在执行任务。
> Proto 字段已预留（`proto/orchestrator.proto:390`），但后端未接线。
>
> 本轮任务目标：
> 让 `worker_status()` 返回真实的活跃 worker 数量。当 worker 认领任务（claim）时计数 +1，
> 任务执行完毕（无论成功、失败、或 restart 退出）时 -1。空闲时 active_workers 为 0。
>
> 约束：
> 1. 优先解决根因（缺少运行时计数器），不接受仅修改硬编码返回值的表面绕过。
> 2. 保留已有核心语义：InnerState 的线程安全保证（RwLock/Arc）、worker loop 的 claim 原子性（`claim_next_pending_task`）、shutdown drain 的 30s 超时行为、RestartRequestedError 的信号传播。
> 3. 最终目标是：`active_workers` 等于当前正在执行任务的 worker 数量，空闲时为 0。

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 判断是否生成/更新 `docs/qa/orchestrator/53-client-server-architecture.md`）。
3. 与课题目标对应的代码改动（预期涉及 `core/src/state.rs`、`core/src/service/system.rs`、`crates/daemon/src/main.rs`）。
4. 自举回归验证结果。
5. 若本轮发现问题，由 `ticket_fix` 和后续步骤尝试收口。

### 1.2 非目标

- 不由人工预先定义实现细节（是用 `AtomicU32`、`AtomicUsize` 还是 `RwLock<u32>` 由 orchestrator 自行决定）。
- 不改 proto 定义（`WorkerStatusResponse.active_workers` 字段已存在）。
- 不要求本轮同时解决 `follow_task_logs` Phase 3 TODO。

### 1.3 课题选择理由

1. **真实技术债**：`system.rs:39` 的 TODO 是明确的代码缺陷，不是虚构课题。
2. **跨 crate 协调**：改动横跨 daemon（`crates/daemon`）和 core（`core/src`），检验 orchestrator 对多模块边界的理解。
3. **self_test 有效性**：改动触及 `InnerState` 核心结构，编译失败会被 self_test 拦截。
4. **可观测完成态**：通过 `orchestrator task worker status` 即可直接验证改动效果。
5. **范围可控**：预计 3-5 个文件、50-100 行改动，适合 2-cycle 自举验证。

---

## 2. 执行方式

本轮按 `self-bootstrap` 的标准链路执行：

```text
Cycle 1: plan -> qa_doc_gen -> implement -> self_test -> self_restart
Cycle 2: plan -> qa_doc_gen -> implement -> self_test -> [self_restart skipped] -> qa_testing -> ticket_fix -> align_tests -> doc_governance -> loop_guard
```

人工职责只有两类：

1. 启动和提供课题目标。
2. 监控执行状态、观察行为变化、判断是否卡住、记录结果。

---

## 3. 启动步骤

### 3.1 构建并启动 daemon

```bash
cd /Volumes/Yotta/ai_native_sdlc

cargo build --release -p orchestratord -p orchestrator-cli

# 启动 daemon
nohup ./target/release/orchestratord --foreground --workers 2 > /tmp/orchestratord.log 2>&1 &

# 验证 daemon 运行
ps aux | grep orchestratord | grep -v grep
```

### 3.2 基线采集（改动前行为快照）

在启动任务前，采集当前 `worker_status` 的基线数据，用于与改动后对比：

```bash
# 基线 1：空闲状态下查询 worker status
# 预期：active_workers=0（因为硬编码）
orchestrator task worker status
# 记录输出 → BASELINE_IDLE

# 基线 2：查看 proto 字段定义
grep -A3 "WorkerStatusResponse" proto/orchestrator.proto
# 预期：字段 active_workers 已存在

# 基线 3：查看硬编码位置
grep -n "active_workers" core/src/service/system.rs
# 预期：第 39 行 active_workers: 0
```

### 3.3 初始化数据库并加载资源

```bash
orchestrator db reset -f --include-config --include-history
orchestrator init -f
orchestrator apply -f docs/workflow/claude-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap
```

### 3.4 验证资源已加载

```bash
sqlite3 data/agent_orchestrator.db \
  "SELECT json_group_array(key) FROM (
     SELECT key FROM json_each(
       (SELECT json_extract(config_json, '$.projects.\"self-bootstrap\".workspaces')
        FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1)
     )
   );"
# 预期: ["self"]

sqlite3 data/agent_orchestrator.db \
  "SELECT json_group_array(key) FROM (
     SELECT key FROM json_each(
       (SELECT json_extract(config_json, '$.projects.\"self-bootstrap\".agents')
        FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1)
     )
   );"
# 预期: ["architect","coder","reviewer","tester"]
```

### 3.5 创建任务

```bash
orchestrator task create \
  -n "worker-active-count" \
  -w self -W self-bootstrap \
  --project self-bootstrap \
  -g "课题：Worker 活跃计数真实化。背景：WorkerStatusResponse.active_workers 硬编码为 0（core/src/service/system.rs:39），daemon worker 状态报告不准确。目标：让 worker_status() 返回真实的活跃 worker 数量——worker 认领任务时 +1，任务完毕（成功/失败/restart）时 -1。约束：(1) 在 InnerState 或 daemon 层引入原子计数器解决根因，不接受仅改硬编码值；(2) 保留 InnerState 线程安全、worker claim 原子性、shutdown drain 30s 行为、RestartRequestedError 信号传播；(3) 完成态：active_workers 等于正在执行任务的 worker 数，空闲时为 0。" \
  -t docs/qa/orchestrator/53-client-server-architecture.md
```

记录返回的 `<task_id>`。

---

## 4. 监控方法

### 4.1 状态监控

```bash
orchestrator task list
orchestrator task info <task_id>
orchestrator task trace <task_id>    # 带异常检测的执行时间线
orchestrator task watch <task_id>    # 实时刷新状态面板
```

重点观察：

1. 当前 cycle（预期从 1 开始，最终到 2）
2. 当前步骤名称和顺序
3. task status 是否前进（`running` → 不卡住）
4. `task trace` 中步骤顺序是否符合 §2 的 pipeline 定义
5. 是否出现 `failed`、`blocked`、长时间无进展

### 4.2 日志监控

```bash
orchestrator task logs --tail 100 <task_id>
orchestrator task logs --tail 200 <task_id>
```

重点观察：

1. `plan` 是否识别出 `system.rs:39` 硬编码为根因
2. `plan` 是否提出在 `InnerState` 或 daemon 层引入原子计数器
3. `implement` 是否在修改正确的文件（`state.rs`、`system.rs`、`main.rs`）
4. `implement` 是否正确处理了所有退出路径（成功、失败、restart、panic）
5. `self_test` 编译是否通过
6. Cycle 2 的 `qa_testing` 是否验证了 worker status 的行为变化

### 4.3 进程 / daemon 监控

```bash
# daemon 进程
ps aux | grep orchestratord | grep -v grep

# agent 子进程
ps aux | grep "claude -p" | grep -v grep

# 代码变更
git diff --stat
```

### 4.4 补充诊断命令

```bash
# 查看最近事件
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id = '<task_id>' ORDER BY id DESC LIMIT 20;"

# 查看 self_restart 相关事件
sqlite3 data/agent_orchestrator.db \
  "SELECT payload_json FROM events WHERE task_id = '<task_id>' AND event_type LIKE 'self_restart%' ORDER BY id DESC LIMIT 10;"
```

---

## 5. 行为变化观察

本课题的核心验证点是 **可观测的行为变化**。以下是改动前后的预期对比。

### 5.1 worker_status 返回值变化

| 场景 | 改动前（基线） | 改动后（预期） |
|------|---------------|---------------|
| 空闲（无任务运行） | `active_workers: 0` | `active_workers: 0` |
| 1 个 worker 正在执行任务 | `active_workers: 0` | `active_workers: 1` |
| 2 个 worker 同时执行任务 | `active_workers: 0` | `active_workers: 2` |
| 任务完成后立即查询 | `active_workers: 0` | `active_workers: 0`（回归空闲） |
| 任务失败后 | `active_workers: 0` | `active_workers: 0`（计数正确递减） |

### 5.2 代码变化预期

| 文件 | 预期变化 |
|------|---------|
| `core/src/state.rs` | `InnerState` 新增原子计数器字段 |
| `core/src/service/system.rs` | `worker_status()` 读取真实计数替代硬编码 `0` |
| `crates/daemon/src/main.rs` | `worker_loop` 在 claim 成功后 +1，task 结束后 -1 |
| `proto/orchestrator.proto` | **不变**（字段已存在） |

### 5.3 self_restart 行为验证

Cycle 1 的 `self_restart` 会用改动后的代码重建 daemon binary。需要确认：

1. `cargo build --release -p orchestratord` 成功（说明新增的原子计数器字段编译通过）
2. 新 binary 通过 `--help` 验证
3. exec() 热重载后 daemon PID 不变
4. Cycle 2 在新 binary 上继续执行

### 5.4 行为验证脚本（任务完成后手工执行）

当 self-bootstrap 完成后，用以下脚本验证实际行为变化：

```bash
# 1. 重建并重启 daemon（使用改动后的代码）
cargo build --release -p orchestratord -p orchestrator-cli
kill $(cat data/daemon.pid 2>/dev/null) 2>/dev/null; sleep 2
nohup ./target/release/orchestratord --foreground --workers 2 > /tmp/orchestratord-verify.log 2>&1 &
sleep 3

# 2. 验证空闲状态
echo "=== 空闲状态 ==="
orchestrator task worker status
# 预期：active_workers=0

# 3. 创建一个耗时任务来观察活跃计数
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project verify-test
orchestrator task create \
  -n "verify-active-count" \
  --project verify-test \
  -g "slow test task"

# 4. 立即查询 worker status（任务运行中）
sleep 2
echo "=== 任务运行中 ==="
orchestrator task worker status
# 预期：active_workers >= 1

# 5. 等待任务结束后再查询
sleep 30
echo "=== 任务完成后 ==="
orchestrator task worker status
# 预期：active_workers=0

# 6. 清理
orchestrator project reset verify-test --force --include-config
```

---

## 6. 关键检查点

### 6.1 Plan 阶段检查点

确认 orchestrator 理解的问题是：

1. **根因**：`core/src/service/system.rs:39` 硬编码 `active_workers: 0`，缺少运行时计数器
2. **完成态**：`worker_status()` 返回真实活跃 worker 数
3. **保留语义**：`InnerState` 线程安全、worker claim 原子性、shutdown drain

如果 plan 只提出"改硬编码值为某个常量"而非引入运行时计数，应判定为偏题。

### 6.2 Implement 阶段检查点

确认代码改动满足：

1. 在 `InnerState` 或独立共享状态中引入了原子计数器（如 `AtomicU32`/`AtomicUsize`）
2. `worker_loop` 中 claim 成功后递增、task 结束后递减（包括 Ok、Err、restart 三个路径）
3. `worker_status()` 读取计数器而非返回硬编码值
4. 使用了正确的内存序（`Ordering::Relaxed` 或更强）

如果改动只发生在 `system.rs` 一个文件（比如返回一个随机数），应判定为不满足目标。

### 6.3 Self-Restart 阶段检查点

确认：

1. `self_restart` 在 Cycle 1 的 `self_test` 之后执行
2. `cargo build --release -p orchestratord` 成功（含新字段的代码可编译）
3. daemon 通过 exec() 热重载
4. Cycle 2 在新 binary 上继续

### 6.4 Self-Test 阶段检查点

确认：

1. `cargo check` 和 `cargo test` 通过
2. 新增的计数器不破坏 `InnerState` 的 `Default` trait 或构造逻辑
3. 闸门未被绕过

### 6.5 Validation 阶段检查点（Cycle 2）

1. `qa_testing` 是否产出回归 ticket（预期无，因为改动是纯新增）
2. `ticket_fix` 是否需要介入
3. `align_tests` 是否为 `worker_status()` 补充了单元测试
4. `doc_governance` 是否更新了 `53-client-server-architecture.md` 的 Scenario 4 对 active_workers 的验证

---

## 7. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 2 个 cycle 的 `self-bootstrap` 流程，在 `loop_guard` 正常收口。
2. `core/src/service/system.rs:39` 的 `active_workers: 0` 硬编码已被真实计数器替代。
3. `crates/daemon/src/main.rs` 的 `worker_loop` 在 claim/finish 路径上正确维护计数。
4. 行为验证脚本（§5.4）确认：任务运行中 `active_workers >= 1`，空闲时 `active_workers == 0`。
5. `self_test` 仍能作为 builtin 正常执行。
6. 本轮没有留下新的未解决 ticket。

---

## 8. 异常处理

### 8.1 课题特有的异常场景

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| plan 把课题理解为"改 proto" | plan 输出提到修改 `.proto` 文件 | 判定偏题，proto 字段已存在 |
| implement 只改了 `system.rs` 一个文件 | `git diff --stat` 只显示一个文件 | 说明没有引入计数器，只是换了返回值 |
| 计数器只 +1 没 -1 | 行为验证发现任务结束后 `active_workers` 不回零 | 说明 implement 遗漏了 finish 路径 |
| self_test 编译失败 | self_test 步骤报 `cargo check` 失败 | 预期 Cycle 2 implement 会修复，若两轮都失败则人工介入 |
| self_restart build 失败 | self_restart 事件显示 build error | `on_failure: continue`，不影响后续，但说明代码有编译问题 |

### 8.2 C/S 架构特有异常

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| daemon 未运行 | CLI 报 `failed to connect to daemon` | 用 `orchestratord --foreground --workers 2` 启动 |
| CLI 指向旧单体二进制 | `which orchestrator` 指向 `core/target/release/` | 更新 symlink 到 `target/release/orchestrator` |
| 重建后 daemon 仍用旧代码 | 观察到 active_workers 仍为 0 | 杀掉旧 daemon 进程再启动新的 |

### 8.3 通用异常

记录状态、日志、diff，必要时人工接管：

```bash
orchestrator task info <task_id>
orchestrator task logs --tail 200 <task_id>
git diff --stat
```

---

## 9. 人工角色边界

本计划中，人工角色明确限定为：

1. 提供目标
2. 启动 workflow
3. 执行基线采集（§3.2）和行为验证脚本（§5.4）
4. 监控状态和行为变化
5. 在异常时中断并记录

人工不预设具体代码实现方式（用哪种原子类型、放在哪个结构体里），不手动修改代码。

---

## 10. 收尾清理

任务完成后，根据验证结果决定处理方式：

### 10.A 验证通过 — 保留改动

```bash
git add -p   # 审查并暂存改动
git commit -m "feat(daemon): track real active worker count in WorkerStatusResponse"
```

### 10.B 验证失败或需要重跑 — 还原改动

```bash
# 还原 agent 修改的文件
git checkout HEAD -- core/src/ crates/daemon/src/ crates/cli/src/

# 删除 agent 可能创建的新文件
git clean -fd core/src/ crates/

# 确认工作树干净
git status --short

# 验证编译
cargo check
```

> Agent 可能修改 `state.rs`、`system.rs`、`main.rs` 等核心文件。
> 每次执行后务必检查 `git diff --stat` 并根据 §7 成功判定决定保留还是还原。
