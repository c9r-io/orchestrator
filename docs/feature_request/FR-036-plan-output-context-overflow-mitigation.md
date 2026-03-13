# FR-036: Plan Output 上下文溢出缓解

## 状态

Open

## 优先级

P2 — 间接导致 Cycle 2+ implement 失败，是 FR-035 退化循环的触发条件之一

## 背景

### 问题发现

2026-03-13 执行 `follow-logs-callback-execution.md` 测试计划时，Cycle 2+ 的 implement agent 尝试读取 `plan_output.txt`（80K tokens）作为上下文，但该文件超过了 Claude 单次 Read 工具的限制（25K tokens），导致读取失败。

agent 的日志显示它尝试了多种方式读取 plan output：
1. `Read plan_output.txt` → 失败（file too large）
2. `Grep "QA Strategy"` → 匹配行被截断（long matching line omitted）
3. `Read plan_output.txt offset=70 limit=20` → 仍然超过 token 限制

每次尝试失败后 agent 以 exit=-1 退出，触发新 cycle 重试，形成退化循环。

### 根因分析

#### 1. plan_output.txt 包含完整 session transcript

`plan_output.txt` 存储的是 plan agent 的完整 Claude session transcript（JSON lines），包括：
- 所有 thinking blocks
- 所有 tool_use 请求和 tool_result 响应
- 最终的 plan 文本

对于一个执行了 25 个 turns 的 plan session，输出轻松超过 80K tokens。

#### 2. 下游 step 依赖 plan_output 获取上下文

implement step 的 prompt template 引用 `{plan_output_path}` 变量，指向 `plan_output.txt`。agent 被指示 "读取 plan 文件了解实现方案"，但文件太大无法读取。

#### 3. 无摘要/精简机制

从 plan agent 的输出到 plan_output.txt 之间没有任何处理。完整的 session transcript 原样写入文件。

## 需求

### 方案 A：plan_output 自动摘要（推荐）

在 plan step 完成后，从 session transcript 中提取最终 assistant 输出（plan 正文），写入独立的 `plan_summary.txt`：

```
处理流程：
  plan agent session → plan_output.txt (完整 transcript, 保留用于审计)
                     → plan_summary.txt (仅最终 plan 文本, 供下游使用)
```

实现方式：
1. 在 `phase_output_published` 事件处理中，解析 plan_output.txt
2. 提取最后一条 `type=result` 消息的 `result` 字段
3. 写入 `plan_summary.txt`
4. 修改 implement step template 的 `{plan_output_path}` 指向 `plan_summary.txt`

### 方案 B：Pipeline 变量直接传递 plan 文本

利用现有的 pipeline variable 机制（类似 `qa_doc_gen_output`），将 plan 文本直接存储为 pipeline variable：

```yaml
# step template
- name: plan
  post_actions:
    - type: SetPipelineVar
      key: plan_text
      from: result  # 从 agent output 的 result 字段提取
```

下游 step 直接通过 `{plan_text}` 引用，无需读取文件。

### 方案 C：分段 plan_output 索引

为大型 plan_output.txt 生成目录索引文件 `plan_output_index.txt`：

```
## Sections
- Line 1-70: Session initialization and tool calls
- Line 71-77: Plan text (main content)
- Line 78: Final result summary
```

下游 agent 先读取索引，再用 `offset/limit` 定位读取所需部分。

## 验收标准

1. implement agent 能够在不读取完整 plan_output.txt 的情况下获取 plan 内容
2. plan 正文内容完整传递，不丢失关键信息（文件列表、变更说明、scope boundary）
3. 完整的 plan session transcript 仍保留用于审计和调试
4. 对于 < 25K token 的 plan output，行为不变（向后兼容）

## 关联

- 发现于：`follow-logs-callback-execution.md` 测试计划，Cycle 3+ implement 失败
- 根因关系：plan_output 不可读 → implement exit=-1 → 触发 FR-035 退化循环
- 相关文件：`core/src/scheduler/engine.rs`（phase_output_published 处理）
- 相关 step template：`docs/workflow/self-bootstrap.yaml` 的 implement step
