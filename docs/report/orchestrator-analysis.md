# Orchestrator 深度分析报告

**分析日期**: 2026-02-21
**分析角度**: Agent Orchestration
**分析对象**: orchestrator/src-tauri/src/ 全模块

---

## 执行摘要

本报告从 **Agent Orchestration** 角度对 Orchestrator 进行深度分析。经过对最新实现的全面审查，当前系统已从早期原型演进为具备 K8s 风格资源管理、CEL 表达式引擎、能力路由和成本偏好选择的工作流编排平台。但本质上仍是**流程编排**（Workflow Orchestration）而非真正的 **Agent Orchestration**。主要发现包括：agent 选择策略有限、健康状态管理粗糙、缺乏多 agent 协作机制、并发控制缺失等。

---

## 1. Agent 选择策略有限 — 基于成本的静态排序

### 现状描述

系统已实现基于能力（capability）的 agent 选择和成本偏好排序：

```rust
// main.rs:1080-1119
fn select_agent_by_capability(
    capability: &str,
    agents: &HashMap<String, AgentConfig>,
    cost_preference: &Option<CostPreference>,
) -> Result<(String, String)> {
    let mut candidates: Vec<_> = agents
        .iter()
        .filter(|(_, cfg)| cfg.supports_capability(capability))
        .collect();
    // ...
    match pref {
        CostPreference::Performance => { /* sort by cost ascending */ }
        CostPreference::Quality => { /* sort by cost descending */ }
        CostPreference::Balance => { /* sort by deviation from 50 */ }
    }
}
```

AgentConfig 已具备结构化的元数据和能力声明：

```rust
// main.rs:127-136
struct AgentConfig {
    metadata: AgentMetadata,       // name, description, version, cost
    capabilities: Vec<String>,     // 能力标签列表
    templates: HashMap<String, String>,  // 能力 -> 命令模板
    preference: AgentPreference,   // success_rate, avg_duration_ms, total_runs
}
```

### 仍存在的问题

- **选择仅基于静态 cost 字段排序**，`AgentPreference` 中的 `success_rate`、`avg_duration_ms` 等运行时指标未被 `select_agent_by_capability` 使用
- `run_phase_with_rotation` (main.rs:5104) 的重试逻辑会在候选列表中循环，但每次重试仍调用 `select_agent_by_capability`，可能重复选择同一个不健康的 agent（健康检查在选择后执行）
- 缺乏基于历史成功率的加权选择
- 缺乏 agent 负载感知

### 影响

当多个 agent 支持同一 capability 时，选择策略过于简单，无法根据实际运行表现优化分配。

---

## 2. Agent Health 状态管理粗糙

### 现状描述

健康状态基于连续错误次数和固定时间窗口：

```rust
// main.rs:971-972
const DISEASE_DURATION_HOURS: i64 = 5;
const CONSECUTIVE_ERROR_THRESHOLD: u32 = 2;
```

- 连续 2 次错误标记为 diseased，持续 5 小时
- 超时直接标记为 diseased（无阈值）
- 成功执行时重置 `consecutive_errors`

### 问题分析

- 疾病持续时间固定 5 小时，无指数退避或动态调整
- 不区分错误类型（超时 vs 认证失败 vs 资源不足）
- 超时直接 diseased 但普通错误需要 2 次——策略不一致
- 健康状态是全局的，不区分 capability 维度（agent 在 `qa` 失败会影响 `fix` 的可用性）
- `select_agent_by_capability` 函数本身**不检查健康状态**，健康过滤仅在 `run_phase_with_rotation` 的重试循环中隐式处理

### 影响

Agent 可能因单个 capability 的临时故障而在所有 capability 上被排除 5 小时。

---

## 3. 缺乏真正的 Agent 通信/协作机制

### 现状描述

Workflow 是线性执行的，每个 item 按步骤序列处理：

```
init_once → [per item: qa → ticket_scan → fix → retest] → loop_guard
```

Agent 之间的"通信"仅通过：
1. **Ticket 文件** — QA agent 生成 ticket，fix agent 读取
2. **Exit code** — 上一步的退出码通过 prehook context 传递给下一步的条件判断
3. **文件系统** — agent 直接读写工作区文件

### 问题分析

- 缺乏结构化的 agent 间消息传递
- 无共享内存/上下文机制
- Agent 输出（stdout/stderr）仅写入日志文件，不解析为结构化数据传递
- 无法实现 Planner-Executor、Critic-Actor 等多 agent 协作模式

### 影响

Agent 间只能通过文件系统进行粗粒度的信息传递，无法支持复杂的协作场景。

---

## 4. Prehook 条件系统 — 有力但仍是静态编排

### 现状描述

系统引入了基于 CEL（Common Expression Language）的 prehook 条件执行机制：

```rust
// main.rs:1958-1997
fn evaluate_step_prehook(
    state: &InnerState,
    app: Option<&AppHandle>,
    prehook: Option<&StepPrehookConfig>,
    context: &StepPrehookContext,  // cycle, ticket counts, exit codes, status flags
) -> Result<bool>
```

可用的 CEL 上下文变量包括：`cycle`、`active_ticket_count`、`new_ticket_count`、`qa_exit_code`、`fix_exit_code`、`retest_exit_code`、`qa_failed`、`fix_required` 等。

前端还提供了可视化 prehook 构建器和模拟测试功能。

### 仍存在的问题

- Prehook 只能决定**跳过**步骤，不能动态**添加**步骤或改变步骤顺序
- 步骤序列在任务创建时完全确定（`TaskExecutionPlan`），运行时不可修改
- 缺乏基于 LLM 的动态步骤规划能力
- CEL 变量集合固定，无法引用 agent 输出内容

### 影响

Prehook 增强了条件控制力，但本质仍是预定义 workflow 的静态调度，而非基于运行时结果的动态编排。

---

## 5. Template 渲染能力有限

### 现状描述

模板使用简单的字符串替换：

```rust
// qa_utils.rs — render_template()
// 占位符: {rel_path}, {ticket_paths}

// main.rs — render_loop_guard_template()
// 占位符: {task_id}, {cycle}, {unresolved_items}
```

### 问题分析

- 占位符集合固定，不可扩展
- 没有条件渲染（if/else）
- 没有循环/迭代支持
- Agent 无法在模板中引用前序步骤的输出
- 不同步骤类型的占位符集合不同，缺乏统一

### 影响

复杂场景需要硬编码大量变体模板。无法实现上下文感知的动态 prompt 生成。

---

## 6. Loop Guard 决策路径清晰但扩展性有限

### 现状描述

Loop guard 决策分为三层，优先级明确：

```rust
// main.rs:4518-4536
fn evaluate_loop_guard_rules(...) -> Option<(bool, String)> {
    match loop_policy.mode {
        LoopMode::Once => Some((false, "once_mode")),
        LoopMode::Infinite => {
            if !loop_policy.guard.enabled { return Some((true, "guard_disabled")); }
            if current_cycle >= max_cycles { return Some((false, "max_cycles_reached")); }
            None  // 交给后续逻辑
        }
    }
}
```

后续依次检查：guard step 执行结果 → `stop_when_no_unresolved` 配置 → 默认继续。

### 问题分析

- 决策逻辑相比早期版本已更清晰（规则优先 → agent 决定 → 默认行为）
- 但 guard agent 的输出解析较脆弱 — 通过 `parse_guard_agent_decision` 从 stdout 解析 JSON 或关键词
- Guard step 与普通 workflow step 耦合在同一执行循环中
- 缺乏可观测的决策追踪（虽然有 event 记录，但无统一的决策审计日志）

### 影响

Loop guard 可工作，但 agent 输出的解析方式脆弱，对输出格式敏感。

---

## 7. 并发控制缺失

### 现状描述

`spawn_task_runner` 对每个任务创建独立的 tokio task：

```rust
// main.rs:4094-4103
async fn spawn_task_runner(state: Arc<InnerState>, app: AppHandle, task_id: String) -> Result<()> {
    // 检查 task_id 是否已在运行（防重复），但无全局并发限制
    running.insert(task_id.clone(), RunningTask::new());
    tokio::spawn(async move { ... });
}
```

### 问题分析

- 没有全局并发任务数限制（无 semaphore/token bucket）
- 没有 agent 级别的并发控制（同一 agent 可被多个任务同时调用）
- Agent 执行有 idle timeout（10 分钟，`IDLE_TIMEOUT_SECS = 600`），但无硬性执行超时
- 防重复启动仅检查同一 task_id，不限制总任务数

### 影响

多任务并发可能耗尽系统资源或同时调用同一外部 agent 导致竞争。

---

## 8. 结果验证仅基于 Exit Code

### 现状描述

Agent 执行结果的成功/失败判断：

```rust
// main.rs:5170-5179 (run_phase_with_rotation 中)
if result.timed_out {
    mark_agent_diseased(state, app, &agent_id);
    continue;
}
if !result.success {
    let errors = increment_consecutive_errors(state, app, &agent_id);
    if errors >= CONSECUTIVE_ERROR_THRESHOLD {
        mark_agent_diseased(state, app, &agent_id);
        continue;
    }
}
```

Workflow finalization 使用 CEL 规则确定 item 最终状态：

```rust
// main.rs:2270-2283
fn resolve_workflow_finalize_outcome(
    finalize: &WorkflowFinalizeConfig,
    context: &ItemFinalizeContext,
) -> Result<Option<WorkflowFinalizeOutcome>>
```

### 问题分析

- Exit code 是唯一的执行成功指标
- Agent 可能返回 exit code 0 但输出无效内容
- Finalization 规则只能基于 ticket 文件存在性和步骤执行状态判断，不检查 agent 输出质量
- 缺乏对 agent 输出的结构化解析和验证
- 无质量评分或置信度评估

### 影响

无法检测"静默失败" — agent 返回成功但实际未完成预期工作。

---

## 9. K8s 风格资源管理 — 架构进步但能力有限

### 现状描述

系统已实现 K8s 风格的声明式资源管理：

```rust
// cli_types.rs — 三种资源类型
enum ResourceKind { Workspace, Agent, Workflow }

// resource.rs — Resource trait
trait Resource {
    fn kind(&self) -> ResourceKind;
    fn validate(&self) -> Result<()>;
    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult>;
    fn to_yaml(&self) -> Result<String>;
}
```

CLI 支持 `apply`、`get`、`describe` 等操作，配置有版本管理。

### 仍存在的问题

- 资源变更是**全量替换**（apply 覆盖），缺乏 patch/merge 语义
- 无 watch 机制 — 资源变更不会自动触发运行中任务的重新调度
- 无 namespace 隔离（所有资源在同一平面）
- 缺乏资源间依赖声明和验证（workflow 引用不存在的 agent 只在运行时报错）

### 影响

资源管理提供了良好的声明式接口，但缺乏 K8s 的动态调谐（reconciliation）能力。

---

## 问题汇总

| 问题类别 | 严重程度 | 影响 | 现状 |
|---------|---------|------|------|
| Agent 选择策略有限 | 中 | 效率低 | 有 capability + cost 排序，但未用运行时指标 |
| 健康状态管理粗糙 | 中 | 可靠性低 | 固定阈值+时间窗口，无错误类型区分 |
| 无多 agent 协作 | 高 | 功能受限 | 仅通过 ticket 文件和 exit code 传递信息 |
| Prehook 仅跳过无法扩展 | 中 | 适应性差 | CEL 条件引擎已就位，但只能跳过步骤 |
| 模板能力有限 | 中 | 灵活性差 | 固定占位符，无条件/循环渲染 |
| 并发控制缺失 | 中 | 资源风险 | 无全局并发限制 |
| 结果验证仅靠 exit code | 高 | 质量无保证 | 无输出内容验证 |
| 资源管理缺乏动态调谐 | 低 | 扩展受限 | K8s 风格接口已有，缺 watch/reconcile |

---

## 结论

相比早期版本，当前 orchestrator 在以下方面有显著进步：
- **Agent 抽象**: 从硬编码 phase 绑定演进为 capability + metadata + cost preference 模型
- **条件控制**: 引入 CEL 表达式引擎实现灵活的步骤条件执行
- **资源管理**: K8s 风格的声明式配置管理和版本控制
- **可观测性**: 结构化事件系统（prehook 评估、guard 决策、状态变更）

但要构建真正的 Agent Orchestration 系统，仍需解决：

1. **智能选择**: 利用已有的 `AgentPreference` 数据实现基于运行时表现的加权选择
2. **协作机制**: 引入结构化的 agent 间消息传递，而非仅依赖文件系统
3. **动态编排**: 允许运行时修改步骤序列，支持基于 LLM 的自适应规划
4. **输出验证**: 对 agent 输出进行结构化解析和质量评估
5. **并发治理**: 添加全局和 agent 级并发限制

---

*Report generated from codebase analysis — 2026-02-21*
