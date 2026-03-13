# FR-031: generate_items 对 LLM 非标准 JSON 输出的容错解析

## 状态

Proposed

## 优先级

P1 — 直接影响 self-bootstrap 工作流的 QA 约束执行正确性

## 背景

### 问题发现

2026-03-13 执行 `follow-logs-callback-execution.md` 测试计划时发现：Cycle 1 的 `qa_doc_gen` 步骤正确识别了 5 个回归测试目标，但 Cycle 2 的 `qa_testing` 步骤对全部 102 个 item 执行了全量测试，而非仅测试 `qa_doc_gen` 约束的 5 个目标。

### 根因链

1. `qa_doc_gen` agent (architect) 输出了 **非标准 JSON**：
   ```
   {regression_targets: [{id: docs/qa/orchestrator/02-cli-task-lifecycle.md, name: docs/qa/orchestrator/02-cli-task-lifecycle.md}, ...]}
   ```
   key 和 string value 均未加引号（类 YAML 格式），而非合法 JSON：
   ```json
   {"regression_targets": [{"id": "docs/qa/orchestrator/02-cli-task-lifecycle.md", "name": "docs/qa/orchestrator/02-cli-task-lifecycle.md"}, ...]}
   ```

2. `flush_pending_generate_items` 调用 `extract_dynamic_items`，内部走 `extract_json_array`，无法在文本中找到合法 JSON 匹配 JSONPath `$.regression_targets`，返回错误。

3. 错误被 `flush_pending_generate_items` 静默降级为 WARN 日志：
   ```
   WARN extract_json_array failed: no valid JSON containing path '$.regression_targets' found in text
     from_var=qa_doc_gen_output  json_path=$.regression_targets  content_len=622
   WARN failed to extract dynamic items
   ```

4. `generate_items` 失败后，原有 102 个 static items 保留不变。Cycle 2 的 `qa_testing` prehook（`is_last_cycle && qa_file_path.startsWith("docs/qa/")...`）对所有 item 放行，变成全量测试。

### 证据

- **daemon 日志**：`/tmp/orchestratord.log` 中 `03:43:56.001103Z` 记录了 extraction failure
- **DB 事件**：`events` 表中无 `items_generated` 事件（正常应有 `{"count": 5, "replace": true}`）
- **DB pipeline vars**：`qa_doc_gen_output` 值已正确持久化，内容为 622 字节非标准 JSON
- **Task items**：全部 102 个 static items 保留，无 dynamic items 产生

### 影响范围

- 任何使用 `generate_items` post-action 消费 LLM 结构化输出的工作流都会受到影响
- `self-bootstrap` 和 `self-evolution` 工作流均依赖 `generate_items` 进行动态 item 筛选
- 当前 `extract_json_array`（`core/src/scheduler/item_generate.rs`）已有对 mixed-text LLM 输出的容错（FR 由 commit `32f2814` 引入），但仅处理 JSON 前后包裹文本的情况，未处理 JSON 本身 key/value unquoted 的情况

## 需求

### 核心需求

增强 `extract_json_array` 使其能解析 LLM 常见的非标准 JSON 输出格式，至少覆盖以下变体：

1. **Unquoted keys**: `{regression_targets: [...]}` → `{"regression_targets": [...]}`
2. **Unquoted string values**: `{id: docs/qa/foo.md}` → `{"id": "docs/qa/foo.md"}`
3. **混合引号**: 部分 key 有引号、部分无引号的混合情况

### 辅助需求

1. **失败可见性**：当 `generate_items` extraction 失败时，除 WARN 日志外，应 emit 一个 `items_generation_failed` 事件到 events 表，payload 包含 `from_var`、`json_path`、`error` 和 `content_preview`，便于 `task trace` 和监控捕获
2. **Fallback 语义明确化**：当前 `generate_items` 失败是静默 fallback（保留 static items）。应在事件中明确标记 `fallback: "static_items_retained"` 以便审计

### 非目标

- 不要求 agent prompt 改为强制输出 strict JSON（LLM 输出格式不可控）
- 不变更 `generate_items` 的 YAML 配置格式或 `json_path` 语法
- 不处理完全无结构的自然语言输出（仍需输出结构至少可辨认为 object/array）

## 涉及文件

| 文件 | 变更类型 |
|------|---------|
| `core/src/scheduler/item_generate.rs` | `extract_json_array` 增加 unquoted JSON 修复逻辑 |
| `core/src/scheduler/loop_engine/segment.rs` | `flush_pending_generate_items` 失败时 emit 事件 |
| `core/src/scheduler/item_generate.rs` (tests) | 新增 unquoted JSON 测试用例 |

## 验收标准

1. `extract_json_array` 对以下输入均能正确提取：
   - `{regression_targets: [{id: a.md, name: a.md}]}` + JSONPath `$.regression_targets` → 提取出 1 个 item
   - `{"regression_targets": [{"id": "a.md"}]}` (标准 JSON) → 继续正常工作
   - `Some text before {regression_targets: [...]} and after` (混合文本 + unquoted) → 提取成功
2. `flush_pending_generate_items` 失败时 `events` 表出现 `items_generation_failed` 事件
3. `cargo test --workspace --lib` 通过
4. 回归验证：重跑 `follow-logs-callback-execution.md` 计划时，Cycle 2 的 `qa_testing` 仅测试 `qa_doc_gen` 约束的 regression targets

## 复现步骤

```bash
# 可在单元测试中直接复现 extraction 失败
cargo test -p agent-orchestrator extract_json_array -- --nocapture

# 完整工作流复现：执行 self-bootstrap 计划并观察 daemon 日志
grep "extract_json_array failed" /tmp/orchestratord.log
```

## 参考

- 测试计划：`docs/plan/follow-logs-callback-execution.md`
- `extract_json_array` 实现：`core/src/scheduler/item_generate.rs:60-109`
- `flush_pending_generate_items`：`core/src/scheduler/loop_engine/segment.rs:579-653`
- 前序修复 commit：`32f2814` (Fix extract_json_array to handle mixed-text LLM output)
