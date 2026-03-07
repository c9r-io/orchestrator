# Self-Evolution Workflow 实测报告

**日期**: 2026-03-08
**任务 ID**: `f91560ab-cc4c-48a8-b140-0671361f73a8`
**任务名称**: `evo-final2`
**最终状态**: completed
**总耗时**: 23 分 53 秒 (15:37:09 → 16:01:02 UTC)

---

## 1. 测试目标

验证 `self-evolution` workflow 能否端到端完成以下完整进化管线：

```
evo_plan → generate_items → evo_implement (×2) → evo_benchmark (×2)
    → select_best → evo_apply_winner → evo_align_tests → self_test → loop_guard
```

课题选用「StepTemplate prompt 变量解析增强」，要求引擎自主：
1. 生成两条竞争方案
2. 各自独立实现
3. 客观评分
4. 自动选出最优方案并落地
5. 通过编译和测试验证

---

## 2. 测试历程

### 2.1 第一次运行 (evo-final, task 1156f8d0)

| 步骤 | 耗时 | 结果 |
|------|------|------|
| evo_plan | 75s | ✓ 生成 2 候选 |
| evo_implement (item-a) | 757s | ✓ |
| evo_benchmark (item-a) | 163s | ✓ |
| evo_implement (item-b) | 794s | ✓ |
| evo_benchmark (item-b) | 170s | ✓ |
| select_best | <1s | ✓ 选出 winner |
| **evo_apply_winner** | - | **FAILED**: `runner command too long (>16384 bytes)` |

**失败原因分析**: `evo_apply_winner` 是 `select_best` 之后的首个 task-scoped agent 步骤。此时 pipeline 已累积 15+ 个变量（goal、evo_plan_output、evo_implement_output、evo_benchmark_output、diff_path、score、strategy、approach 等），每个变量在 spill 机制下最大内联 4096 字节。变量展开后的完整 prompt 超过了 runner 的 16KB 命令长度限制。

**修复**: 将 `runner.rs` 中的命令长度上限从 16,384 字节提升至 131,072 字节 (128KB)。macOS 的 `ARG_MAX` 约为 1MB，且命令通过 `Command::arg()` 传递（操作系统进程参数），不受 shell 行长限制，因此 128KB 是安全的。

**提交**: `f1d1324` — Raise runner command length limit from 16KB to 128KB

### 2.2 第二次运行 (evo-final2, task f91560ab) — 成功

完整端到端通过，详见下方。

---

## 3. 成功运行详细时间线

| 时间 (UTC) | 事件 | 耗时 | Agent | 说明 |
|------------|------|------|-------|------|
| 15:37:09 | task_started | - | - | 手动启动 |
| 15:37:09 | evo_plan started | - | evo_architect | 生成竞争方案 |
| 15:38:45 | evo_plan finished | **96s** | evo_architect | 输出 2 候选 JSON |
| 15:38:45 | items_generated | <1s | (引擎) | 创建 2 dynamic items |
| 15:38:45 | evo_implement (A) started | - | evo_coder | Regex-based parser |
| 15:40:39 | evo_implement (A) finished | **114s** | evo_coder | 编译通过 |
| 15:40:39 | evo_benchmark (A) started | - | evo_coder | 评分 |
| 15:43:23 | evo_benchmark (A) finished | **164s** | evo_coder | score=85 |
| 15:43:23 | evo_implement (B) started | - | evo_coder | Manual tokenizer |
| 15:47:28 | evo_implement (B) finished | **244s** | evo_coder | 编译通过 |
| 15:47:28 | evo_benchmark (B) started | - | evo_coder | 评分 |
| 15:50:30 | evo_benchmark (B) finished | **182s** | evo_coder | score=67 |
| 15:50:30 | select_best | **<1s** | (引擎 builtin) | item_select max score |
| 15:50:30 | evo_apply_winner started | - | evo_coder | 应用胜出方案 |
| 15:55:09 | evo_apply_winner finished | **279s** | evo_coder | 代码落地 |
| 15:55:09 | evo_align_tests started | - | evo_coder | 对齐测试 |
| 15:59:56 | evo_align_tests finished | **288s** | evo_coder | 测试对齐完成 |
| 15:59:56 | self_test started | - | (引擎 builtin) | cargo test |
| 16:00:52 | self_test finished | **56s** | (引擎 builtin) | 24 tests passed |
| 16:01:02 | task_completed | - | - | 正常收口 |

**总 agent 执行时间**: 1,367s (~22.8 min)
**总 overhead 时间**: ~65s (引擎调度、DB 操作、checkpoint)

---

## 4. 候选方案分析

### 4.1 方案 A: Regex-based parser (胜出)

| 维度 | 值 |
|------|-----|
| 名称 | Regex-based parser |
| 策略 | 添加 `regex` crate，使用正则模式分两遍解析默认值和条件段落 |
| 编译 | ✓ |
| 测试 | ✓ (1439 tests pass) |
| Clippy | 3 warnings (unwrap on Regex::new) |
| Diff 行数 | 63 行 |
| **总分** | **85** |

**策略描述**:
1. 第一遍: 正则 `\{\{#if\s+(\w+)\}\}(.*?)\{\{/if\}\}` 处理条件段落
2. 第二遍: 正则 `\{(\w+)\|([^}]+)\}` 处理带默认值的变量
3. 第三遍: 查找剩余 `{...}` 占位符并 `tracing::warn!`

### 4.2 方案 B: Manual tokenizer parser (淘汰)

| 维度 | 值 |
|------|-----|
| 名称 | Manual tokenizer parser |
| 策略 | 在 context.rs 中新建 `TemplateParser` struct，逐字符解析 |
| 编译 | ✓ |
| 测试 | ✓ (1439 tests pass) |
| Clippy | 2 warnings (manual_map) |
| Diff 行数 | 320 行 |
| **总分** | **67** |

**策略描述**:
使用 enum `Token { Plain, Var, VarDefault, ConditionalStart, ConditionalEnd }` 构建 tokenizer，单遍解析模板字符串，直接输出结果。

### 4.3 评分差异分析

方案 A 胜出的主要原因：
- **Diff 精简** (63 vs 320 行): 方案 A 代码量仅为方案 B 的 20%，变更集中且易于审查
- **利用成熟库**: regex crate 是 Rust 生态中经过充分验证的基础库，正确性和性能有保障
- **测试全通过**: 两方案均无测试失败，但 A 的精简实现更不易引入边缘情况
- **Clippy 差异不大**: 3 vs 2 warnings，均为低风险问题

方案 B 的劣势：
- **代码膨胀**: 手写 parser 引入了 320 行新代码，增加了维护负担
- **语法偏移**: 实际使用了 `{var|||default}` (三管道) 和 `{{###if var}}` 语法，偏离了原始需求中的 `{var:-default}` 和 `{?var}...{/var}`
- **超过 200 行阈值**: benchmark agent 明确提到 diff 超过 200 行是减分项

---

## 5. 进化机制评估

### 5.1 候选生成质量

| 维度 | 评价 | 评分 |
|------|------|------|
| 方案多样性 | 两方案有实质差异（正则 vs 手写 parser） | ★★★★☆ |
| 策略可行性 | 两方案均可编译、通过测试 | ★★★★★ |
| 策略描述清晰度 | 策略足够具体，指导 implement agent 执行 | ★★★★☆ |
| 风险评估 | 各方案标注了风险，基本准确 | ★★★☆☆ |

**观察**: `evo_architect` (plan agent) 在 96 秒内生成了两条有实质分化的方案，方案描述包含了具体的实现策略、风险和复杂度估计。分化程度合理。

### 5.2 竞争实现质量

| 维度 | 评价 | 评分 |
|------|------|------|
| 实现与方案一致性 | 方案 A 基本一致；方案 B 语法有偏移 | ★★★☆☆ |
| 代码质量 | 两方案均可编译并通过全部测试 | ★★★★☆ |
| 隔离性 | 两个 item 各自独立实现，无交叉干扰 | ★★★★★ |
| 实现效率 | A=114s, B=244s, B 耗时 2 倍 | ★★★☆☆ |

**观察**: 方案 B 的实现 agent 偏离了原始策略中的语法设计（使用 `|||` 代替 `|`，`{{###if}}` 代替 `{{#if}}`），这是一个需要注意的点——agent 在实现时可能会根据自身判断偏离 plan，后续 benchmark 对此缺乏惩罚。

### 5.3 评分客观性

| 维度 | 评价 | 评分 |
|------|------|------|
| 指标覆盖 | 编译/测试/clippy/diff 大小，覆盖合理 | ★★★★☆ |
| 分数区分度 | 85 vs 67，有明显差异 | ★★★★☆ |
| 评分一致性 | 两次 benchmark 使用相同流程 | ★★★★☆ |
| 客观性 | 基于可验证的自动化指标 | ★★★★☆ |

**观察**: benchmark agent 的评分体系合理——编译通过、测试通过是基础分，clippy clean 是加分项，diff 大小是权重指标。85 vs 67 的分差有足够区分度，选出了更精简的方案。

### 5.4 选择与落地

| 维度 | 评价 | 评分 |
|------|------|------|
| 选择正确性 | 选出分数更高的方案 A | ★★★★★ |
| 应用过程 | evo_apply_winner 成功应用代码 | ★★★★☆ |
| 测试对齐 | evo_align_tests 完成测试调整 | ★★★★☆ |
| 最终验证 | self_test 24 tests passed | ★★★★★ |

---

## 6. 引擎功能验证清单

| 功能 | 状态 | 说明 |
|------|------|------|
| evo_plan → generate_items | ✓ | 从 agent JSON 输出提取 candidates，创建 2 个 dynamic items |
| Dynamic item vars 注入 | ✓ | approach、strategy、item_label 正确注入 pipeline vars |
| Item-scoped 步骤过滤 | ✓ | evo_implement/evo_benchmark 只在 2 个 dynamic items 上执行 |
| max_parallel=1 串行执行 | ✓ | item-a 全部完成后才开始 item-b |
| Pipeline var spill | ✓ | 大输出被 spill 到文件，内联截断到 4KB |
| item_select (max score) | ✓ | 正确选出 score 最高的 item |
| winner_latest store | ✓ | evolution.winner_latest 正确写入 |
| Task-scoped 步骤恢复 | ✓ | select_best 后续步骤正确切回 task scope |
| self_test shell step | ✓ | cargo test 执行并验证 exit_code=0 |
| Checkpoint/restore | ✓ | 各步骤间正确创建 checkpoint |
| Stream-json 解析 | ✓ | agent JSONL 输出正确解析 |
| Redacted JSON 回退 | ✓ | 敏感值被 `[REDACTED]` 替换后仍可提取 result |

---

## 7. 发现的新 Bug 及修复

### Bug #7: Runner command too long

| 项目 | 内容 |
|------|------|
| **现象** | `evo_apply_winner` 步骤启动时报错 `runner command too long (>16384 bytes)` |
| **根因** | task-scoped 步骤在 select_best 后已累积 15+ 个 pipeline vars，每个最大内联 4KB，总 prompt 超过 16KB |
| **影响** | 任何在管线后段的 task-scoped agent 步骤（pipeline vars 多时）都会触发 |
| **修复** | 将 `enforce_runner_policy` 中的命令长度上限从 16,384 → 131,072 字节 |
| **文件** | `core/src/runner.rs:361`, `core/src/scheduler.rs:550` |
| **提交** | `f1d1324` |
| **风险** | 低。macOS ARG_MAX ~1MB，命令通过 `Command::arg()` 传递，不受 shell 限制 |

**后续改进建议**:
- 可以考虑仅将步骤 `from_var` 引用的变量注入 prompt，而非全部 pipeline vars
- 或者将 prompt 通过 stdin pipe 传递而非命令行参数

---

## 8. 资源消耗

### 8.1 时间分布

| 阶段 | 耗时 | 占比 |
|------|------|------|
| evo_plan (方案设计) | 96s | 7% |
| evo_implement × 2 (实现) | 358s | 25% |
| evo_benchmark × 2 (评估) | 346s | 24% |
| evo_apply_winner (应用) | 279s | 19% |
| evo_align_tests (测试对齐) | 288s | 20% |
| self_test (最终验证) | 56s | 4% |
| 引擎 overhead | ~10s | <1% |
| **合计** | **~1433s** | **100%** |

### 8.2 Agent 输出量

| 步骤 | 输出大小 |
|------|----------|
| evo_plan | 204 KB |
| evo_implement (A) | 115 KB |
| evo_implement (B) | 268 KB |
| evo_benchmark (A) | 74 KB |
| evo_benchmark (B) | 88 KB |
| evo_apply_winner | 160 KB |
| evo_align_tests | 118 KB |
| **合计** | **~1,002 KB** |

### 8.3 日志总占用

任务日志目录总大小: **2.3 MB**

---

## 9. 与 self-bootstrap 对比

| 维度 | self-bootstrap | self-evolution |
|------|---------------|----------------|
| 管线长度 | 8 步 × 2 cycles | 10 步 × 1 cycle |
| 实现路径 | 单一线性 | 2 候选竞争 |
| 方案探索 | 无 | evo_plan + generate_items |
| 选择机制 | 无 | item_select (max score) |
| Agent 调用次数 | ~8 | 7 (plan + impl×2 + bench×2 + apply + align) |
| 总耗时 | ~30-45 min | ~24 min |
| 代码质量保障 | self_test + loop_guard | benchmark 评分 + self_test |
| 适用场景 | 确定性任务，需迭代打磨 | 探索性任务，需方案对比 |

---

## 10. 已知局限性与改进方向

### 10.1 当前局限

1. **Score capture 不够精细**: 当前两个 benchmark 的 score 在 pipeline 中都存为 `"score": "0"`（被覆盖），item_select 实际依赖的是 `total_score` 字段被 benchmark agent 写入到 JSON 输出中。后续应改进 score capture 机制。

2. **语法偏移无惩罚**: 方案 B 的 implement agent 偏离了 plan 中指定的语法（`|||` 代替 `|`），但 benchmark 没有检测这种偏移。后续可在 benchmark prompt 中增加「与 plan 一致性」评分维度。

3. **Winner 代码未被 commit**: `evo_apply_winner` 应用了代码变更但没有 git commit。需要后续人工审查后决定是否保留。

4. **串行执行**: `max_parallel=1` 使两个候选串行执行，总时间约为并行的 2 倍。

5. **依赖引入判断**: 方案 A 引入了 `regex` crate，而课题要求「不引入外部模板引擎依赖」。regex 是否算「模板引擎」是可争论的，但 agent 合理地判断 regex 是通用工具库而非模板引擎。

### 10.2 改进方向

1. **Selective var injection**: 仅将步骤 `from_var` 显式引用的变量注入 prompt，减少命令体积
2. **Stdin piping**: 大 prompt 通过 stdin 传递而非命令行参数
3. **Score capture 增强**: 支持从 agent JSON output 中提取数值 score 字段
4. **Parallel candidates**: 当资源允许时，两个候选并行执行
5. **Plan conformance scoring**: 在 benchmark 中增加方案一致性检查

---

## 11. 结论

**Self-evolution workflow 已成功完成首次端到端实测**。

核心机制全部验证通过：
- 候选方案生成 (generate_items) 产出了有意义的分化
- 竞争评估基于客观指标（编译/测试/clippy/diff 大小）
- 选择结果合理（精简方案胜出，符合工程直觉）
- 胜出方案成功落地并通过最终验证

发现并修复了 1 个新 bug（runner command length limit），所有已修复的 7 个 bug 均在本次运行中稳定工作。

**self-evolution 管线从概念设计到端到端实测完成，共经历 8 个修复提交，3 次迭代运行。它为 orchestrator 提供了一种新的 AI-native 代码进化路径：不需要人工预判最优方案，由引擎自主探索、评估、选择。**
