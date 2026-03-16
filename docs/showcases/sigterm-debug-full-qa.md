# SIGTERM 溯源 + Full-QA 回归测试 — 执行与监控 Prompt

> 用于 Claude Code session，复制整段作为 prompt 输入。
> 前提：在一台**闲置 Mac** 上执行，确保无其他进程干扰。

---

## 背景

orchestratord 在执行 full-qa 回归测试期间多次收到不明 SIGTERM 被杀。
两次复现的关键事实：

| 维度 | 第一次 (2026-03-16 AM) | 第二次 (2026-03-16 PM) |
|------|----------------------|----------------------|
| 存活时间 | ~3 分钟 | ~38 分钟 |
| Workers | 1 | 4 |
| Git hash | 9163065-dirty | db499d6 |
| SIGTERM 日志 | `received SIGTERM, shutting down` | 同左 |
| Daemon PPID | 1 (init-adopted) | 1 (init-adopted) |
| Agent 子进程是否发送 kill | 否（已排除） | 否（已排除） |
| Shutdown reason | `shutdown`（SIGTERM 路径） | 同左 |
| 系统是否重启 | 否 | 否 |
| OOM/dmesg | 无 | 无 |
| macOS unified log | 无相关记录 | 无相关记录 |

已排除的原因：
- Agent 子进程发送 kill（daemon-pid-guard hook 阻断）
- `orchestrator daemon stop` 子命令（上次有 agent 尝试，exit 2 失败）
- `cargo build --release` 替换 binary（仅编译 lib crate，未替换 daemon binary）
- 系统重启 / OOM killer
- daemon 内部 idle timeout（代码审查确认无此逻辑）

待验证假设：
1. macOS 系统级进程管理（launchd, power management, Spotlight indexer）
2. nohup 进程被 terminal session 或 Claude Code 会话清理
3. 某个 cron/launchd agent 清理孤儿进程
4. 极端情况：Rust tokio runtime 内部 panic 被转为 SIGTERM

---

## 执行步骤

请严格按以下顺序执行。你是**监控者**，不要修改任何源代码。

### Phase 0: 环境预检

```bash
# 1. 确认无残留进程
pgrep -f orchestratord   # 必须为空
pgrep -f 'claude -p'     # 必须为空

# 2. 确认系统刚启动或空闲
uptime
ps aux | wc -l            # 记录基线进程数

# 3. 检查是否有 launchd 任务可能干扰
launchctl list | grep -i "cleanup\|kill\|daemon\|cron" | head -20

# 4. 记录系统版本
sw_vers
uname -a
```

### Phase 1: 构建

```bash
cd /path/to/orchestrator   # 替换为实际路径
cargo build --release -p orchestratord -p orchestrator-cli
```

### Phase 2: 启动 Daemon + SIGTERM 审计

**关键：使用 dtrace 追踪 SIGTERM 来源。需要在单独终端手动运行。**

```bash
# 终端 1: 启动 daemon
nohup ./target/release/orchestratord --foreground --workers 4 > /tmp/orchestratord.log 2>&1 &
DAEMON_PID=$!
echo "DAEMON_PID=$DAEMON_PID"

# 等待启动
sleep 3
ps -o pid,ppid,pgid,comm -p $DAEMON_PID
cat data/daemon.pid
ls -la data/orchestrator.sock
```

**在终端 2（需要 sudo）运行 dtrace：**

```bash
# 方式 A: dtrace（精确追踪谁发送了 SIGTERM）
DAEMON_PID=<填入上面的PID>
sudo dtrace -qn "
proc:::signal-send
/args[1]->si_signo == 15 && args[0]->pr_pid == $DAEMON_PID/
{
    printf(\"%Y pid=%d (%s) sent SIGTERM to %d\n\",
           walltimestamp, pid, execname, args[0]->pr_pid);
}
" 2>&1 | tee /tmp/dtrace-sigterm.log

# 方式 B: 如果 dtrace 不可用，用 audit trail
sudo log stream --predicate 'eventMessage CONTAINS "SIGTERM"' --info 2>&1 | tee /tmp/sigterm-stream.log
```

**在终端 3 运行 watchdog 脚本：**

```bash
DAEMON_PID=<填入PID>
while true; do
    if ! ps -p $DAEMON_PID > /dev/null 2>&1; then
        echo "$(date -u) DAEMON DIED!"
        echo "--- dmesg ---"
        dmesg 2>/dev/null | tail -20
        echo "--- daemon log ---"
        tail -20 /tmp/orchestratord.log
        echo "--- last reboot ---"
        last reboot | head -3
        echo "--- dtrace output ---"
        cat /tmp/dtrace-sigterm.log 2>/dev/null
        break
    fi
    echo "$(date -u) alive pid=$DAEMON_PID"
    sleep 10
done
```

### Phase 3: 加载资源并创建任务

回到 Claude Code 终端：

```bash
export ORCHESTRATOR_SOCKET=data/orchestrator.sock

# 验证连接
orchestrator task list

# 加载资源
orchestrator init
orchestrator apply -f docs/workflow/claude-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/minimax-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/full-qa.yaml --project self-bootstrap

# 创建任务
orchestrator task create \
  -n "full-qa-regression" \
  -w full-qa -W full-qa \
  --project self-bootstrap \
  -g "对 docs/qa/ 下全部 QA 文档执行场景级回归测试，对失败项创建 ticket 并尝试修复，最终确保所有场景通过或明确记录未通过原因"
```

记录返回的 task_id。

### Phase 4: 监控循环

每 60 秒执行：

```bash
TASK_ID=<填入task_id>
DAEMON_PID=<填入PID>

# 进度
orchestrator task info $TASK_ID | head -8

# Daemon 存活（用 ps 而非 kill -0，避免触发 hook）
ps -p $DAEMON_PID -o pid,comm > /dev/null 2>&1 && echo "daemon ALIVE" || echo "DAEMON DEAD!"

# Agent 并行数
pgrep -f 'claude -p' | wc -l

# PID 文件一致性
cat data/daemon.pid

# Socket 存在性
ls -la data/orchestrator.sock
```

每 5 分钟检查一次详细状态：

```bash
# 已完成/失败/跳过的 items
orchestrator task info $TASK_ID | grep -E '\[qa_passed\]|\[qa_failed\]|\[skipped\]|\[unresolved\]'

# Tickets
ls docs/ticket/*.md 2>/dev/null | grep -v README

# Skipped 数量（预期 ~27 个 self_referential_safe: false）
orchestrator task info $TASK_ID | grep -c '\[skipped\]'
```

### Phase 5: 如果 Daemon 再次被杀

**立即执行**（按优先级排序）：

```bash
# 1. 检查 dtrace 输出（最关键！）
cat /tmp/dtrace-sigterm.log

# 2. Daemon 最后日志
tail -50 /tmp/orchestratord.log

# 3. 内核日志
dmesg 2>/dev/null | tail -20

# 4. 系统是否重启
last reboot | head -3

# 5. 检查是否有进程在事件发生时访问了 daemon.pid
# (事后无法查，但 dtrace 应已捕获)

# 6. 检查 /tmp/sigterm-stream.log（如果用了方式 B）
tail -50 /tmp/sigterm-stream.log 2>/dev/null
```

**dtrace 输出格式解读**：
```
2026-03-16 12:34:56 pid=12345 (some_process) sent SIGTERM to 4867
```
- `pid=12345` = 发送者的 PID
- `(some_process)` = 发送者的进程名
- 如果是 `launchd` → macOS 系统管理
- 如果是 `bash`/`zsh` → shell session 清理
- 如果是 `claude` → Claude Code 进程管理
- 如果无输出 → 可能是内核级信号（OOM/资源限制）

### Phase 6: 成功判定

如果 daemon 存活到 task 完成：

1. 全部 138 items 处理完毕
2. ~27 个 unsafe docs 被 skip
3. Pass rate ≥ 90%
4. 失败项有对应 ticket
5. **Daemon PID 从头到尾未变** ← 这是本次最重要的验证

---

## 上一轮已发现的 Bugs（3 个 ticket）

供参考，这些 bug 可能在本轮再次出现：

1. **qa108 - Step Progress Mismatch**: step_progress 计数把 running items 算成 completed
2. **qa110 - Agent Health Policy Not Wired**: `increment_consecutive_errors()` / `mark_agent_diseased()` 在生产代码中从未被调用
3. **qa015 - Multi-target loop_guard false failure**: 所有 items exit=0 但 loop_guard 仍标记 task 为 failed

---

## 注意事项

- **不要修改源代码** — 你是监控者
- **不要 kill daemon** — 除非用户明确要求
- 用 `ps -p PID` 代替 `kill -0 PID` 做存活检查（避免 hook 误报）
- 异常只记录为 ticket（`docs/ticket/`），不修复
- 如果 dtrace 捕获到 SIGTERM 来源，**立即报告用户**，这是本次最高优先级
