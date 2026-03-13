# Design Doc 46: generate_items 对 LLM 非标准 JSON 输出的容错解析 (FR-031)

## Problem

LLM agent（如 Claude architect mode）在输出结构化 JSON 时，经常省略 key 和 string value
的引号，产生类似 YAML 的非标准 JSON：

```
{regression_targets: [{id: docs/qa/foo.md, name: docs/qa/foo.md}]}
```

`extract_json_array` 依赖 `serde_json::from_str` 解析 JSON，对此类输入直接报错。
当 `generate_items` post-action 消费此输出失败时，静默 fallback 为保留全部 static items，
导致下游 step（如 `qa_testing`）对全量 item 执行，而非仅处理 `generate_items` 约束的子集。

## Design Decision

**在 `json_extract.rs` 中新增 `repair_unquoted_json()` 修复函数，集成到 `extract_json_array` 的解析流水线。**

### 修复策略

`repair_unquoted_json` 是一个状态机解析器，逐字符扫描输入：

1. 维护 context stack（Object / Array）和 expecting 状态（Key / Value / ArrayElement）
2. 遇到 bare token 时根据 expecting 状态决定是否加引号：
   - **Key**：累积 `[a-zA-Z0-9_-]` 字符，两端加双引号
   - **Value/ArrayElement**：累积到 `, } ]` 为止，判断是否为 number/bool/null，否则加引号
3. 已引号的 string 原样透传（包括转义序列）
4. 顶层结构闭合后立即停止，保留尾部文本不被破坏

### 集成点

`extract_json_array` 的解析顺序：
1. 整串 `serde_json::from_str` — 标准 JSON 快速路径
2. Fenced code block 提取 — markdown 包裹
3. **`repair_unquoted_json` 修复后重新解析** — 处理全文 unquoted
4. 逐 `{` / `[` 扫描 + 标准解析 — mixed text
5. 逐 `{` / `[` 扫描 + repair 后解析 — mixed text + unquoted

### 失败可见性

`flush_pending_generate_items` 在 extraction 失败时 emit `items_generation_failed` 事件，
payload 包含 `from_var`、`json_path`、`error`、`fallback: "static_items_retained"`，
便于 `task trace` 和监控审计。

### Why this approach

1. **最小侵入** — 修复逻辑封装在独立函数，不改变 `extract_json_array` 的接口契约
2. **渐进降级** — 标准 JSON 走快速路径零开销；仅在标准解析失败后才调用 repair
3. **幂等安全** — 对已合法的 JSON 输入，repair 输出等于输入
4. **覆盖面广** — 同时处理 unquoted keys、unquoted string values、mixed quotes、
   numbers/bools/null 保留、file path 中的 `/` 和 `.`

### Alternatives considered

- **要求 agent prompt 强制输出 strict JSON**：rejected — LLM 输出格式不可控，
  尤其 architect mode 的 `--output-format stream-json` 下 result 字段内容不受 prompt 约束
- **使用 `serde_yaml` 解析**：rejected — unquoted JSON 不是合法 YAML（无换行/缩进），
  且引入新依赖

## Key Files

| File | Role |
|------|------|
| `core/src/json_extract.rs` | `repair_unquoted_json()` 实现 + `extract_json_array` 集成 |
| `core/src/scheduler/loop_engine/segment.rs` | `flush_pending_generate_items` 事件 emit |
| `core/src/scheduler/item_generate.rs` | `extract_dynamic_items` + 集成测试 |

## Test Coverage

- `repair_unquoted_json_keys_and_values` — bare key + bare value
- `repair_unquoted_json_nested_array` — nested object array
- `repair_unquoted_json_preserves_valid` — idempotency on valid JSON
- `repair_unquoted_json_mixed_quoted` — partial quoting
- `repair_unquoted_json_numbers_bools_null` — primitive type preservation
- `repair_unquoted_json_file_paths` — slash/dot in values
- `extract_array_unquoted_regression_targets` — end-to-end 5-item extraction
- `extract_array_mixed_text_unquoted` — mixed text + unquoted JSON
- `test_extract_dynamic_items_unquoted_json` — full pipeline integration
