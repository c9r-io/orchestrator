# self-bootstrap 课题执行计划：follow_task_logs 流式回调重构

本文档基于 [`self-bootstrap-execution-template.md`](self-bootstrap-execution-template.md) 生成，用于验证 orchestrator 的自举能力。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`follow_task_logs 流式回调重构`
>
> 背景：
> `follow_task_logs_stream()` 在 `core/src/service/task.rs:129-146` 接收一个 `send_fn` 回调参数，
> 但该参数完全被忽略（参数名为 `_send_fn`），函数直接委托给 `follow_task_logs()`，
> 后者将日志内容通过 `print!()` / `eprint!()` 写入 stdout/stderr（`core/src/scheduler/query/log_stream.rs:279-283`）。
>
> 这导致 gRPC `TaskFollow` 端点虽然搭建了 streaming channel（`crates/daemon/src/server/task.rs:304-337`），
> 传入了将日志行发送到 channel 的 `send_fn`，但日志实际走了 stdout 而非 channel。
> 远程客户端通过 gRPC `TaskFollow` 只能收到一个空流。
>
> 本轮任务目标：
> 重构 `follow_task_logs` 使其通过回调函数输出日志，而非直接写 stdout/stderr。
> 让 `follow_task_logs_stream()` 正确使用 `send_fn` 参数。
>
> 约束：
> 1. `follow_task_logs` 的核心逻辑（poll 日志文件、检测 phase 切换、检查 task 终态、redaction）保持不变。
> 2. 现有的 `orchestrator task logs --follow`（CLI 本地模式）行为不变——仍然输出到 stdout。
> 3. gRPC `TaskFollow` 端点能通过 channel 正确接收日志行。
> 4. 不引入新的依赖。
> 5. 为重构后的回调模式补充单元测试。

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 判断是否需要）。
3. 代码改动（预期涉及 `core/src/scheduler/query/log_stream.rs`、`core/src/service/task.rs`，可选 `crates/daemon/src/server/task.rs`）。
4. 新增单元测试覆盖回调模式。
5. 自举回归验证结果。

### 1.2 非目标

- 不改变 `stream_task_logs_impl` 的行为（历史日志查询不受影响）。
- 不改 proto 定义（`TaskFollowRequest`/`TaskLogLine` 已存在）。
- 不要求本轮同时解决 `task watch` 的其他 TODO。
- 不重新设计日志存储格式。

### 1.3 课题选择理由

1. **唯一代码级 TODO**：`service/task.rs:142` 的 `// TODO: Phase 3` 是代码库中仅存的 TODO 注释。
2. **跨 crate 协调**：改动横跨 core（`log_stream.rs`、`service/task.rs`）和 daemon（`server/task.rs`），检验对模块边界的理解。
3. **self_test 有效性**：改动触及日志流公共 API，编译失败和签名变更会被 self_test 拦截。
4. **可观测完成态**：gRPC `TaskFollow` 端点从返回空流变为返回真实日志行。
5. **范围可控**：预计 2-3 个文件、60-100 行改动，适合 2-cycle 自举验证。

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
cd /Volumes/Yotta/c9r-io/orchestrator

cargo build --release -p orchestratord -p orchestrator-cli

# 启动 daemon
nohup ./target/release/orchestratord --foreground --workers 2 > /tmp/orchestratord.log 2>&1 &

# 验证 daemon 运行
ps aux | grep orchestratord | grep -v grep
```

### 3.2 基线采集（改动前行为快照）

在启动任务前，采集当前 `follow_task_logs_stream` 的基线数据：

```bash
# 基线 1：确认 _send_fn 参数被忽略
grep -n "_send_fn" core/src/service/task.rs
# 预期：第 132 行参数名为 _send_fn（下划线前缀表示未使用）

# 基线 2：确认 follow_task_logs 直接写 stdout
grep -n "print!\|eprint!" core/src/scheduler/query/log_stream.rs
# 预期：第 279-283 行有 print! 和 eprint! 调用

# 基线 3：确认 TODO 存在
grep -n "TODO" core/src/service/task.rs
# 预期：第 142 行 "TODO: Phase 3 — refactor follow_task_logs to use a channel/callback"
```

### 3.3 初始化数据库并加载资源

```bash
orchestrator delete project/self-bootstrap --force
orchestrator init
orchestrator apply -f docs/workflow/claude-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/minimax-secret.yaml --project self-bootstrap
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
       (SELECT json_extract(config_json, '$.projects.\"self-bootstrap\".workflows')
        FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1)
     )
   );"
# 预期: ["self-bootstrap"]

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
  -n "follow-logs-callback-refactor" \
  -w self -W self-bootstrap \
  --project self-bootstrap \
  -g "课题：follow_task_logs 流式回调重构。背景：follow_task_logs_stream()（core/src/service/task.rs:129-146）接收 send_fn 回调但完全忽略它（参数名 _send_fn），直接委托 follow_task_logs() 写 stdout/stderr（core/src/scheduler/query/log_stream.rs:279-283）。gRPC TaskFollow 端点传入了 channel-based send_fn，但日志走 stdout 而非 channel，远程客户端收到空流。目标：(1) 重构 follow_task_logs 接受回调函数输出日志行，而非直接 print!/eprint!；(2) follow_task_logs_stream() 正确接线 send_fn；(3) CLI 本地 follow 模式行为不变（用 print! 的回调）；(4) 不引入新依赖；(5) 补充回调模式的单元测试。"
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

1. `plan` 是否识别出 `_send_fn` 未使用和 stdout 直写为根因
2. `plan` 是否提出将回调注入 `follow_task_logs` 的方案
3. `implement` 是否修改了 `log_stream.rs` 的 `follow_task_logs` 和 `follow_one_stream`
4. `implement` 是否保留了 CLI 本地 follow 的 stdout 行为（用 print! 的回调传入）
5. `self_test` 编译是否通过
6. 是否有新增的回调模式单元测试

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

本课题的核心验证点是 **API 行为变化**。以下是改动前后的预期对比。

### 5.1 follow_task_logs_stream 行为变化

| 维度 | 改动前 | 改动后（预期） |
|------|--------|---------------|
| `_send_fn` 参数 | 被忽略（下划线前缀） | 正常使用（`send_fn`），每行调用 |
| 日志输出目标 | stdout/stderr（`print!/eprint!`） | 通过回调函数传递 |
| gRPC `TaskFollow` 流 | 空流（无数据） | 包含实时日志行 |
| CLI `task logs --follow` | 输出到终端 | 行为不变（传入 print 回调） |

### 5.2 代码变化预期

| 文件 | 预期变化 |
|------|---------|
| `core/src/scheduler/query/log_stream.rs` | `follow_task_logs` 签名变更：接受回调参数；`follow_one_stream` 通过回调输出而非 `print!` |
| `core/src/service/task.rs` | `follow_task_logs_stream` 将 `send_fn` 传递给重构后的 `follow_task_logs`；删除 TODO 注释 |
| `crates/daemon/src/server/task.rs` | 可能无变化（gRPC 端已正确传入 `send_fn`） |

### 5.3 self_restart 行为验证

Cycle 1 的 `self_restart` 会用改动后的代码重建 daemon binary。需要确认：

1. `cargo build --release -p orchestratord` 成功（说明签名变更编译通过）
2. 新 binary 通过 `--help` 验证
3. exec() 热重载后 daemon PID 不变
4. Cycle 2 在新 binary 上继续执行

### 5.4 行为验证脚本（任务完成后手工执行）

当 self-bootstrap 完成后，用以下方式验证实际行为变化：

```bash
# 1. 验证 TODO 已删除
grep -n "TODO" core/src/service/task.rs
# 预期：无 "Phase 3" TODO

# 2. 验证 _send_fn 已改为 send_fn
grep -n "send_fn" core/src/service/task.rs
# 预期：参数名不再有下划线前缀

# 3. 验证 follow_task_logs 签名变更
grep -n "pub async fn follow_task_logs" core/src/scheduler/query/log_stream.rs
# 预期：函数签名包含回调参数

# 4. 验证 print!/eprint! 不再直接在 follow_one_stream 中
grep -n "print!\|eprint!" core/src/scheduler/query/log_stream.rs
# 预期：follow_one_stream 中不再有 print!/eprint!（或改为通过回调调用）

# 5. 编译验证
cargo check --workspace
cargo test --workspace --lib

# 6. 重建并重启 daemon（使用改动后的代码）
cargo build --release -p orchestratord -p orchestrator-cli
kill $(cat data/daemon.pid 2>/dev/null) 2>/dev/null; sleep 2
nohup ./target/release/orchestratord --foreground --workers 2 > /tmp/orchestratord-verify.log 2>&1 &
sleep 3

# 7. 创建一个短任务并测试 follow
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project verify-test 2>/dev/null
orchestrator task create \
  -n "verify-follow" \
  --project verify-test \
  -g "echo test"

# 在另一终端执行 follow（CLI 模式仍应输出到终端）
# orchestrator task logs --follow <task_id>

# 8. 清理
orchestrator delete project/verify-test --force
```

---

## 6. 关键检查点

### 6.1 Plan 阶段检查点

确认 orchestrator 理解的问题是：

1. **根因**：`follow_task_logs` 直接写 stdout/stderr，`follow_task_logs_stream` 忽略 `send_fn` 回调
2. **完成态**：日志通过回调输出，gRPC 端能接收日志行
3. **保留语义**：CLI 本地 follow 行为不变、redaction 逻辑不变、phase 切换检测不变

如果 plan 只提出"把 TODO 注释删了"而非重构输出路径，应判定为偏题。

### 6.2 Implement 阶段检查点

确认代码改动满足：

1. `follow_task_logs` 接受回调参数（如 `Fn(String)` 或 `mpsc::Sender<String>`）
2. `follow_one_stream` 通过回调输出而非 `print!`/`eprint!`
3. `follow_task_logs_stream` 正确传递 `send_fn` 给 `follow_task_logs`
4. 现有 `follow_task_logs` 的调用者（CLI 路径）传入 stdout-writing 回调以保持行为
5. 新增至少 1 个测试验证回调模式

如果改动只删了 TODO 注释或只改了参数名（去掉下划线），应判定为不满足目标。

### 6.3 Self-Restart 阶段检查点

确认：

1. `self_restart` 在 Cycle 1 的 `self_test` 之后执行
2. `cargo build --release -p orchestratord` 成功（含签名变更的代码可编译）
3. daemon 通过 exec() 热重载
4. Cycle 2 在新 binary 上继续

### 6.4 Self-Test 阶段检查点

确认：

1. `cargo check` 和 `cargo test` 通过
2. `follow_task_logs` 的签名变更不破坏现有调用链
3. 闸门未被绕过

### 6.5 Validation 阶段检查点（Cycle 2）

1. `qa_testing` 是否产出回归 ticket
2. `ticket_fix` 是否需要介入
3. `align_tests` 是否为回调模式补充了测试
4. `doc_governance` 是否有需要更新的 QA 文档

---

## 7. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 2 个 cycle 的 `self-bootstrap` 流程，在 `loop_guard` 正常收口。
2. `core/src/service/task.rs` 的 `_send_fn` 参数不再被忽略，TODO 注释已删除。
3. `core/src/scheduler/query/log_stream.rs` 的 `follow_task_logs` 通过回调输出日志。
4. `follow_one_stream` 不再直接调用 `print!`/`eprint!`。
5. CLI 本地 `task logs --follow` 行为不变（仍输出到终端）。
6. 行为验证（§5.4）确认代码结构符合预期。
7. `cargo test --workspace --lib` 通过（含新增的回调测试）。
8. 本轮没有留下新的未解决 ticket。

---

## 8. 异常处理

### 8.1 课题特有的异常场景

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| plan 只提出"删 TODO 注释" | plan 输出不包含回调/callback/channel 关键词 | 判定偏题 |
| implement 只改了参数名（`_send_fn` → `send_fn`） | `git diff` 只有 1 行变化 | 说明没有重构输出路径 |
| 签名变更导致 daemon 编译失败 | self_test 或 self_restart 报 cargo check 失败 | Cycle 2 implement 修复；若两轮都失败则人工介入 |
| 回调模式引入 lifetime/trait bound 错误 | cargo check 报 type error | 预期 implement 会迭代修复 |
| self_restart build 失败 | self_restart 事件显示 build error | `on_failure: continue`，不影响后续 |
| CLI follow 模式行为回归 | 改动后 CLI follow 不输出日志 | 说明回调接线有误，需修复传入的 print 回调 |

### 8.2 C/S 架构特有异常

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| daemon 未运行 | CLI 报 `failed to connect to daemon` | 用 `orchestratord --foreground --workers 2` 启动 |
| CLI 指向旧单体二进制 | `which orchestrator` 指向 `core/target/release/` | 更新 symlink 到 `target/release/orchestrator` |
| 重建后 daemon 仍用旧代码 | 观察到旧行为 | 杀掉旧 daemon 进程再启动新的 |

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
3. 执行基线采集（§3.2）和行为验证（§5.4）
4. 监控状态和行为变化
5. 在异常时中断并记录

人工不预设具体重构方式（用泛型回调、trait object、还是 channel sender），不手动修改代码。

---

## 10. 收尾清理

任务完成后，根据验证结果决定处理方式：

### 10.A 验证通过 — 保留改动

```bash
git add -p   # 审查并暂存改动
git commit -m "feat(core): refactor follow_task_logs to use callback instead of stdout"
```

### 10.B 验证失败或需要重跑 — 还原改动

```bash
# 还原 agent 修改的文件
git checkout HEAD -- core/src/scheduler/query/log_stream.rs \
  core/src/service/task.rs crates/daemon/src/server/task.rs

# 删除 agent 可能创建的新文件
git clean -fd core/src/ crates/

# 确认工作树干净
git status --short

# 验证编译
cargo check
```

> Agent 可能修改 `log_stream.rs`、`task.rs`（service 和 scheduler 两个）、`server/task.rs` 等文件。
> 每次执行后务必检查 `git diff --stat` 并根据 §7 成功判定决定保留还是还原。
