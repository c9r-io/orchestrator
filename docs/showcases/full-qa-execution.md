# 全量 QA 回归测试执行计划

本文档用于对当前代码库进行**单次迭代、全量 QA 回归测试**，不涉及代码改动。
适用场景：大批量重构后、发布前、或定期回归验证。

---

## 1. 任务目标

> 课题名称：`全量 QA 回归测试`
>
> 背景：
> 需要对当前代码库的所有 QA 文档（docs/qa/orchestrator/ + docs/qa/self-bootstrap/）
> 进行全面的场景级回归测试，确认各功能点均正常工作。
>
> 本轮任务目标：
> 遍历全部 128 个 QA 文档，逐一执行场景验证，对失败项创建 ticket，
> 由 ticket_fix 尝试修复，最后执行 align_tests 和 doc_governance 收口。
>
> 约束：
> 1. 本轮不做主动代码改动，仅在 ticket_fix 中修复 QA 发现的问题。
> 2. 保留所有现有行为不变。
> 3. 最终目标：所有 QA 场景通过或明确记录未通过原因。

### 1.1 预期产出

1. 全部 QA 场景的执行结果（pass/fail）。
2. 失败场景对应的 ticket（docs/ticket/）。
3. ticket_fix 对可修复项的自动修复。
4. align_tests 确保单测与代码一致。
5. doc_governance 确保文档无漂移。

### 1.2 执行链路

```text
qa_testing(item) → ticket_fix(item) → align_tests(task) → doc_governance(task) → loop_guard
```

单 cycle，无 plan/implement/self_test/self_restart。

---

## 2. 执行步骤

### 2.1 构建并确认 daemon 运行

```bash
cd /Volumes/Yotta/c9r-io/orchestrator

# 确认 daemon 运行
ps aux | grep orchestratord | grep -v grep

# 如未运行：
# nohup ./target/release/orchestratord --foreground --workers 4 > /tmp/orchestratord.log 2>&1 &
```

### 2.2 加载 full-qa workflow 资源

```bash
# 清理旧项目（如需要重新开始）
# orchestrator delete project/full-qa --force

# 初始化
orchestrator init

# 加载 secrets 和 execution profiles
orchestrator apply -f docs/workflow/claude-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/minimax-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap

# 加载 self-bootstrap 的 StepTemplates（full-qa 复用这些模板）
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap

# 加载 full-qa workflow
orchestrator apply -f docs/workflow/full-qa.yaml --project self-bootstrap
```

### 2.3 创建任务（全量扫描）

```bash
orchestrator task create \
  -n "full-qa-regression" \
  -w full-qa -W full-qa \
  --project self-bootstrap \
  -g "对 docs/qa/ 下全部 QA 文档执行场景级回归测试，对失败项创建 ticket 并尝试修复，最终确保所有场景通过或明确记录未通过原因"
```

> 不指定 `-t`，系统自动扫描 `qa_targets` 配置的 `docs/qa/` 下所有 `.md` 文件。
> 预计约 128 个 item。

记录返回的 `<task_id>`。

---

## 3. 监控方法

### 3.1 状态监控

```bash
orchestrator task list
orchestrator task info <task_id>
orchestrator task trace <task_id>
orchestrator task watch <task_id>
```

重点观察：

1. item 执行进度（已完成 / 总数）
2. qa_testing 步骤的 pass/fail 分布
3. ticket_fix 是否正在处理活跃 ticket
4. 是否有 item 长时间卡住

### 3.2 日志监控

```bash
orchestrator task logs --tail 200 <task_id>
```

重点观察：

1. 各 QA 文档的执行结果
2. ticket 创建和修复情况
3. prehook 跳过的自引用不安全文档

### 3.3 进程监控

```bash
# agent 子进程
ps aux | grep "claude -p" | grep -v grep | wc -l

# 预期最多 4 个并行（max_parallel: 4）
```

### 3.4 中间检查

当 item segment 完成约 50% 时，可检查：

```bash
# 查看已创建的 ticket
ls docs/ticket/

# 查看 ticket 数量
ls docs/ticket/*.md 2>/dev/null | wc -l
```

---

## 4. 关键检查点

### 4.1 QA Testing 阶段

- [ ] 所有非自引用不安全的 QA 文档都被执行
- [ ] 每个场景的 pass/fail 有明确结论
- [ ] 失败场景有对应的 ticket 文件

### 4.2 Ticket Fix 阶段

- [ ] 活跃 ticket 被尝试修复
- [ ] 修复后重新验证场景通过
- [ ] 无法修复的 ticket 保留并记录原因

### 4.3 Align Tests 阶段

- [ ] cargo test 全部通过
- [ ] cargo clippy 无警告
- [ ] 编译无警告

### 4.4 Doc Governance 阶段

- [ ] QA 文档无格式漂移
- [ ] README/manifest 一致性

---

## 5. 成功判定

当以下条件同时成立，可判定本轮全量 QA 完成：

1. orchestrator 完整跑完 `full-qa` workflow，在 `loop_guard` 正常收口。
2. QA 场景通过率 ≥ 90%（允许部分环境依赖的场景失败）。
3. 所有 ticket 被 ticket_fix 处理（修复或明确标记无法修复）。
4. `align_tests` 确认单测和编译无回归。
5. `doc_governance` 确认文档无漂移。

---

## 6. 异常处理

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| 大量 QA 文档同类失败 | 相同 pattern 的 ticket 超过 10 个 | 可能是系统性问题，暂停排查根因 |
| agent 进程僵死 | `claude -p` 进程无输出超过 10 分钟 | 检查 API 配额和网络 |
| ticket_fix 产生新问题 | 修复后 align_tests 失败 | 检查 ticket_fix 的改动范围 |
| daemon 内存过高 | 128 item 并发导致内存压力 | 降低 max_parallel 到 2 |

---

## 7. 预计执行时间

- **128 个 QA 文档** × **每个约 2-5 分钟** = 约 60-160 分钟（4 并行）
- ticket_fix 取决于 ticket 数量
- align_tests + doc_governance 约 10-20 分钟

总计预估：**1.5 - 3 小时**

---

## 8. 人工角色边界

本计划中，人工角色限定为：

1. 启动 workflow
2. 监控执行进度
3. 在系统性异常时中断
4. 记录最终结果

不人工干预具体 QA 场景的执行和 ticket 修复。
