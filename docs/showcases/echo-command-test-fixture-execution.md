# self-bootstrap 测试 fixture：Echo CLI 子命令

本文档是一个**永久测试课题**，用于验证 orchestrator 的 self-bootstrap 能力。
课题本身可实现但不应合并——每次测试后还原代码即可重复使用。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`Echo CLI 子命令`
>
> 背景：
> 当前 orchestrator CLI 没有 `echo` 子命令。为了验证端到端的 RPC 通路（proto 定义 → daemon handler → CLI 命令），
> 需要新增一个 `orchestrator echo --message "hello"` 子命令，
> 它通过 gRPC 调用 daemon 的 `Echo` RPC，daemon 原样返回消息，CLI 打印结果。
>
> 本轮任务目标：
> 1. 在 `proto/orchestrator.proto` 中新增 `Echo` RPC、`EchoRequest`（含 `message` 字段）、`EchoResponse`（含 `reply` 字段）。
> 2. 在 `crates/daemon/src/server/system.rs` 中实现 `Echo` handler，原样返回 `request.message`。
> 3. 在 `crates/cli/src/commands/` 中新增 `echo.rs` 子命令，调用 daemon 的 `Echo` RPC 并打印结果。
> 4. 在 CLI 的 `mod.rs` 或主命令注册点中注册 `echo` 子命令。
> 5. 补充单元测试。
>
> 约束：
> 1. 不修改任何现有 RPC 或命令的行为。
> 2. `Echo` handler 需经过控制面授权检查（`authorize(server, &request, "Echo")`），角色为 `ReadOnly`。
> 3. 保持与现有代码风格一致（clap derive、tonic handler 模式）。
> 4. 不引入新的依赖。

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 判断是否需要）。
3. 代码改动（预期涉及 `proto/orchestrator.proto`、`crates/daemon/src/server/system.rs`、`crates/cli/src/commands/`）。
4. 新增单元测试。
5. 自举回归验证结果。

### 1.2 非目标

- 不改变任何现有 RPC 的签名或行为。
- 不引入新的 proto service（在现有 `OrchestratorService` 中添加）。
- 不需要 gRPC streaming——简单的 unary RPC 即可。

### 1.3 作为测试 fixture 的设计理由

1. **跨 crate 完整性**：改动横跨 proto → daemon → CLI 三层，验证 orchestrator 对模块边界的理解。
2. **编译门禁有效性**：proto 变更触发代码生成，签名不匹配会被 `self_test` 拦截。
3. **self_restart 验证**：新增 RPC 需要重建 daemon binary，`self_restart` 必须成功。
4. **完成态明确**：`orchestrator echo --message "test"` 返回 `test` 即成功。
5. **永远不该合并**：echo 命令对生产环境无价值。
6. **幂等可重复**：所有改动都是新增文件/字段，`git checkout` 干净还原，下次从 clean tree 再跑。
7. **课题不随代码演进失效**：不依赖特定函数名或内部结构，只要 proto + daemon + CLI 架构不变就有效。

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

### 3.2 基线采集

```bash
# 确认 Echo RPC 不存在
grep -n "Echo" proto/orchestrator.proto
# 预期：无匹配

# 确认 echo 子命令不存在
ls crates/cli/src/commands/echo.rs 2>/dev/null
# 预期：文件不存在

# 确认 system.rs 无 echo handler
grep -n "echo" crates/daemon/src/server/system.rs
# 预期：无匹配
```

### 3.3 初始化数据库并加载资源

```bash
orchestrator delete project/self-bootstrap --force
orchestrator init
orchestrator apply -f docs/workflow/claude-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/minimax-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap
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
  -n "echo-command-test" \
  -w self -W self-bootstrap \
  --project self-bootstrap \
  -g "课题：Echo CLI 子命令。在 proto/orchestrator.proto 中新增 Echo RPC（EchoRequest 含 message 字段，EchoResponse 含 reply 字段）。在 crates/daemon/src/server/system.rs 中实现 Echo handler，原样返回 request.message，需经过 authorize 检查（ReadOnly 角色）。在 crates/cli/src/commands/ 中新增 echo.rs 子命令，调用 daemon Echo RPC 并打印结果。在 CLI 主命令注册点注册 echo 子命令。补充单元测试。不修改任何现有 RPC 或命令的行为，不引入新依赖。"
```

记录返回的 `<task_id>`。

---

## 4. 监控方法

### 4.1 状态监控

```bash
orchestrator task list
orchestrator task info <task_id>
orchestrator task trace <task_id>
orchestrator task watch <task_id>
```

重点观察：

1. 当前 cycle（预期从 1 开始，最终到 2）
2. 当前步骤名称和顺序
3. task status 是否前进
4. `task trace` 中步骤顺序是否符合 §2 的 pipeline 定义

### 4.2 日志监控

```bash
orchestrator task logs --tail 100 <task_id>
orchestrator task logs --tail 200 <task_id>
```

重点观察：

1. `plan` 是否识别出需要跨 proto/daemon/CLI 三层改动
2. `implement` 是否正确生成 proto 定义、handler、CLI 命令
3. `self_test` 编译是否通过（proto codegen + handler 签名）
4. `self_restart` 是否成功重建含新 RPC 的 daemon binary

### 4.3 进程 / daemon 监控

```bash
ps aux | grep orchestratord | grep -v grep
ps aux | grep "claude -p" | grep -v grep
git diff --stat
```

---

## 5. 行为变化观察

### 5.1 代码变化预期

| 文件 | 预期变化 |
|------|---------|
| `proto/orchestrator.proto` | 新增 `rpc Echo`、`message EchoRequest`、`message EchoResponse` |
| `crates/daemon/src/server/system.rs` | 新增 `echo` handler，调用 `authorize`，返回 `EchoResponse { reply: req.message }` |
| `crates/daemon/src/server/mod.rs` | 路由注册（如需手动注册） |
| `crates/cli/src/commands/echo.rs` | 新增 `echo` 子命令，clap derive，调用 gRPC Echo |
| `crates/cli/src/commands/mod.rs` | 注册 echo 模块和子命令 |

### 5.2 self_restart 行为验证

Cycle 1 的 `self_restart` 会用含新 RPC 的代码重建 daemon binary。需要确认：

1. `cargo build --release -p orchestratord` 成功（proto codegen + 新 handler 编译通过）
2. 新 binary 通过 `--help` 验证
3. exec() 热重载后 daemon PID 不变
4. Cycle 2 在新 binary 上继续执行

---

## 6. 关键检查点

### 6.1 Plan 阶段检查点

确认 orchestrator 理解需要：

1. 修改 proto 文件新增 RPC 定义
2. 实现 daemon 侧 handler
3. 实现 CLI 侧子命令
4. 串联三层，保持现有行为不变

如果 plan 只提出修改某一层而忽略其他层，应判定为不完整。

### 6.2 Implement 阶段检查点

确认代码改动满足：

1. proto 中有 `Echo` RPC 定义
2. handler 中有 `authorize` 调用
3. CLI 中有 clap 注册和 gRPC 调用
4. 不修改任何现有 RPC

### 6.3 Self-Test 阶段检查点

1. `cargo check` 通过（proto codegen 成功）
2. `cargo test` 通过（无回归）
3. 闸门未被绕过

---

## 7. 成功判定

当以下条件同时成立，可判定本轮测试通过：

1. orchestrator 完整跑完 2 个 cycle 的 `self-bootstrap` 流程，在 `loop_guard` 正常收口。
2. `proto/orchestrator.proto` 中存在 `Echo` RPC 定义。
3. `crates/daemon/src/server/system.rs` 中存在 `echo` handler 且调用了 `authorize`。
4. `crates/cli/src/commands/` 中存在 `echo` 子命令。
5. `cargo test --workspace --lib` 通过。
6. 本轮没有留下新的未解决 ticket。

---

## 8. 测试后还原

**本课题不应合并。** 测试完成后，无论成功或失败，都应还原代码：

```bash
# 还原所有改动
git checkout HEAD -- proto/ crates/ core/

# 删除 agent 可能创建的新文件
git clean -fd proto/ crates/ core/ docs/qa/ docs/design_doc/

# 确认工作树干净
git status --short

# 验证编译
cargo check
```

下一次测试时从 clean tree 重新启动即可。

---

## 9. 人工角色边界

本计划中，人工角色明确限定为：

1. 提供目标
2. 启动 workflow
3. 执行基线采集（§3.2）
4. 监控状态和行为变化
5. 在异常时中断并记录
6. **测试结束后还原代码**

人工不预设具体实现方式，不手动修改代码。
