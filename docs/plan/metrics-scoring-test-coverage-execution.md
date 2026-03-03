# self-bootstrap 课题执行计划：metrics.rs Agent 评分策略测试覆盖

---

## 1. 任务目标

> 课题名称：`Expand metrics.rs agent scoring test coverage`
>
> 背景：
> `core/src/metrics.rs` 实现了 6 种 SelectionStrategy（CostBased, SuccessRateWeighted, PerformanceFirst, Adaptive, LoadBalanced, CapabilityAware）用于 agent 选择评分，以及 MetricsCollector 用于 EMA 指标追踪。当前仅有 5 个单元测试，覆盖了基本功能但缺少：策略间对比验证、边界条件（零运行次数、极端 cost、高负载）、EMA 收敛行为、health penalty 计算、CapabilityHealth 边界。
>
> 本轮任务目标：
> 为 `core/src/metrics.rs` 补齐单元测试，覆盖全部 6 种 SelectionStrategy 的评分公式验证、MetricsCollector 的 EMA 收敛行为、边界条件处理、health/load penalty 计算，使测试数量从 5 增加到至少 18。
>
> 约束：
> 1. 仅修改 `core/src/metrics.rs` 的 `#[cfg(test)] mod tests` 部分，不修改任何生产代码逻辑。
> 2. 保留已有 5 个测试不变。
> 3. 最终目标是：所有 6 种策略各有至少 1 个专项测试，EMA 行为有收敛验证，边界条件（零运行、None metrics、高负载、diseased agent）各有覆盖。

### 1.1 预期产出

1. 一份实现计划（由 `plan` 步骤生成）。
2. QA 文档判断（由 `qa_doc_gen` 判断——预期为纯测试任务，不需要新 QA 文档）。
3. `core/src/metrics.rs` tests 模块中新增 13+ 个单元测试。
4. 自举回归验证结果。

### 1.2 非目标

不修改评分公式或生产逻辑。不重构现有代码结构。不添加新的 public API。

---

## 2. 执行方式

标准 `self-bootstrap` 链路：

```text
plan -> qa_doc_gen -> implement -> self_test -> qa_testing -> ticket_fix -> align_tests -> doc_governance -> loop_guard
```

---

## 3. 启动步骤

### 3.1 构建并初始化运行时

```bash
cd /Volumes/Yotta/ai_native_sdlc
cd core && cargo build --release && cd ..
./scripts/orchestrator.sh db reset -f --include-config --include-history
./scripts/orchestrator.sh init -f
./scripts/orchestrator.sh apply -f docs/workflow/self-bootstrap.yaml
```

### 3.2 验证资源已加载

```bash
./scripts/orchestrator.sh get workspace
./scripts/orchestrator.sh get workflow
./scripts/orchestrator.sh get agent
```

### 3.3 创建任务

```bash
./scripts/orchestrator.sh task create \
  -n "metrics-test-coverage" \
  -w self -W self-bootstrap \
  --no-start \
  -g "Expand unit test coverage for core/src/metrics.rs. Currently only 5 tests exist for 6 SelectionStrategy variants and MetricsCollector. Add tests for: (1) each SelectionStrategy scoring formula with known inputs and expected outputs, (2) EMA convergence behavior over multiple record_success/record_failure calls, (3) boundary conditions — zero total_runs, None metrics, None health, max load, (4) health penalty — diseased agent, consecutive errors, (5) CapabilityHealth::success_rate with zero total, (6) load increment/decrement boundaries. Target: at least 18 total tests. Only modify the #[cfg(test)] mod tests section, do not change production code." \
  -t core/src/metrics.rs
```

---

## 4. 成功判定

1. orchestrator 完整跑完 self-bootstrap 流程。
2. `core/src/metrics.rs` tests 模块测试数量 >= 18。
3. 所有 6 种 SelectionStrategy 各有专项测试。
4. `cargo test -p agent-orchestrator metrics` 全部通过。
5. 无未解决 ticket。

---

## 5. 异常处理

若 plan 偏离"仅添加测试"目标（如试图重构评分公式），判定为偏题。
若 implement 修改了 `#[cfg(test)]` 之外的代码，判定为越界。
