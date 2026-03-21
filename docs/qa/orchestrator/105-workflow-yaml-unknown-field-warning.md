---
self_referential_safe: true
---

# QA-105: Workflow YAML 步骤定义未知字段警告

## 关联
- FR-051
- Design Doc 63

## 场景

### S1: capture 写在 step 层级时收到 warning

**目标**: 验证 `capture` 作为 step 层级未知字段时，`did_you_mean` 提供建议 `behavior.captures`。

**步骤**:
1. **Code review** — 确认 `did_you_mean()` 在 `core/src/config_load/validate/workflow_steps.rs` 中为 `"capture"` 返回建议：
   ```bash
   rg -n "capture.*behavior.captures" core/src/config_load/validate/workflow_steps.rs
   ```
2. **Unit test** — 运行检测带建议的未知字段测试：
   ```bash
   cargo test -p agent-orchestrator -- validate::workflow_steps::tests::unknown_field_detected_with_suggestion --nocapture
   ```

**预期**:
- `did_you_mean("capture")` 返回 `Some("behavior.captures")`
- 测试验证 warning 消息包含 `"did you mean"`

### S2: 未知字段无建议时仅报字段名

**步骤**:
1. **Unit test** — 运行检测无建议的未知字段测试：
   ```bash
   cargo test -p agent-orchestrator -- validate::workflow_steps::tests::unknown_field_detected_without_suggestion --nocapture
   ```

**预期**:
- 对于不在建议映射中的字段（如 `"foobar"`），warning 不含 "did you mean"
- 仅报告字段名

### S3: 正确 YAML 无 warning

**步骤**:
1. **Code review** — 确认 `validate_workflow_steps` 对合法字段不发出 warning：
   ```bash
   rg -n "fn validate_workflow_steps" core/src/config_load/validate/workflow_steps.rs
   ```
2. **Unit test** — 运行 YAML round-trip 测试确认合法配置无 warning：
   ```bash
   cargo test -p agent-orchestrator -- validate::workflow_steps::tests::yaml_round_trip_captures_unknown_fields --nocapture
   ```

**预期**:
- 合法 step 定义（capture 在 `behavior.captures` 下）不触发 warning

### S4: prehook 引用未声明 capture 变量时收到 warning

**步骤**:
1. **Unit test** — 运行 prehook 未声明变量 warning 测试：
   ```bash
   cargo test -p agent-orchestrator -- validate::workflow_steps::tests::prehook_warns_on_uncaptured_variable --nocapture
   ```

**预期**:
- step B 的 prehook 引用了未被任何前序 step 声明的 capture 变量时，产生 warning

### S5: prehook 引用已声明 capture 变量时无 warning

**步骤**:
1. **Unit test** — 运行 prehook 已声明变量测试：
   ```bash
   cargo test -p agent-orchestrator -- validate::workflow_steps::tests::prehook_no_warning_when_variable_captured --nocapture
   ```

**预期**:
- step A 声明了 `behavior.captures[].var: regression_target_ids`，step B prehook 引用该变量时无 warning

### S6: warning 不影响退出码

**步骤**:
1. **Code review** — 确认 `validate_workflow_steps` 仅 push warning 到 `notices` 集合，不返回 `Err`：
   ```bash
   rg -n "notices.push|warnings.push" core/src/config_load/validate/workflow_steps.rs
   ```
2. **Unit test** — 运行全部 workflow_steps 验证测试确认无 panic：
   ```bash
   cargo test -p agent-orchestrator -- validate::workflow_steps::tests --nocapture
   ```

**预期**:
- Warning 仅追加到 notice 列表，不影响 apply 成功或退出码

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S5 false positive warnings fixed — added `len` to CEL keywords, `steps` to builtin vars, and prior step IDs to allowed identifiers |
| 2 | S1: capture suggestion | ☑ | `did_you_mean("capture")` → `"behavior.captures"` confirmed; test passed |
| 3 | S2: no suggestion | ☑ | Unknown field without suggestion only reports field name; test passed |
| 4 | S3: valid YAML | ☑ | yaml_round-trip test passes; no spurious warnings |
| 5 | S4: prehook uncaptured | ☑ | Warning generated for undeclared capture variable; test passed |
| 6 | S5: prehook captured | ☑ | No warning when variable previously declared; test passed |
| 7 | S6: no exit code impact | ☑ | warnings.push on lines 226/231/257; 9 workflow_steps tests pass |
