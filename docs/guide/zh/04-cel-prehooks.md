# 04 - CEL 预钩子

预钩子（Prehook）是工作流步骤上的条件门控。在步骤运行前，其预钩子 CEL 表达式被求值；如果返回 `false`，该步骤在当前循环或项中被跳过。

## 预钩子语法

```yaml
- id: qa_testing
  prehook:
    engine: cel                # 仅支持 "cel"
    when: "is_last_cycle"      # CEL 表达式 —— 必须求值为布尔值
    reason: "QA 延迟到最后一个循环"   # 人类可读的说明（可选）
```

当 `when` 求值为 `true` 时步骤运行。求值为 `false` 时步骤被跳过，`reason` 被记录到日志。

## 可用变量（预钩子上下文）

以下变量可在预钩子 `when` 表达式中使用：

### 循环与任务状态

| 变量 | 类型 | 说明 |
|------|------|------|
| `cycle` | `int` | 当前循环轮次（从 1 开始） |
| `max_cycles` | `int` | 配置的总循环数 |
| `is_last_cycle` | `bool` | 当 `cycle == max_cycles` 时为 `true` |
| `task_id` | `string` | 当前任务 ID |
| `task_item_id` | `string` | 当前项 ID（task 作用域步骤为空） |
| `task_status` | `string` | 当前任务状态 |
| `item_status` | `string` | 当前项状态 |
| `step` | `string` | 当前步骤 ID |

### QA 与工单状态

| 变量 | 类型 | 说明 |
|------|------|------|
| `qa_file_path` | `string` | 此项的 QA 文件路径 |
| `qa_exit_code` | `int?` | 上次 QA 步骤的退出码（未运行则为 `null`） |
| `qa_failed` | `bool` | 上次 QA 步骤是否失败 |
| `active_ticket_count` | `int` | 活动（未解决）工单数量 |
| `new_ticket_count` | `int` | 当前循环中创建的工单数量 |

### 修复与重测状态

| 变量 | 类型 | 说明 |
|------|------|------|
| `fix_exit_code` | `int?` | 上次 fix 步骤的退出码 |
| `fix_required` | `bool` | 是否需要修复 |
| `retest_exit_code` | `int?` | 上次 retest 步骤的退出码 |

### 构建与测试状态

| 变量 | 类型 | 说明 |
|------|------|------|
| `build_exit_code` | `int?` | 上次 build 步骤的退出码 |
| `test_exit_code` | `int?` | 上次 test 步骤的退出码 |
| `build_errors` | `int` | 构建错误数 |
| `test_failures` | `int` | 测试失败数 |
| `self_test_exit_code` | `int?` | 上次 self_test 步骤的退出码 |
| `self_test_passed` | `bool` | 上次 self_test 是否通过 |

### 安全

| 变量 | 类型 | 说明 |
|------|------|------|
| `self_referential_safe` | `bool` | 此项是否可安全用于自引用执行 |

## 常见模式

### 延迟到最后一个循环

在多循环工作流中仅在最后一个循环运行 QA：

```yaml
prehook:
  engine: cel
  when: "is_last_cycle"
  reason: "QA 延迟到最后一个循环"
```

### 条件修复

仅在有活动工单时运行修复：

```yaml
prehook:
  engine: cel
  when: "active_ticket_count > 0"
  reason: "没有需要修复的工单"
```

### 组合条件

将 QA 延迟到最后一个循环，并按安全文件过滤：

```yaml
prehook:
  engine: cel
  when: >-
    is_last_cycle
    && self_referential_safe
    && qa_file_path.startsWith("docs/qa/")
    && qa_file_path.endsWith(".md")
  reason: "QA 测试延迟到最后一个循环；跳过不安全的文档"
```

### 基于置信度的门控

如果 QA 置信度足够高则跳过修复：

```yaml
prehook:
  engine: cel
  when: "qa_confidence != null && qa_confidence < 0.8"
  reason: "QA 置信度高于阈值 —— 无需修复"
```

### 构建失败门控

仅在构建成功后运行部署：

```yaml
prehook:
  engine: cel
  when: "build_exit_code != null && build_exit_code == 0"
  reason: "部署前构建必须通过"
```

## CEL 表达式快速参考

CEL（Common Expression Language）支持标准操作：

```cel
# 比较
cycle > 1
active_ticket_count == 0

# 逻辑运算符
is_last_cycle && qa_failed
fix_required || active_ticket_count > 0

# 空值检查（对可选值很重要）
qa_exit_code != null && qa_exit_code == 0

# 字符串操作
qa_file_path.startsWith("docs/qa/")
qa_file_path.endsWith(".md")
step == "qa_testing"

# 取反
!qa_failed
!(is_last_cycle && fix_required)
```

**重要**：可选整数变量（`qa_exit_code`、`fix_exit_code` 等）可能为 `null`。比较前务必进行空值检查：

```cel
# 错误 —— 如果 qa_exit_code 为 null 会出错
qa_exit_code == 0

# 正确
qa_exit_code != null && qa_exit_code == 0
```

## 终结规则（CEL 上下文）

终结规则使用相同的 CEL 引擎，但具有扩展的变量集。除了上述预钩子变量外，终结规则还可以访问：

| 变量 | 类型 | 说明 |
|------|------|------|
| `retest_new_ticket_count` | `int` | 重测期间创建的工单数 |
| `qa_configured` | `bool` | 工作流中存在 QA 步骤 |
| `qa_observed` | `bool` | 此循环中观察到了 QA 步骤 |
| `qa_enabled` | `bool` | QA 步骤已启用 |
| `qa_ran` | `bool` | QA 步骤实际执行了 |
| `qa_skipped` | `bool` | QA 步骤被跳过（预钩子返回 false） |
| `fix_configured` | `bool` | 工作流中存在 Fix 步骤 |
| `fix_enabled` | `bool` | Fix 步骤已启用 |
| `fix_ran` | `bool` | Fix 步骤已执行 |
| `fix_skipped` | `bool` | Fix 步骤被跳过 |
| `fix_success` | `bool` | Fix 成功完成 |
| `retest_enabled` | `bool` | Retest 步骤已启用 |
| `retest_ran` | `bool` | Retest 已执行 |
| `retest_success` | `bool` | Retest 通过 |
| `is_last_cycle` | `bool` | 是否为最后一个循环 |

### 默认终结规则

如果未指定自定义终结规则，引擎按以下顺序应用 12 条内置规则（第一条匹配的生效）：

| # | 规则 ID | 条件（简化） | 状态 |
|---|---------|-------------|------|
| 1 | `skip_without_tickets` | `qa_skipped && active_ticket_count == 0 && is_last_cycle` | skipped |
| 2 | `qa_passed_without_tickets` | `qa_ran && qa_exit_code == 0 && active_ticket_count == 0` | qa_passed |
| 3 | `fix_disabled_with_tickets` | `!fix_enabled && active_ticket_count > 0` | unresolved |
| 4 | `fix_failed` | `fix_ran && !fix_success` | unresolved |
| 5 | `fixed_without_retest` | `fix_success && !retest_enabled` | fixed |
| 6 | `fix_skipped_and_retest_disabled` | `fix_enabled && !fix_ran && !retest_enabled && active_ticket_count > 0` | unresolved |
| 7 | `fixed_retest_skipped_after_fix_success` | `retest_enabled && !retest_ran && fix_success` | fixed |
| 8 | `unresolved_retest_skipped_without_fix` | `retest_enabled && !retest_ran && !fix_success && active_ticket_count > 0` | unresolved |
| 9 | `verified_after_retest` | `retest_ran && retest_success && retest_new_ticket_count == 0` | verified |
| 10 | `unresolved_after_retest` | `retest_ran && (!retest_success \|\| retest_new_ticket_count > 0)` | unresolved |
| 11 | `fallback_unresolved_with_tickets` | `active_ticket_count > 0` | unresolved |
| 12 | `fallback_qa_passed` | `active_ticket_count == 0` | qa_passed |

最后两条规则是兜底回退。工作流中 `finalize.rules` 的自定义规则会完全替换这些默认规则。

### 自定义终结规则示例

```yaml
finalize:
  rules:
    # QA 干净通过
    - id: qa_clean_pass
      engine: cel
      when: "qa_ran && active_ticket_count == 0"
      status: qa_passed
      reason: "QA 通过，无活动工单"

    # 修复经重测验证
    - id: fix_verified
      engine: cel
      when: "fix_ran && retest_ran && retest_success"
      status: fix_verified
      reason: "修复已应用并验证通过"

    # 非最终循环中 QA 被跳过 —— 保持 pending
    - id: qa_deferred
      engine: cel
      when: "qa_skipped && !is_last_cycle"
      status: pending
      reason: "QA 延迟到下一个循环"

    # 兜底
    - id: fallback
      engine: cel
      when: "true"
      status: pending
      reason: "无规则匹配 —— 保持 pending"
```

## 下一步

- [05 - 高级特性](05-advanced-features.md) —— CRD、持久化存储、任务派生
- [03 - 工作流配置](03-workflow-configuration.md) —— 步骤定义和循环策略
