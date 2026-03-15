# QA-105: Workflow YAML 步骤定义未知字段警告

## 关联
- FR-051
- Design Doc 63

## 场景

### S1: capture 写在 step 层级时收到 warning
- **前置**: YAML step 包含 `capture:` 字段（与 `behavior:` 同级）
- **操作**: `orchestrator apply -f workflow.yaml`
- **预期**: 输出 `Warning: ... contains unknown field 'capture' (did you mean 'behavior.captures'?)`，apply 仍成功

### S2: 未知字段无建议时仅报字段名
- **前置**: YAML step 包含 `foobar:` 未知字段
- **操作**: `orchestrator apply -f workflow.yaml`
- **预期**: 输出 `Warning: ... contains unknown field 'foobar'`，不含 "did you mean"

### S3: 正确 YAML 无 warning
- **前置**: 所有 capture 正确写在 `behavior.captures` 下
- **操作**: `orchestrator apply -f workflow.yaml`
- **预期**: 无任何 Warning 输出

### S4: prehook 引用未声明 capture 变量时收到 warning
- **前置**: step A 无 capture，step B prehook 引用 `regression_target_ids`
- **操作**: `orchestrator apply -f workflow.yaml`
- **预期**: 输出 `Warning: ... step 'B' prehook references 'regression_target_ids' but no prior step captures this variable`

### S5: prehook 引用已声明 capture 变量时无 warning
- **前置**: step A 声明 `behavior.captures[].var: regression_target_ids`，step B prehook 引用该变量
- **操作**: `orchestrator apply -f workflow.yaml`
- **预期**: 无 Warning

### S6: warning 不影响退出码
- **前置**: YAML 包含未知字段，其余配置合法
- **操作**: `orchestrator apply -f workflow.yaml; echo $?`
- **预期**: 退出码为 0（warning 不阻止 apply）

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
