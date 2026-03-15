---
self_referential_safe: false
---
# QA: Daemon Crash Resilience — Graceful Shutdown & Regression (FR-032)

**Split from**: `docs/qa/orchestrator/91-daemon-crash-resilience.md`

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: 优雅关闭不受影响（代码审查）

**步骤:**
1. 检查 daemon shutdown 序列（SIGTERM/SIGINT 处理）

**预期结果:**
- [ ] `request_shutdown()` 将 lifecycle 切换为 `Draining`
- [ ] shutdown_tx 通知所有 worker 退出循环
- [ ] 5s grace period 等待运行中任务完成
- [ ] supervisor 在 30s timeout 内等待所有 worker handle join
- [ ] 正常关闭不触发 `worker_panic_recovered` 或 `daemon_crash_recovered` 事件

## 场景 2: 全量单元测试通过

**步骤:**
1. 运行 `cd {source_tree} && cargo test --workspace --lib`

**预期结果:**
- [ ] 所有测试通过，无 failure
- [ ] 无新增编译警告

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
