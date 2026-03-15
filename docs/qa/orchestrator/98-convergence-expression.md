# QA-98: convergence_expr 收敛条件表达式

## 关联
- FR-043
- Design Doc 55

## 场景

### S1: convergence_expr 驱动收敛终止
- **前置**: workflow YAML 配置 `convergence_expr: [{when: "cycle >= 2", reason: "test"}]`，mode=infinite，max_cycles=10
- **操作**: 启动 task 并运行
- **预期**: task 在 cycle 2 后 completed，`loop_guard_decision` 事件包含 reason="test"

### S2: 缺省 convergence_expr 行为不变
- **前置**: workflow YAML 无 convergence_expr 字段
- **操作**: 启动 task 并运行
- **预期**: 行为与 FR-043 之前完全一致（fixed mode 按 max_cycles 停止，infinite mode 按 stop_when_no_unresolved 停止）

### S3: 多条 convergence_expr 短路求值
- **前置**: 配置两条 convergence_expr，第一条 `cycle >= 2`，第二条 `cycle >= 5`
- **操作**: 运行到 cycle 2
- **预期**: 第一条匹配即停止，reason 为第一条的 reason

### S4: CEL 编译校验
- **前置**: convergence_expr.when 包含无效 CEL 语法
- **操作**: 加载 manifest
- **预期**: daemon 拒绝加载并返回包含 "invalid CEL" 的错误信息

### S5: pipeline 变量注入
- **前置**: step captures 写入 `delta_lines=3`，convergence_expr: `delta_lines < 5 && cycle >= 2`
- **操作**: 运行 task
- **预期**: cycle 2 时 delta_lines=3 < 5 成立，task 收敛终止

### S6: max_cycles 仍为硬上限
- **前置**: convergence_expr 永远不为 true，max_cycles=3
- **操作**: 运行 task
- **预期**: cycle 3 后因 max_cycles 停止，非因 convergence_expr

## 自动化覆盖
- S1: `crates/integration-tests/tests/workflow_loop.rs::convergence_expr_stops_loop`
- S2: `crates/integration-tests/tests/workflow_loop.rs::multi_cycle_loop`（回归）
- S4: `core/src/config_load/validate/loop_policy.rs` 单元测试

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
