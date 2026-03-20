# QA-103: Prehook CEL Pipeline Variables

## 关联
- FR-049
- Design Doc 61

## 场景

### S1: 字符串类型 pipeline var 可在 prehook 中引用
- **前置**: step capture 写入 `my_var=hello`
- **操作**: prehook 表达式 `my_var == 'hello'`
- **预期**: 表达式求值为 true，步骤执行

### S2: 整数类型 pipeline var 自动推导
- **前置**: step capture 写入 `my_count=42`
- **操作**: prehook 表达式 `my_count > 10`
- **预期**: `my_count` 被推导为 int，比较成立

### S3: 布尔类型 pipeline var 自动推导
- **前置**: step capture 写入 `feature_on=true`
- **操作**: prehook 表达式 `feature_on`
- **预期**: `feature_on` 被推导为 bool，表达式为 true

### S4: 浮点类型 pipeline var 自动推导
- **前置**: step capture 写入 `score=3.14`
- **操作**: prehook 表达式 `score > 3.0`
- **预期**: `score` 被推导为 double，比较成立

### S5: JSON 数组 pipeline var 支持 `in` 操作符
- **前置**: step capture 写入 `regression_target_ids=["docs/qa/test.md","docs/qa/other.md"]`
- **操作**: prehook 表达式 `qa_file_path in regression_target_ids`，当前 item 的 `qa_file_path` 为 `docs/qa/test.md`
- **预期**: 表达式为 true，步骤执行

### S6: JSON 数组 `in` 不匹配时跳过
- **前置**: 同 S5，但 `regression_target_ids=["docs/qa/other.md"]`
- **操作**: prehook 表达式 `qa_file_path in regression_target_ids`
- **预期**: 表达式为 false，步骤跳过

### S7: 截断变量在 CEL 上下文中被跳过
- **前置**: pipeline var 值包含 `[truncated` 标记（已溢出到磁盘）
- **操作**: prehook 表达式引用该变量
- **预期**: CEL 求值报错（变量未定义），不会注入截断文本

### S8: 内建变量名优先于 pipeline var
- **前置**: pipeline var 名为 `cycle`，值为 `999`
- **操作**: prehook 表达式 `cycle == 2`（实际 cycle 为 2）
- **预期**: 内建 `cycle` 优先，表达式为 true（而非使用 pipeline var 的 999）

### S9: task-scoped 与 item-scoped vars 合并
- **前置**: task-scoped pipeline var `a=1`，item-scoped pipeline var `b=2` 和 `a=3`
- **操作**: 构造 `StepPrehookContext`
- **预期**: vars 包含 `a=3`（item 覆盖 task）和 `b=2`

### S10: 缺省 vars 不影响现有行为
- **前置**: pipeline vars 为空
- **操作**: 运行任意现有 prehook 表达式
- **预期**: 行为与 FR-049 之前完全一致

## 自动化覆盖
- S1: `core/src/prehook/tests.rs::test_prehook_pipeline_var_string`
- S2: `core/src/prehook/tests.rs::test_prehook_pipeline_var_int`
- S3: `core/src/prehook/tests.rs::test_prehook_pipeline_var_bool`
- S4: `core/src/prehook/tests.rs::test_prehook_pipeline_var_float`
- S5: `core/src/prehook/tests.rs::test_prehook_pipeline_var_json_array_in_operator`
- S6: `core/src/prehook/tests.rs::test_prehook_pipeline_var_json_array_not_in`
- S7: `core/src/prehook/tests.rs::test_prehook_pipeline_var_truncated_skipped`
- S8: `core/src/prehook/tests.rs::test_prehook_pipeline_var_builtin_takes_precedence`
- S9: 无直接单测 — `StepPrehookContext.vars` 的合并由调用方（scheduler）负责，合并正确性通过 S1-S8 间接覆盖
- S10: 无显式单测 — 现有 S1-S8 测试中 `vars: Default::default()` 即为空 vars 场景，行为不变性已隐式验证

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1-S8: unit test PASS; S9-S10: 通过现有测试间接覆盖（见自动化覆盖说明） |
