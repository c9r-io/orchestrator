# FR-044: Sandbox 写入拒绝检测与 writable_paths 完善

- **优先级**: P1
- **状态**: Proposed
- **来源**: echo-command-test-fixture 执行监控 (2026-03-14)

---

## 1. 问题描述

### 1.1 writable_paths 不完整

`sandbox_write` 执行配置的 `writable_paths` 为 `[docs, core/src, crates, tests]`，缺少 `proto/`。任何涉及 proto 文件修改的跨 crate 任务（如新增 RPC）在 sandbox 内执行时，所有对 `proto/` 的写入都会被 macOS seatbelt 拒绝（EPERM），导致 implement 步骤静默失败。

### 1.2 Sandbox 拒绝静默吞没

implement 步骤在 19 次 EPERM 错误后仍报告 exit_code=0。下游步骤（self_test、self_restart）因无代码变更而"通过"，整个 pipeline 在未完成任何实际工作的情况下推进到 Cycle 2。

这形成了一个危险的假阳性链：
```
implement (EPERM×19 → exit 0) → self_test (无变更 → 通过) → self_restart (重建同一 binary) → Cycle 2 重复同样的失败
```

## 2. 需求

### 2.1 writable_paths 完善

在 `docs/workflow/execution-profiles.yaml` 的 `sandbox_write` profile 中添加 `proto` 到 `writable_paths`：

```yaml
writable_paths:
  - docs
  - core/src
  - crates
  - tests
  - proto          # <-- 新增
```

### 2.2 Sandbox 拒绝检测

在 implement 步骤的 finalize 逻辑中检测 sandbox 拒绝：

1. **计数器方案**：Runner 捕获 agent 进程日志中的 EPERM/sandbox 拒绝事件，累计到 pipeline 变量 `sandbox_denied_count`。
2. **Finalize 表达式**：若 `sandbox_denied_count > 0`，将步骤标记为失败（exit_code 非零），附带拒绝详情。
3. **用户可见诊断**：在 `task trace` 的 anomalies 中报告 sandbox 拒绝，包含被拒绝的文件路径。

### 2.3 self_test 空变更检测

当 `git diff --stat` 在 implement 步骤后为空时，self_test 应发出警告或直接判定为不通过（可配置），避免"无变更=通过"的假阳性。

## 3. 验收标准

1. 包含 proto 修改的 self-bootstrap 任务（如 Echo fixture）能在 sandbox 内成功写入 proto 文件。
2. 当 sandbox 拒绝关键写入时，implement 步骤报告非零 exit_code。
3. `task trace` 显示 sandbox 拒绝的 anomaly。
4. self_test 在无代码变更时发出警告。

## 4. 影响范围

- `docs/workflow/execution-profiles.yaml` — 添加 `proto` 路径
- `core/src/scheduler.rs` 或 runner 层 — sandbox 拒绝计数逻辑
- `core/src/service/step/` — self_test 空变更检测
- `docs/qa/` — 新增验证场景

## 5. 风险

- 将 `proto/` 加入 writable_paths 扩大了 sandbox 的写入面，但 proto 文件是代码生成的源头，implement 步骤必须能修改。
- sandbox 拒绝检测依赖日志解析，需要确保 EPERM 信息从 agent 子进程传播到 runner。
