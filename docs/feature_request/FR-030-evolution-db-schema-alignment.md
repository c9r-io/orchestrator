# FR-030: Self-Evolution 数据库 Schema 对齐验证

**优先级**: P1
**状态**: Proposed
**目标**: 确认并补全 self-evolution workflow 运行所需的数据库表和列，确保监控查询与运行时操作可正常工作

## 背景与目标

self-evolution 测试计划（`docs/plan/self-evolution-execution.md`）中定义了多项 SQL 监控查询用于观察进化过程的关键事件，同时运行时代码依赖特定的表结构来存储动态 item 和 workflow store 数据。

当前需要验证以下数据库结构是否与代码实现和监控查询对齐：

1. **`task_items` 表**：动态 item 生成（`create_dynamic_task_items`）依赖 `label`、`source`、`dynamic_vars_json` 列。这些列是后期增加的非初始 schema 字段，需确认 migration 已覆盖。

2. **`workflow_store_entries` 表**：`item_select` builtin 步骤通过 `store_result` 配置将 winner 数据写入此表（namespace: `evolution`, key: `winner_latest`），`evo_apply_winner` 通过 `store_inputs` 读取。需确认表存在且 schema 匹配。

3. **`events` 表**：监控查询使用 `event_type='items_generated'` 过滤动态 item 生成事件。需确认 `items_generated` 事件类型在代码中正确 emit。

4. **监控 SQL 查询正确性**：测试计划 4.2 节的 sqlite3 查询需要与实际表结构匹配。

目标：

- 逐一验证上述表/列/事件是否在当前 migration 和代码中存在。
- 对缺失项补充 migration 或代码修正。
- 确保测试计划中的所有 sqlite3 监控查询可执行。

非目标：

- 不重新设计数据库 schema（仅补齐缺失项）。
- 不修改监控查询的逻辑（仅确保其可执行）。

## 检查清单

### 1. task_items 表

需验证以下列存在：

| 列名 | 类型 | 用途 | 引用代码 |
|------|------|------|---------|
| `id` | TEXT PRIMARY KEY | 动态 item UUID | `item_generate.rs:create_dynamic_task_items` |
| `task_id` | TEXT | 所属 task | 同上 |
| `order_no` | INTEGER | 排序 | 同上 |
| `qa_file_path` | TEXT | 存储 item_id（复用字段） | 同上，`item.item_id` 写入此列 |
| `label` | TEXT | 候选方案名称 | 同上，`item.label` |
| `source` | TEXT | 来源标记（`dynamic` / `static`） | 同上，硬编码 `'dynamic'` |
| `dynamic_vars_json` | TEXT | 每个 item 的变量 JSON | 同上，`item.vars` 序列化 |
| `status` | TEXT | 状态 | 同上，初始 `'pending'` |

**关键确认**：`label`、`source`、`dynamic_vars_json` 三列是否在 migration 中定义。

### 2. workflow_store_entries 表

需验证表存在，且包含以下列：

| 列名 | 类型 | 用途 |
|------|------|------|
| `store_name` / `namespace` | TEXT | store 命名空间（如 `evolution`） |
| `key` | TEXT | entry key（如 `winner_latest`） |
| `value_json` | TEXT | 存储的 JSON 值 |
| `task_id` | TEXT | 所属 task |

**关键确认**：实际列名与监控查询 `SELECT value_json FROM workflow_store_entries WHERE store_name='evolution' AND key='winner_latest'` 匹配。

### 3. events 表

需验证：
- 表存在且包含 `task_id`、`event_type`、`payload_json` 列。
- `items_generated` 事件类型在 `generate_items` post_action 执行后正确 emit。

### 4. 监控查询验证

测试计划中的三组 sqlite3 查询：

```sql
-- 查询 1：items_generated 事件
SELECT payload_json FROM events WHERE task_id='<task_id>' AND event_type='items_generated';

-- 查询 2：动态 item 状态
SELECT id, label, source, status FROM task_items WHERE task_id='<task_id>';

-- 查询 3：选择结果
SELECT value_json FROM workflow_store_entries WHERE store_name='evolution' AND key='winner_latest';
```

需确认每个查询的表名、列名与实际 schema 一致。

## 实施方案

### 第一步：Schema 审计

- 检查 `core/src/db/` 或 migration 文件中 `task_items`、`workflow_store_entries`、`events` 表的 CREATE TABLE 语句。
- 比对代码中 INSERT/SELECT 语句使用的列名。
- 标注缺失项。

### 第二步：补齐 Migration（如有缺失）

- 对缺失列增加 ALTER TABLE migration。
- 对缺失表增加 CREATE TABLE migration。
- 确保 `orchestrator init` 能正确执行所有 migration。

### 第三步：验证监控查询

- 在 `orchestrator init` 后执行测试计划的三组 sqlite3 查询，确认无 SQL 错误。
- 对返回空结果是预期的（尚无任务数据），只需确认表和列存在。

### 第四步：端到端冒烟测试

- 使用 mock agent（echo 固定 JSON）创建一个 self-evolution task。
- 验证 `task_items` 中出现 `source='dynamic'` 的记录。
- 验证 `workflow_store_entries` 中出现 `evolution/winner_latest` 记录。
- 验证 `events` 中出现 `items_generated` 事件。

## CLI / API 影响

无。本 FR 为内部 schema 对齐，不涉及用户可见接口变更。

## 风险与缓解

风险：补齐 migration 导致已有数据库 schema 不兼容。
缓解：新增 migration 使用 `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`（SQLite 3.35+ 支持）或先检查列是否存在。

风险：测试计划中的查询使用了错误的表名/列名。
缓解：本 FR 的核心目标就是对齐这些查询，确认后更新测试计划文档。

## 验收标准

- `orchestrator init` 后，`task_items` 表包含 `label`、`source`、`dynamic_vars_json` 列。
- `orchestrator init` 后，`workflow_store_entries` 表存在且 schema 与代码中的读写操作匹配。
- `orchestrator init` 后，`events` 表存在且包含 `task_id`、`event_type`、`payload_json` 列。
- 测试计划 4.2 节的三组 sqlite3 查询在空数据库上执行不报 SQL 错误。
- 使用 mock agent 的冒烟测试验证数据流通（动态 item 创建 → store 写入 → 事件 emit）。
- `cargo test --workspace` 通过。
