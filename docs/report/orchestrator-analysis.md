# Orchestrator 深度分析报告

**分析日期**: 2026-02-21
**更新日期**: 2026-02-21 (v1.1 - 补充 Visual Rules/Finalize 章节，修正 performance_score 描述)
**分析角度**: Agent Orchestration
**分析对象**: orchestrator/src-tauri/src/ 全模块

---

## 执行摘要

本报告从 **Agent Orchestration** 角度对 Orchestrator 进行深度分析。经过对最新实现的全面审查，当前系统已从早期原型演进为具备 K8s 风格资源管理、CEL 表达式引擎、能力路由和成本偏好选择的工作流编排平台。但本质上仍是**流程编排**（Workflow Orchestration）而非真正的 **Agent Orchestration**。主要发现包括：健康状态管理已改进为能力级别跟踪、Agent 选择支持运行时指标评分、但仍缺乏多 agent 协作机制和输出验证。

---

## 1. Agent 选择策略 — 已升级为运行时评分

### 现状描述

系统使用 `select_agent_advanced` 进行多因子评分选择：

#### 高级选择 (`select_agent_advanced`, main.rs:1043-1127)
- 使用 `metrics::calculate_agent_score` (metrics.rs:221) 进行多因子评分
- 支持多种选择策略：

```rust
// metrics.rs:68-85
pub enum SelectionStrategy {
    CostBased,           // 静态成本排序（遗留行为）
    SuccessRateWeighted, // 成功率加权选择
    PerformanceFirst,    // 延迟优先
    Adaptive,            // 自适应评分（可配置权重）
    LoadBalanced,        // 负载均衡
    CapabilityAware,     // 能力感知健康跟踪
}
```

评分因子包括：
- **cost_score**: 100 - cost (0-100)
- **success_rate_score**: successful_runs / total_runs * 100
- **performance_score**: min(60000 / avg_duration_ms, 100) (反比，上限100)
- **load_penalty**: -current_load * 10 (最大 -50)
- **health_penalty**: -100 (diseased) 或 -consecutive_errors * 15

> **注**: `p95_duration_ms` 字段已在 `AgentMetrics` 中收集，但当前评分函数尚未使用。

选择流程：每个候选 agent 使用其自身配置的 `selection.strategy` 和 `selection.weights`，若未配置则回退到全局 `cost_preference`。

### 已修复

- ✅ 删除了从未使用的 `AgentPreference` 结构体
- ✅ 删除了遗留的 `select_agent_by_capability` 函数
- ✅ `AgentSelectionConfig`（selection 字段）现在真正生效

### 仍存在的问题

- 缺乏基于历史成功率的加权选择（虽然有 SuccessRateWeighted 策略，但需要配置启用）
- 选择策略配置较为分散（全局 cost_preference vs agent 级别 selection）

---

## 2. Agent Health 状态管理 — 已改进为能力级别跟踪

### 现状描述

健康状态基于连续错误次数和固定时间窗口：

```rust
// main.rs:885-886
const DISEASE_DURATION_HOURS: i64 = 5;
const CONSECUTIVE_ERROR_THRESHOLD: u32 = 2;
```

- 连续 2 次错误标记为 diseased，持续 5 小时
- 超时直接标记为 diseased（无阈值）
- 成功执行时重置 `consecutive_errors`

### 能力级别健康跟踪

系统已实现能力级别健康跟踪：

```rust
// metrics.rs:33-53
pub struct CapabilityHealth {
    pub success_count: u32,
    pub failure_count: u32,
    pub last_error_at: Option<DateTime<Utc>>,
}

impl CapabilityHealth {
    pub fn success_rate(&self) -> f32 {
        let total = self.success_count + self.failure_count;
        if total == 0 { return 0.5; }
        self.success_count as f32 / total as f32
    }
}

// metrics.rs:55-66
pub struct AgentHealthState {
    pub diseased_until: Option<DateTime<Utc>>,
    pub consecutive_errors: u32,
    pub total_lifetime_errors: u32,
    pub capability_health: HashMap<String, CapabilityHealth>,
}
```

选择时使用 `is_capability_healthy` (metrics.rs:320-339) 进行能力级别检查：
- Agent 可以全局 diseased，但在特定 capability 上 healthy（如果该 capability success_rate >= 0.5）
- 实现了 hybrid 健康跟踪模型

### 仍存在的问题

- 疾病持续时间固定 5 小时，无指数退避或动态调整
- 不区分错误类型（超时 vs 认证失败 vs 资源不足）
- 超时直接 diseased 但普通错误需要 2 次——策略不一致

---

## 3. 缺乏真正的 Agent 通信/协作机制 — ✅ 已解决

### 现状描述

Workflow 是线性执行的，每个 item 按步骤序列处理：

```
init_once → [per item: qa → ticket_scan → fix → retest] → loop_guard
```

Agent 之间的"通信"仅通过：
1. **Ticket 文件** — QA agent 生成 ticket，fix agent 读取
2. **Exit code** — 上一步的退出码通过 prehook context 传递给下一步的条件判断
3. **文件系统** — agent 直接读写工作区文件

### ✅ 已实现解决方案

**新增 collab.rs 模块**，提供：

1. **AgentOutput 结构** - 替代纯 exit_code：
```rust
pub struct AgentOutput {
    pub run_id: Uuid,
    pub agent_id: String,
    pub phase: String,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub artifacts: Vec<Artifact>,      // 结构化产物
    pub metrics: ExecutionMetrics,      // 执行指标
    pub confidence: f32,              // 置信度 0-1
    pub quality_score: f32,            // 质量评分 0-1
}
```

2. **Artifact 类型** - 替代 ticket 文件扫描：
   - Ticket (severity, category)
   - CodeChange (files)
   - TestResult (passed, failed)
   - Analysis (findings)
   - Decision (choice, rationale)

3. **MessageBus** - Agent 间消息传递：
   - publish/subscribe 模式
   - 支持 Request/Response/Publish 消息类型
   - 支持 Broadcast 广播

4. **AgentContext** - 共享运行时上下文：
   - upstream_outputs: 上游 Agent 输出
   - artifacts: 产物注册表
   - shared_state: 键值存储
   - 增强模板渲染支持 `{upstream[0].confidence}`, `{shared_state.key}` 等

5. **Artifact 解析器** - 从 stdout/stderr 自动解析：
   - JSON 格式 artifacts
   - 纯文本 `[TICKET: severity=high]` 标记

6. **StepPrehookContext 扩展**：
   - `qa_confidence`, `qa_quality_score`
   - `fix_has_changes`
   - `upstream_artifacts`

### 影响

现在支持：
- 结构化的 agent 输出和产物传递
- Planner-Executor、Critic-Actor 等多 agent 协作模式
- 基于置信度和质量评分的动态决策
- MessageBus 发布订阅通信

---

## 4. Prehook 条件系统 — 有力但仍是静态编排

### 现状描述

系统引入了基于 CEL（Common Expression Language）的 prehook 条件执行机制：

```rust
// main.rs:2020-2071
fn evaluate_step_prehook(
    state: &InnerState,
    app: Option<&AppHandle>,
    prehook: Option<&StepPrehookConfig>,
    context: &StepPrehookContext,
) -> Result<bool>
```

CEL 上下文变量包括 (main.rs:2062-2096)：
- `task_id`, `task_item_id`, `cycle`, `step`, `qa_file_path`
- `item_status`, `task_status`
- `qa_exit_code`, `fix_exit_code`, `retest_exit_code`
- `active_ticket_count`, `new_ticket_count`
- `qa_failed`, `fix_required`

内置预设 (README):
- `ticket_scan`: always run
- `fix`: `active_ticket_count > 0`
- `retest`: `active_ticket_count > 0 && fix_exit_code == 0`

前端还提供了可视化 prehook 构建器和模拟测试功能。

### 仍存在的问题

- Prehook 只能决定**跳过**步骤，不能动态**添加**步骤或改变步骤顺序
- 步骤序列在任务创建时完全确定（`TaskExecutionPlan`），运行时不可修改
- 缺乏基于 LLM 的动态步骤规划能力
- CEL 变量集合固定，无法引用 agent 输出内容

### 影响

Prehook 增强了条件控制力，但本质仍是预定义 workflow 的静态调度，而非基于运行时结果的动态编排。

### 补充：Visual Rules 编辑器

前端提供了两种 prehook 编辑模式：

1. **Visual Rules（默认）** — 无需编写 CEL，通过可视化表单配置条件
   - 可用字段：`active_ticket_count`, `new_ticket_count`, `cycle`
   - 步骤特定字段：`qa_exit_code`, `fix_exit_code`, `retest_exit_code`
   - 布尔标志：`qa_failed`, `fix_required`
   - UI 自动生成 CEL 表达式

2. **Advanced CEL（可选）** — 允许用户直接编写 CEL 表达式

两种模式的运行时评估逻辑相同（后端统一使用 `evaluate_step_prehook_expression`），保证一致性。UI 还提供 **Simulate** 功能可在保存前测试 CEL 表达式。

### 补充：Finalize Rules

Workflow 的最终状态决策由 `workflow.finalize.rules[]` 控制：

```yaml
workflows:
  qa_fix_retest:
    finalize:
      rules:
        - when: "qa_exit_code == 0 && active_ticket_count == 0"
          status: qa_passed
        - when: "fix_exit_code == 0 && active_ticket_count == 0"
          status: verified
        - when: "retest_exit_code != 0"
          status: unresolved
```

规则按顺序匹配（first-match-wins），支持的状态：`skipped`, `qa_passed`, `fixed`, `verified`, `unresolved`。

Finalize 与 Prehook 互补：
- **Prehook**: 决定步骤是否执行
- **Finalize**: 决定 item 最终状态

---

## 5. Template 渲染能力有限

### 现状描述

模板使用简单的字符串替换：

```rust
// qa_utils.rs:35-39
pub fn render_template(template: &str, rel_path: &str, ticket_paths: &[String]) -> String {
    template
        .replace("{rel_path}", rel_path)
        .replace("{ticket_paths}", &ticket_paths.join(" "))
}

// main.rs:4684-4694
fn render_loop_guard_template(template: &str, task_id: &str, cycle: u32, unresolved_items: i64) -> String {
    template
        .replace("{task_id}", task_id)
        .replace("{cycle}", &cycle.to_string())
        .replace("{unresolved_items}", &unresolved_items.to_string())
}
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

## 7. 并发控制 — 部分实现

### 现状描述

`spawn_task_runner` 对每个任务创建独立的 tokio task：

```rust
// main.rs:4200-4203
async fn spawn_task_runner(state: Arc<InnerState>, app: AppHandle, task_id: String) -> Result<()> {
    // 检查 task_id 是否已在运行（防重复），但无全局并发限制
    running.insert(task_id.clone(), RunningTask::new());
    tokio::spawn(async move { ... });
}
```

### 已实现的并发控制

- `SelectionRequirement.max_load = 5` (main.rs:1152) — 每个 agent 最大并发负载
- `load_penalty` 在评分函数中惩罚高负载 agent (metrics.rs:254-259)
- `AgentMetrics.current_load` 追踪当前执行数

### 仍存在的问题

- 没有全局任务数限制（无 semaphore/token bucket）
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

Workflow finalization 使用 CEL 规则确定 item 最终状态。

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
| Agent 选择策略有限 | 低 | 效率低 | ✅ 已实现运行时指标评分 + agent 级别选择配置 |
| 健康状态管理粗糙 | 低 | 可靠性提升 | 已实现能力级别健康跟踪 |
| 无多 agent 协作 | 高 | 功能受限 | ✅ 已实现 AgentOutput + MessageBus + Artifact 解析 |
| Prehook 仅跳过无法扩展 | 中 | 适应性差 | CEL 条件引擎已就位，但只能跳过步骤 |
| 模板能力有限 | 中 | 灵活性差 | ✅ 已支持 upstream[]/shared_state 占位符 |
| 并发控制缺失 | 低 | 资源风险 | 已实现 agent 级负载限制，缺全局限制 |
| 结果验证仅靠 exit code | 高 | 质量无保证 | ✅ 已支持 confidence/quality_score/artifacts |
| 资源管理缺乏动态调谐 | 低 | 扩展受限 | K8s 风格接口已有，缺 watch/reconcile |

---

## 结论

相比早期版本，当前 orchestrator 在以下方面有显著进步：
- **Agent 抽象**: 演进为 capability + metadata + cost + runtime metrics 模型
- **智能选择**: `select_agent_advanced` 使用多因子评分（cost + success_rate + performance + load）
- **健康跟踪**: 实现了能力级别健康跟踪（capability_health），支持全局 diseased 但能力 healthy
- **条件控制**: 引入 CEL 表达式引擎实现灵活的步骤条件执行
- **资源管理**: K8s 风格的声明式配置管理和版本控制
- **可观测性**: 结构化事件系统（prehook 评估、guard 决策、状态变更）
- **Agent 协作**: AgentOutput + MessageBus + Artifact 解析实现结构化通信

> **2026-02-21 更新**: 问题 #3 (无多 agent 协作) 和 #8 (结果验证仅靠 exit code) 已通过 collab 模块解决。

要构建真正的 Agent Orchestration 系统，仍需解决：

1. **动态编排**: 允许运行时修改步骤序列，支持基于 LLM 的自适应规划
2. **并发治理**: 添加全局任务并发限制
3. **资源调谐**: 实现 K8s 风格的 watch/reconcile 机制

---

*Report generated from codebase analysis — 2026-02-21*
*Updated: 2026-02-21 (v1.1 - 补充 Visual Rules/Finalize 章节，修正 performance_score 描述)*
*Fix applied: 2026-02-21 (removed unused AgentPreference, integrated selection config)*
